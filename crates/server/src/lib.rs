//! API HTTP de mini8n.

pub mod auth;
pub mod config;
pub mod error;
pub mod security;
pub mod validate;
pub mod workflows;
pub mod workspaces;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::routing::{get, patch, post};
use axum::{Json, Router, middleware};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::Semaphore;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

pub use config::Config;
use error::AppError;
use security::RateLimiter;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Limiteurs de débit (en mémoire, par instance).
pub struct Limits {
    pub login_ip: RateLimiter,
    pub login_account: RateLimiter,
    pub register_ip: RateLimiter,
    pub writes: RateLimiter,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            login_ip: RateLimiter::new(20, 10),
            login_account: RateLimiter::new(10, 5),
            register_ip: RateLimiter::new(5, 2),
            writes: RateLimiter::new(60, 60),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub limits: Arc<Limits>,
    /// Borne le nombre de calculs Argon2 simultanés (protection CPU / mémoire).
    pub hash_permits: Arc<Semaphore>,
}

impl AppState {
    pub fn new(db: PgPool, config: Config) -> Self {
        Self {
            db,
            config: Arc::new(config),
            limits: Arc::new(Limits::default()),
            hash_permits: Arc::new(Semaphore::new(4)),
        }
    }
}

async fn health(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn not_found() -> AppError {
    AppError::NotFound
}

pub fn app(state: AppState) -> Router {
    use workflows as wfl;
    use workspaces as ws;

    let api = Router::new()
        .route("/health", get(health))
        .route("/auth/config", get(auth::public_config))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/logout-all", post(auth::logout_all))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/workspaces", get(ws::list).post(ws::create))
        .route("/workspaces/{ws}", patch(ws::rename).delete(ws::delete))
        .route(
            "/workspaces/{ws}/members",
            get(ws::members).post(ws::add_member),
        )
        .route(
            "/workspaces/{ws}/members/{user}",
            patch(ws::update_member).delete(ws::remove_member),
        )
        .route(
            "/workspaces/{ws}/workflows",
            get(wfl::list).post(wfl::create),
        )
        .route(
            "/workspaces/{ws}/workflows/{id}",
            get(wfl::get).put(wfl::update).delete(wfl::delete),
        )
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::require_same_origin,
        ))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024));

    Router::new()
        .nest("/api", api)
        .fallback(not_found)
        .with_state(state)
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http())
}
