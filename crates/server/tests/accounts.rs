//! Parcours de compte (premier compte, inscriptions fermées, mot de passe oublié,
//! vérification d'adresse, invitations) de bout en bout, avec une boîte e-mail en mémoire.
//! Nécessite PostgreSQL : `TEST_DATABASE_URL=postgres://… cargo test -p mini8n-server`.
//! Base dédiée et temporaire : le « premier compte » suppose une base vide.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use mini8n_server::config::CookieSecure;
use mini8n_server::mailer::{Mailer, Sent};
use mini8n_server::{AppState, Config, MIGRATOR, app};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:8080";

struct Env {
    app: Router,
    outbox: Arc<Mutex<Vec<Sent>>>,
    admin: PgPool,
    db_name: String,
}

/// Application sur une base neuve, inscriptions fermées, e-mails en mémoire.
async fn setup() -> Option<Env> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let admin = PgPool::connect(&url).await.expect("connexion");
    let db_name = format!("mini8n_accounts_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {db_name}"))
        .execute(&admin)
        .await
        .expect("création de la base");
    let (base, _) = url.rsplit_once('/').expect("URL de base");
    let db_url = format!("{base}/{db_name}");
    let db = PgPool::connect(&db_url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: db_url,
        database_max_connections: 10,
        public_origins: vec![ORIGIN.into()],
        cookie_secure: CookieSecure::Never,
        trust_proxy: false,
        session_ttl_hours: 1,
        allow_registration: false,
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
        metrics_token: None,
    };
    let mut state = AppState::new(db, config);
    let (mailer, outbox) = Mailer::memory();
    state.mailer = Some(Arc::new(mailer));
    Some(Env {
        app: app(state),
        outbox,
        admin,
        db_name,
    })
}

impl Env {
    async fn drop_db(self) {
        drop(self.app);
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.db_name
        ))
        .execute(&self.admin)
        .await
        .ok();
    }

    /// Dernier e-mail reçu par `to` (attend l'envoi en tâche de fond).
    async fn mail_to(&self, to: &str) -> Sent {
        for _ in 0..100 {
            if let Some(m) = self
                .outbox
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|m| m.to == to)
            {
                return m.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("aucun e-mail pour {to}");
    }

    fn mails_to(&self, to: &str) -> usize {
        self.outbox
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.to == to)
            .count()
    }
}

/// Jeton contenu dans le lien d'un e-mail (`…/#/<chemin>/<jeton>`).
fn token_in(mail: &Sent, path: &str) -> String {
    let start = mail
        .body
        .find(&format!("/#/{path}/"))
        .expect("lien dans l'e-mail")
        + path.len()
        + 4;
    mail.body[start..start + 64].to_string()
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

async fn register(app: &Router, email: &str) -> Res {
    api(
        app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email, "name": "Test", "password": "un mot de passe solide"})),
        None,
    )
    .await
}

#[tokio::test]
async fn first_account_then_invitations_only() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    let cfg = api(app, "GET", "/api/auth/config", None, None).await;
    assert_eq!(
        cfg.body,
        json!({"allowRegistration": true, "firstAccount": true, "passwordReset": true})
    );

    // Premier compte : l'administrateur, toujours possible.
    let admin = register(app, "admin@exemple.test").await;
    assert_eq!(admin.status, StatusCode::CREATED, "{:?}", admin.body);
    let admin = admin.cookie.unwrap();
    // Ensuite : inscriptions fermées.
    let cfg = api(app, "GET", "/api/auth/config", None, None).await;
    assert_eq!(cfg.body["allowRegistration"], false);
    assert_eq!(
        register(app, "intrus@exemple.test").await.status,
        StatusCode::FORBIDDEN
    );

    let ws = api(
        app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Équipe"})),
        Some(&admin),
    )
    .await;
    let ws = ws.body["id"].as_str().unwrap().to_string();

    // Invitation par e-mail : pas de lien dans la réponse (il n'est envoyé qu'à l'invité).
    let add = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({"email": "Chloe@Exemple.test", "role": "member"})),
        Some(&admin),
    )
    .await;
    assert_eq!(add.status, StatusCode::ACCEPTED, "{:?}", add.body);
    assert_eq!(add.body["invitation"]["delivered"], true);
    assert!(add.body["invitation"].get("link").is_none());
    let pending = api(
        app,
        "GET",
        &format!("/api/workspaces/{ws}/invitations"),
        None,
        Some(&admin),
    )
    .await;
    assert_eq!(pending.body[0]["email"], "chloe@exemple.test");

    let mail = env.mail_to("chloe@exemple.test").await;
    let token = token_in(&mail, "invitation");
    let shown = api(
        app,
        "GET",
        &format!("/api/auth/invitations/{token}"),
        None,
        None,
    )
    .await;
    assert_eq!(shown.body["workspace"], "Équipe");
    assert_eq!(shown.body["accountExists"], false);

    // Mot de passe trop faible refusé, invitation intacte.
    let weak = api(
        app,
        "POST",
        &format!("/api/auth/invitations/{token}/register"),
        Some(json!({"name": "Chloé", "password": "court"})),
        None,
    )
    .await;
    assert_eq!(weak.status, StatusCode::BAD_REQUEST);
    let joined = api(
        app,
        "POST",
        &format!("/api/auth/invitations/{token}/register"),
        Some(json!({"name": "Chloé", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(joined.status, StatusCode::CREATED, "{:?}", joined.body);
    let chloe = joined.cookie.unwrap();
    let me = api(app, "GET", "/api/auth/me", None, Some(&chloe)).await;
    assert_eq!(
        me.body["user"]["emailVerified"], true,
        "invitation reçue par e-mail : adresse prouvée"
    );
    assert_eq!(me.body["workspaces"][0]["id"], ws.as_str());
    let reuse = api(
        app,
        "POST",
        &format!("/api/auth/invitations/{token}/register"),
        Some(json!({"name": "X", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(reuse.status, StatusCode::BAD_REQUEST);

    // Adresse vérifiée : ajout direct dans un autre workspace.
    let ws2 = api(
        app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Projet"})),
        Some(&admin),
    )
    .await;
    let ws2 = ws2.body["id"].as_str().unwrap();
    let direct = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws2}/members"),
        Some(json!({"email": "chloe@exemple.test", "role": "member"})),
        Some(&admin),
    )
    .await;
    assert_eq!(direct.status, StatusCode::CREATED, "{:?}", direct.body);

    // Révocation d'une invitation : le lien ne fonctionne plus.
    api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({"email": "dan@exemple.test", "role": "member"})),
        Some(&admin),
    )
    .await;
    let dan = token_in(&env.mail_to("dan@exemple.test").await, "invitation");
    let pending = api(
        app,
        "GET",
        &format!("/api/workspaces/{ws}/invitations"),
        None,
        Some(&admin),
    )
    .await;
    let id = pending.body[0]["id"].as_str().unwrap();
    // Un membre non propriétaire ne voit ni ne révoque les invitations.
    assert_eq!(
        api(
            app,
            "GET",
            &format!("/api/workspaces/{ws}/invitations"),
            None,
            Some(&chloe)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        api(
            app,
            "DELETE",
            &format!("/api/workspaces/{ws}/invitations/{id}"),
            None,
            Some(&admin)
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api(
            app,
            "GET",
            &format!("/api/auth/invitations/{dan}"),
            None,
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    env.drop_db().await;
}

#[tokio::test]
async fn password_reset_and_email_verification() {
    let Some(env) = setup().await else { return };
    let app = &env.app;
    let first = register(app, "alice@exemple.test").await;
    let alice = first.cookie.unwrap();
    assert_eq!(first.body["user"]["emailVerified"], false);

    // Adresse inconnue : même réponse, aucun e-mail.
    let r = api(
        app,
        "POST",
        "/api/auth/password/forgot",
        Some(json!({"email": "personne@exemple.test"})),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let r = api(
        app,
        "POST",
        "/api/auth/password/forgot",
        Some(json!({"email": "Alice@Exemple.test"})),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let token = token_in(&env.mail_to("alice@exemple.test").await, "reinitialiser");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(env.mails_to("personne@exemple.test"), 0);

    // Faux jeton, mot de passe faible : refusés.
    let fake = "0".repeat(64);
    assert_eq!(
        api(
            app,
            "POST",
            "/api/auth/password/reset",
            Some(json!({"token": fake, "password": "un autre mot de passe"})),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api(
            app,
            "POST",
            "/api/auth/password/reset",
            Some(json!({"token": token, "password": "court"})),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let done = api(
        app,
        "POST",
        "/api/auth/password/reset",
        Some(json!({"token": token, "password": "un autre mot de passe"})),
        None,
    )
    .await;
    assert_eq!(done.status, StatusCode::NO_CONTENT, "{:?}", done.body);
    // Toutes les sessions sont fermées ; le lien ne sert qu'une fois.
    assert_eq!(
        api(app, "GET", "/api/auth/me", None, Some(&alice))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            app,
            "POST",
            "/api/auth/password/reset",
            Some(json!({"token": token, "password": "encore un autre"})),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let old = api(
        app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": "alice@exemple.test", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);
    let login = api(
        app,
        "POST",
        "/api/auth/login",
        Some(json!({"email": "alice@exemple.test", "password": "un autre mot de passe"})),
        None,
    )
    .await;
    assert_eq!(login.status, StatusCode::OK);
    // Le lien reçu prouve l'adresse.
    assert_eq!(login.body["user"]["emailVerified"], true);
    let alice = login.cookie.unwrap();
    assert_eq!(
        api(
            app,
            "POST",
            "/api/auth/email/send-verification",
            None,
            Some(&alice)
        )
        .await
        .status,
        StatusCode::CONFLICT
    );

    // Vérification d'adresse d'un compte invité sans e-mail (lien transmis à la main).
    let ws = api(
        app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "W"})),
        Some(&alice),
    )
    .await;
    let ws = ws.body["id"].as_str().unwrap();
    api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/members"),
        Some(json!({"email": "bob@exemple.test", "role": "member"})),
        Some(&alice),
    )
    .await;
    let invite = token_in(&env.mail_to("bob@exemple.test").await, "invitation");
    let bob = api(
        app,
        "POST",
        &format!("/api/auth/invitations/{invite}/register"),
        Some(json!({"name": "Bob", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    let bob = bob.cookie.unwrap();
    // (adresse déjà vérifiée par l'invitation reçue par e-mail)
    assert_eq!(
        api(
            app,
            "POST",
            "/api/auth/email/send-verification",
            None,
            Some(&bob)
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    env.drop_db().await;
}
