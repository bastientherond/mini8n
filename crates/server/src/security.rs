//! Protections transverses : anti-CSRF (Origin), limitation de débit, IP cliente.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::AppError;

/// Seau à jetons en mémoire (par clé : IP, e-mail…).
pub struct RateLimiter {
    capacity: f64,
    per_second: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl RateLimiter {
    /// `capacity` requêtes en rafale, rechargées à raison de `per_minute` par minute.
    pub fn new(capacity: u32, per_minute: u32) -> Self {
        Self {
            capacity: capacity as f64,
            per_second: per_minute as f64 / 60.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Consomme un jeton ; en cas de dépassement, renvoie le délai d'attente (s).
    pub fn check(&self, key: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > 50_000 {
            let (cap, rate) = (self.capacity, self.per_second);
            buckets.retain(|_, (tokens, at)| {
                *tokens + now.duration_since(*at).as_secs_f64() * rate < cap
            });
        }
        let (tokens, at) = buckets
            .entry(key.to_string())
            .or_insert((self.capacity, now));
        *tokens =
            (*tokens + now.duration_since(*at).as_secs_f64() * self.per_second).min(self.capacity);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - *tokens) / self.per_second).ceil() as u64)
        }
    }

    pub fn limit(&self, key: &str) -> Result<(), AppError> {
        self.check(key).map_err(AppError::TooManyRequests)
    }
}

/// Adresse IP du client (en-tête `X-Real-IP` posé par nginx si `TRUST_PROXY`).
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.config.trust_proxy
            && let Some(ip) = parts
                .headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty() && v.len() <= 45)
        {
            return Ok(ClientIp(ip.to_string()));
        }
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|| "inconnue".into());
        Ok(ClientIp(ip))
    }
}

pub fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(256).collect())
}

/// Anti-CSRF : toute requête modifiant des données doit provenir de l'origine du site.
/// (En complément du cookie `SameSite=Strict` et de l'exigence d'un corps JSON.)
pub async fn require_same_origin(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    // Les webhooks sont appelés par des services tiers (sans en-tête Origin) :
    // ils sont protégés par leur jeton secret, leur secret facultatif et la limitation de débit.
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        || req.uri().path().starts_with("/hooks/")
    {
        return next.run(req).await;
    }
    let headers = req.headers();
    if headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "cross-site")
    {
        return AppError::Forbidden("Requête inter-sites refusée".into()).into_response();
    }
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    match origin {
        Some(o)
            if state
                .config
                .public_origins
                .iter()
                .any(|allowed| allowed == o) =>
        {
            next.run(req).await
        }
        _ => {
            tracing::warn!(origin = ?origin, path = %req.uri().path(), "origine refusée");
            AppError::Forbidden("Origine non autorisée".into()).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;

    #[test]
    fn rate_limiter_blocks_bursts() {
        let rl = RateLimiter::new(3, 60);
        assert!(rl.check("a").is_ok());
        assert!(rl.check("a").is_ok());
        assert!(rl.check("a").is_ok());
        assert!(rl.check("a").is_err());
        assert!(rl.check("b").is_ok());
    }
}
