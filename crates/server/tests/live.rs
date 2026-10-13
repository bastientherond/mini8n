//! Collaboration en direct (WebSocket) de bout en bout. Nécessite PostgreSQL :
//! `TEST_DATABASE_URL=postgres://… cargo test -p mini8n-server` (ignoré sinon).

use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use mini8n_server::config::CookieSecure;
use mini8n_server::{AppState, Config, MIGRATOR, app};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:8080";

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Server {
    app: axum::Router,
    addr: SocketAddr,
}

async fn setup() -> Option<Server> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db = PgPool::connect(&url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
        public_origins: vec![ORIGIN.into()],
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
    };
    let app = app(AppState::new(db, config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap()
    });
    Some(Server { app, addr })
}

async fn api(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Value,
    cookie: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

async fn register(app: &axum::Router, name: &str) -> (String, String) {
    let email = format!("live-{}@exemple.test", uuid::Uuid::new_v4());
    let (status, _, cookie) = api(
        app,
        "POST",
        "/api/auth/register",
        json!({"email": email, "name": name, "password": "un mot de passe solide"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    (cookie.unwrap(), email)
}

async fn open(
    server: &Server,
    path: &str,
    cookie: &str,
    origin: &str,
) -> Result<Socket, StatusCode> {
    let mut req = format!("ws://{}{path}", server.addr)
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert(header::COOKIE, cookie.parse().unwrap());
    req.headers_mut()
        .insert(header::ORIGIN, origin.parse().unwrap());
    match tokio_tungstenite::connect_async(req).await {
        Ok((socket, _)) => Ok(socket),
        Err(tokio_tungstenite::tungstenite::Error::Http(res)) => Err(res.status()),
        Err(e) => panic!("{e}"),
    }
}

/// Prochain message JSON (les pings sont ignorés).
async fn next(socket: &mut Socket) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("message attendu")
            .unwrap()
            .unwrap();
        match msg {
            Message::Text(t) => return serde_json::from_str(t.as_str()).unwrap(),
            Message::Close(frame) => return json!({ "closed": frame.map(|f| u16::from(f.code)) }),
            _ => {}
        }
    }
}

#[tokio::test]
async fn collaborators_see_each_other() {
    let Some(server) = setup().await else { return };
    let app = &server.app;
    let (alice, _) = register(app, "Alice").await;
    let (bob, bob_email) = register(app, "Bob").await;
    let (_, ws, _) = api(
        app,
        "POST",
        "/api/workspaces",
        json!({"name": "Équipe"}),
        Some(&alice),
    )
    .await;
    let ws = ws["id"].as_str().unwrap().to_string();
    // Bob rejoint le workspace par invitation (adresse non vérifiée, pas de SMTP : lien direct).
    let (status, added, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/members"),
        json!({"email": bob_email, "role": "member"}),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let token = added["invitation"]["link"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let (status, _, _) = api(
        app,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        Value::Null,
        Some(&bob),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let def = json!({"nodes": [{"id": "n1", "position": {"x": 0, "y": 0}, "data": {"kind": "csv"}}], "edges": []});
    let (_, wf, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/workflows"),
        json!({"name": "Ventes", "definition": def}),
        Some(&alice),
    )
    .await;
    let wf = wf["id"].as_str().unwrap().to_string();
    let live = format!("/api/workspaces/{ws}/workflows/{wf}/live");

    // Un autre site ne peut pas ouvrir la connexion avec le cookie du visiteur.
    assert_eq!(
        open(&server, &live, &alice, "https://malveillant.exemple")
            .await
            .err(),
        Some(StatusCode::FORBIDDEN)
    );
    // Un non-membre ne voit pas le workflow.
    let (eve, _) = register(app, "Ève").await;
    assert_eq!(
        open(&server, &live, &eve, ORIGIN).await.err(),
        Some(StatusCode::NOT_FOUND)
    );

    let mut a = open(&server, &live, &alice, ORIGIN).await.unwrap();
    let welcome = next(&mut a).await;
    assert_eq!(welcome["t"], "welcome");
    assert_eq!(welcome["peers"], json!([]));
    a.send(Message::text(r#"{"t":"select","nodes":["n1"]}"#))
        .await
        .unwrap();

    let mut b = open(&server, &live, &bob, ORIGIN).await.unwrap();
    let welcome = next(&mut b).await;
    assert_eq!(welcome["peers"][0]["name"], "Alice");
    assert_eq!(welcome["peers"][0]["selection"], json!(["n1"]));
    let joined = next(&mut a).await;
    assert_eq!(joined["t"], "join");
    assert_eq!(joined["peer"]["name"], "Bob");

    b.send(Message::text(r#"{"t":"cursor","at":{"x":120,"y":-40}}"#))
        .await
        .unwrap();
    let cursor = next(&mut a).await;
    assert_eq!(cursor["t"], "cursor");
    assert_eq!(cursor["id"], welcome["you"]);
    assert_eq!(cursor["at"], json!({"x": 120.0, "y": -40.0}));

    // Une sauvegarde est diffusée à tous les clients du workflow.
    let (status, _, _) = api(
        app,
        "PUT",
        &format!("/api/workspaces/{ws}/workflows/{wf}"),
        json!({"name": "Ventes 2", "definition": def, "version": 1}),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for socket in [&mut a, &mut b] {
        let saved = next(socket).await;
        assert_eq!(saved["t"], "saved");
        assert_eq!(saved["doc"]["version"], 2);
        assert_eq!(saved["doc"]["name"], "Ventes 2");
    }

    // Bob retiré du workspace : sa connexion est fermée, Alice est prévenue.
    let (_, me, _) = api(app, "GET", "/api/auth/me", json!(null), Some(&bob)).await;
    let bob_id = me["user"]["id"].as_str().unwrap().to_string();
    let (status, _, _) = api(
        app,
        "DELETE",
        &format!("/api/workspaces/{ws}/members/{bob_id}"),
        json!(null),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(next(&mut b).await["closed"], 4403);
    assert_eq!(next(&mut a).await["t"], "leave");
}
