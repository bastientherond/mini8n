use std::net::SocketAddr;

/// Politique de l'attribut `Secure` du cookie de session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieSecure {
    /// `Secure` (+ préfixe `__Host-`) uniquement si la requête est arrivée en HTTPS
    /// (`X-Forwarded-Proto: https` posé par le reverse proxy de confiance).
    /// Safari refuse les cookies `Secure` sur `http://localhost` : ce mode permet le test local.
    Auto,
    /// Toujours `Secure` (production derrière HTTPS uniquement).
    Always,
    /// Jamais `Secure` (développement uniquement).
    Never,
}

/// Configuration lue depuis l'environnement.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    /// Origine publique du site (ex. `https://mini8n.exemple.com`). Les requêtes
    /// modifiant des données doivent porter cet en-tête `Origin` (anti-CSRF).
    pub public_origins: Vec<String>,
    /// Cookie `Secure` + préfixe `__Host-` : toujours, jamais, ou selon le protocole de la requête.
    pub cookie_secure: CookieSecure,
    /// Faire confiance à `X-Real-IP` (uniquement derrière le reverse proxy nginx).
    pub trust_proxy: bool,
    pub session_ttl_hours: i64,
    /// Désactiver les inscriptions publiques (après création des comptes).
    pub allow_registration: bool,
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    env(key).map_or(default, |v| {
        matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
    })
}

/// Lit un secret depuis `KEY` ou depuis le fichier indiqué par `KEY_FILE` (secrets Docker).
fn secret(key: &str) -> Result<Option<String>, String> {
    if let Some(path) = env(&format!("{key}_FILE")) {
        let v = std::fs::read_to_string(&path).map_err(|e| format!("lecture de {path} : {e}"))?;
        return Ok(Some(v.trim().to_string()));
    }
    Ok(env(key))
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = match secret("DATABASE_URL")? {
            Some(url) => url,
            None => {
                let password = secret("DATABASE_PASSWORD")?
                    .ok_or("DATABASE_URL ou DATABASE_PASSWORD(_FILE) requis")?;
                format!(
                    "postgres://{}:{}@{}:{}/{}",
                    env("DATABASE_USER").unwrap_or_else(|| "mini8n".into()),
                    percent_encode(&password),
                    env("DATABASE_HOST").unwrap_or_else(|| "db".into()),
                    env("DATABASE_PORT").unwrap_or_else(|| "5432".into()),
                    env("DATABASE_NAME").unwrap_or_else(|| "mini8n".into()),
                )
            }
        };
        let public_origins: Vec<String> = env("PUBLIC_ORIGIN")
            .unwrap_or_else(|| "http://localhost:8080".into())
            .split(',')
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(Self {
            bind: env("BIND_ADDR")
                .unwrap_or_else(|| "0.0.0.0:3000".into())
                .parse()
                .map_err(|e| format!("BIND_ADDR invalide : {e}"))?,
            database_url,
            public_origins,
            cookie_secure: match env("COOKIE_SECURE")
                .map(|v| v.to_ascii_lowercase())
                .as_deref()
            {
                None | Some("auto") => CookieSecure::Auto,
                Some("1" | "true" | "yes" | "on" | "always") => CookieSecure::Always,
                Some("0" | "false" | "no" | "off" | "never") => CookieSecure::Never,
                Some(other) => {
                    return Err(format!(
                        "COOKIE_SECURE invalide : {other} (auto, true ou false)"
                    ));
                }
            },
            trust_proxy: env_bool("TRUST_PROXY", false),
            session_ttl_hours: env("SESSION_TTL_HOURS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(24 * 7),
            allow_registration: env_bool("ALLOW_REGISTRATION", true),
        })
    }
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
