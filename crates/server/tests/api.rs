//! Tests d'intégration de l'API. Nécessitent PostgreSQL :
//! `TEST_DATABASE_URL=postgres://… cargo test -p mini8n-server` (ignorés sinon).

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use mini8n_server::config::CookieSecure;
use mini8n_server::{AppState, Config, MIGRATOR, app};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:8080";

async fn setup() -> Option<Router> {
    setup_with(CookieSecure::Always, false).await
}

async fn setup_with(cookie_secure: CookieSecure, trust_proxy: bool) -> Option<Router> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db = PgPool::connect(&url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
        database_max_connections: 10,
        public_origins: vec![ORIGIN.into()],
        cookie_secure,
        trust_proxy,
        session_ttl_hours: 1,
        allow_registration: true,
        mail: None,
        http_allow_private: false,
        runner_threads: 2,
        execution_timeout_secs: 30,
        executions_retention_days: 30,
        network_allowlist: vec![],
        localhost_alias: None,
        credentials_key: [42u8; 32],
        connectors_url: None,
        connectors_token: None,
        metrics_token: None,
    };
    Some(app(AppState::new(db, config)))
}

struct Res {
    status: StatusCode,
    body: Value,
    cookie: Option<String>,
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Res {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    if let Some(o) = origin {
        req = req.header(header::ORIGIN, o);
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Res {
        status,
        body,
        cookie,
    }
}

/// Requête « navigateur » : même origine.
async fn api(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> Res {
    call(app, method, path, body, cookie, Some(ORIGIN)).await
}

fn email() -> String {
    format!("user-{}@exemple.test", uuid::Uuid::new_v4())
}

async fn register(app: &Router, email: &str) -> String {
    let r = api(
        app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email, "name": "Test", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.cookie.expect("cookie de session")
}

#[tokio::test]
async fn register_workspace_workflow_flow() {
    let Some(app) = setup().await else { return };
    let mail = email();
    let r = api(
        &app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": mail, "name": "Alice", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(r.body["workspaces"], json!([]));
    let cookie = r.cookie.unwrap();
    assert!(cookie.starts_with("__Host-mini8n_session="));

    let me = api(&app, "GET", "/api/auth/me", None, Some(&cookie)).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["user"]["name"], "Alice");

    let ws = api(
        &app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Équipe data"})),
        Some(&cookie),
    )
    .await;
    assert_eq!(ws.status, StatusCode::CREATED);
    assert_eq!(ws.body["role"], "owner");
    let ws_id = ws.body["id"].as_str().unwrap().to_string();

    let def = json!({"nodes": [{"id": "n1", "position": {"x": 0, "y": 0}, "data": {"kind": "csv"}}], "edges": []});
    let wf = api(
        &app,
        "POST",
        &format!("/api/workspaces/{ws_id}/workflows"),
        Some(json!({"name": "Ventes", "definition": def})),
        Some(&cookie),
    )
    .await;
    assert_eq!(wf.status, StatusCode::CREATED);
    let wf_id = wf.body["id"].as_str().unwrap().to_string();
    let path = format!("/api/workspaces/{ws_id}/workflows/{wf_id}");

    let up = api(
        &app,
        "PUT",
        &path,
        Some(json!({"name": "Ventes 2", "definition": def, "version": 1})),
        Some(&cookie),
    )
    .await;
    assert_eq!(up.status, StatusCode::OK);
    assert_eq!(up.body["version"], 2);
    // Version périmée : conflit.
    let stale = api(
        &app,
        "PUT",
        &path,
        Some(json!({"name": "x", "definition": def, "version": 1})),
        Some(&cookie),
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);

    let list = api(
        &app,
        "GET",
        &format!("/api/workspaces/{ws_id}/workflows"),
        None,
        Some(&cookie),
    )
    .await;
    assert_eq!(list.body.as_array().unwrap().len(), 1);

    let out = api(&app, "POST", "/api/auth/logout", None, Some(&cookie)).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&cookie))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn auth_protections() {
    let Some(app) = setup().await else { return };
    let mail = email();
    register(&app, &mail).await;

    // Mot de passe faible / e-mail invalide.
    let weak = api(
        &app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email(), "name": "x", "password": "court"})),
        None,
    )
    .await;
    assert_eq!(weak.status, StatusCode::BAD_REQUEST);
    // Doublon (casse différente) : refus sans détail.
    let dup = api(&app, "POST", "/api/auth/register", Some(json!({"email": mail.to_uppercase(), "name": "x", "password": "un mot de passe solide"})), None).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    // Champs inattendus refusés.
    let extra = api(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": mail, "password": "x", "admin": true})),
        None,
    )
    .await;
    assert!(extra.status.is_client_error());

    // Même message pour un compte inconnu et un mauvais mot de passe.
    let bad_pw = api(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": mail, "password": "mauvais mot de passe"})),
        None,
    )
    .await;
    let unknown = api(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": email(), "password": "mauvais mot de passe"})),
        None,
    )
    .await;
    assert_eq!(bad_pw.status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad_pw.body, unknown.body);

    // CSRF : origine absente ou étrangère.
    let body = json!({"email": mail, "password": "un mot de passe solide"});
    let no_origin = call(
        &app,
        "POST",
        "/api/auth/login",
        Some(body.clone()),
        None,
        None,
    )
    .await;
    assert_eq!(no_origin.status, StatusCode::FORBIDDEN);
    let evil = call(
        &app,
        "POST",
        "/api/auth/login",
        Some(body.clone()),
        None,
        Some("https://evil.example"),
    )
    .await;
    assert_eq!(evil.status, StatusCode::FORBIDDEN);

    // Jeton forgé.
    let forged = format!("__Host-mini8n_session={}", "a".repeat(64));
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&forged))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    let ok = api(&app, "POST", "/api/auth/login", Some(body), None).await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
async fn account_lockout() {
    let Some(app) = setup().await else { return };
    let mail = email();
    register(&app, &mail).await;
    for _ in 0..5 {
        let r = api(
            &app,
            "POST",
            "/api/auth/login",
            Some(json!({"email": mail, "password": "mauvais mot de passe"})),
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // Même avec le bon mot de passe, le compte est temporairement bloqué.
    let r = api(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": mail, "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn workspace_isolation_and_members() {
    let Some(app) = setup().await else { return };
    let alice = register(&app, &email()).await;
    let bob_mail = email();
    let bob = register(&app, &bob_mail).await;

    let ws = api(
        &app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Privé"})),
        Some(&alice),
    )
    .await;
    let ws_id = ws.body["id"].as_str().unwrap().to_string();

    // Bob ne voit pas le workspace d'Alice (404, pas 403).
    assert_eq!(
        api(
            &app,
            "GET",
            &format!("/api/workspaces/{ws_id}/workflows"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/workspaces/{ws_id}"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // Alice ajoute Bob comme membre.
    let add = api(
        &app,
        "POST",
        &format!("/api/workspaces/{ws_id}/members"),
        Some(json!({"email": bob_mail, "role": "member"})),
        Some(&alice),
    )
    .await;
    // Adresse de Bob non vérifiée : invitation (sans SMTP, lien à transmettre), pas d'accès direct.
    assert_eq!(add.status, StatusCode::ACCEPTED, "{:?}", add.body);
    let link = add.body["invitation"]["link"].as_str().unwrap().to_string();
    let token = link.rsplit('/').next().unwrap();
    assert_eq!(
        api(
            &app,
            "GET",
            &format!("/api/workspaces/{ws_id}/workflows"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Seul le compte invité peut accepter.
    let eve = register(&app, &email()).await;
    assert_eq!(
        api(
            &app,
            "POST",
            &format!("/api/auth/invitations/{token}/accept"),
            None,
            Some(&eve)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let accepted = api(
        &app,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        None,
        Some(&bob),
    )
    .await;
    assert_eq!(accepted.status, StatusCode::OK, "{:?}", accepted.body);
    // Usage unique.
    assert_eq!(
        api(
            &app,
            "POST",
            &format!("/api/auth/invitations/{token}/accept"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api(
            &app,
            "GET",
            &format!("/api/workspaces/{ws_id}/workflows"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::OK
    );
    // …mais un membre ne gère ni le workspace ni ses membres.
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/workspaces/{ws_id}"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let me = api(&app, "GET", "/api/auth/me", None, Some(&alice)).await;
    let alice_id = me.body["user"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/workspaces/{ws_id}/members/{alice_id}"),
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // Le dernier propriétaire ne peut pas partir.
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/workspaces/{ws_id}/members/{alice_id}"),
            None,
            Some(&alice)
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn password_change_revokes_other_sessions() {
    let Some(app) = setup().await else { return };
    let mail = email();
    let first = register(&app, &mail).await;
    let second = api(
        &app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": mail, "password": "un mot de passe solide"})),
        None,
    )
    .await
    .cookie
    .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/auth/password",
        Some(json!({"currentPassword": "un mot de passe solide", "newPassword": "un tout nouveau secret"})),
        Some(&second),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&first))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&second))
            .await
            .status,
        StatusCode::OK
    );
}

/// Mode `auto` : cookie `Secure` + `__Host-` seulement en HTTPS (Safari refuse `Secure` en HTTP).
#[tokio::test]
async fn cookie_policy_follows_forwarded_proto() {
    let Some(app) = setup_with(CookieSecure::Auto, true).await else {
        return;
    };
    let register = |proto: Option<&'static str>| {
        let app = app.clone();
        async move {
            let mut req = Request::builder()
                .method("POST")
                .uri("/api/auth/register")
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(p) = proto {
                req = req.header("x-forwarded-proto", p);
            }
            let body =
                json!({"email": email(), "name": "Test", "password": "un mot de passe solide"});
            let res = app
                .oneshot(req.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::CREATED);
            res.headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        }
    };

    let plain = register(None).await;
    assert!(plain.starts_with("mini8n_session="), "{plain}");
    assert!(!plain.contains("Secure"), "{plain}");
    assert!(
        plain.contains("HttpOnly") && plain.contains("SameSite=Strict"),
        "{plain}"
    );

    let secure = register(Some("https")).await;
    assert!(secure.starts_with("__Host-mini8n_session="), "{secure}");
    assert!(secure.contains("; Secure"), "{secure}");

    // Le cookie non préfixé est bien accepté en retour.
    let cookie = plain.split(';').next().unwrap();
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(cookie))
            .await
            .status,
        StatusCode::OK
    );
}

/// Un ancien cookie `__Host-` périmé ne doit pas masquer un cookie de session valide.
#[tokio::test]
async fn stale_secure_cookie_does_not_hide_valid_session() {
    let Some(app) = setup_with(CookieSecure::Auto, true).await else {
        return;
    };
    let r = api(
        &app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email(), "name": "T", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    let plain = r.cookie.unwrap();
    assert!(plain.starts_with("mini8n_session="));
    let both = format!("__Host-mini8n_session={}; {plain}", "b".repeat(64));
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&both))
            .await
            .status,
        StatusCode::OK
    );
    // La déconnexion efface les deux variantes du cookie.
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, &both)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let cleared: Vec<_> = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(cleared.len(), 2);
    assert!(cleared.iter().all(|c| c.contains("Max-Age=0")));
    assert_eq!(
        api(&app, "GET", "/api/auth/me", None, Some(&both))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

/// Données personnelles : conservation désactivable par workflow, historique effaçable.
#[tokio::test]
async fn execution_data_retention() {
    let Some(app) = setup().await else { return };
    let owner = register(&app, &email()).await;
    let ws = api(
        &app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "RGPD"})),
        Some(&owner),
    )
    .await;
    let ws = ws.body["id"].as_str().unwrap().to_string();
    let def = json!({"nodes": [{"id": "src", "position": {"x": 0, "y": 0},
        "data": {"kind": "manual", "name": "Clients", "params": {"data": "[{\"email\": \"a@exemple.test\"}]"}}}], "edges": []});
    let wf = api(
        &app,
        "POST",
        &format!("/api/workspaces/{ws}/workflows"),
        Some(json!({"name": "Clients", "definition": def})),
        Some(&owner),
    )
    .await;
    let wf = wf.body["id"].as_str().unwrap().to_string();
    assert!(wf_keep_data(&app, &ws, &wf, &owner).await);

    let output = |exec: String| {
        let (app, ws, owner) = (app.clone(), ws.clone(), owner.clone());
        async move {
            api(
                &app,
                "GET",
                &format!("/api/workspaces/{ws}/executions/{exec}"),
                None,
                Some(&owner),
            )
            .await
            .body["output"]
                .clone()
        }
    };
    let run = || {
        let (app, ws, wf, owner) = (app.clone(), ws.clone(), wf.clone(), owner.clone());
        async move {
            let r = api(
                &app,
                "POST",
                &format!("/api/workspaces/{ws}/workflows/{wf}/run"),
                None,
                Some(&owner),
            )
            .await;
            assert_eq!(r.body["status"], "success", "{:?}", r.body);
            r.body["executionId"].as_str().unwrap().to_string()
        }
    };
    let first = run().await;
    assert!(
        output(first.clone())
            .await
            .to_string()
            .contains("a@exemple.test")
    );

    // Désactivation : l'aperçu déjà enregistré est effacé, les suivants ne sont pas gardés.
    let off = api(
        &app,
        "PUT",
        &format!("/api/workspaces/{ws}/workflows/{wf}/data"),
        Some(json!({"keepData": false})),
        Some(&owner),
    )
    .await;
    assert_eq!(off.status, StatusCode::OK, "{:?}", off.body);
    assert_eq!(off.body["keepData"], false);
    assert_eq!(output(first).await, Value::Null);
    let second = run().await;
    let detail = api(
        &app,
        "GET",
        &format!("/api/workspaces/{ws}/executions/{second}"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(detail.body["output"], Value::Null);
    assert_eq!(detail.body["status"], "success", "le statut reste visible");
    assert_eq!(
        detail.body["report"]["results"]["src"]["rows"], 1,
        "les comptes de lignes aussi"
    );

    // Effacement de l'historique : réservé aux propriétaires.
    let member_mail = email();
    let member = register(&app, &member_mail).await;
    let added = api(
        &app,
        "POST",
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({"email": member_mail, "role": "member"})),
        Some(&owner),
    )
    .await;
    let token = added.body["invitation"]["link"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    api(
        &app,
        "POST",
        &format!("/api/auth/invitations/{token}/accept"),
        None,
        Some(&member),
    )
    .await;
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/workspaces/{ws}/workflows/{wf}/executions"),
            None,
            Some(&member)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let cleared = api(
        &app,
        "DELETE",
        &format!("/api/workspaces/{ws}/workflows/{wf}/executions"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(cleared.body["deleted"], 2);
    let left = api(
        &app,
        "GET",
        &format!("/api/workspaces/{ws}/executions?workflow={wf}"),
        None,
        Some(&owner),
    )
    .await;
    assert_eq!(left.body, json!([]));
}

async fn wf_keep_data(app: &Router, ws: &str, wf: &str, cookie: &str) -> bool {
    let doc = api(
        app,
        "GET",
        &format!("/api/workspaces/{ws}/workflows/{wf}"),
        None,
        Some(cookie),
    )
    .await;
    doc.body["keepData"].as_bool().unwrap()
}
