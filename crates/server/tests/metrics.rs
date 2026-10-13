//! Supervision (`/api/metrics`). Nécessite PostgreSQL : `TEST_DATABASE_URL=postgres://…`.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use mini8n_server::config::CookieSecure;
use mini8n_server::{AppState, Config, MIGRATOR, app};
use sqlx::PgPool;
use tower::ServiceExt;

const TOKEN: &str = "jeton-de-supervision-de-test-0123456789";

async fn setup(metrics_token: Option<&str>) -> Option<Router> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db = PgPool::connect(&url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
        database_max_connections: 10,
        public_origins: vec!["http://localhost:8080".into()],
        cookie_secure: CookieSecure::Never,
        trust_proxy: false,
        session_ttl_hours: 1,
        allow_registration: true,
        mail: None,
        http_allow_private: false,
        runner_threads: 1,
        execution_timeout_secs: 30,
        executions_retention_days: 30,
        network_allowlist: vec![],
        localhost_alias: None,
        credentials_key: [42u8; 32],
        connectors_url: None,
        connectors_token: None,
        metrics_token: metrics_token.map(String::from),
    };
    Some(app(AppState::new(db, config)))
}

async fn get(app: &Router, token: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().uri("/api/metrics");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn metrics_require_their_token() {
    // Sans jeton configuré : l'URL n'existe pas.
    let Some(app) = setup(None).await else { return };
    assert_eq!(get(&app, Some(TOKEN)).await.0, StatusCode::NOT_FOUND);

    let app = setup(Some(TOKEN)).await.unwrap();
    assert_eq!(get(&app, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        get(&app, Some("mauvais-jeton-mauvais-jeton-mauvais"))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = get(&app, Some(TOKEN)).await;
    assert_eq!(status, StatusCode::OK);
    for line in [
        "mini8n_database_up 1",
        "mini8n_runner_queue 0",
        "mini8n_runner_queue_capacity 200",
        "# TYPE mini8n_executions_24h gauge",
        "mini8n_executions_24h{status=\"error\"}",
        "mini8n_live_rooms 0",
        "mini8n_database_connections_max 10",
    ] {
        assert!(body.contains(line), "« {line} » absent :\n{body}");
    }
}
