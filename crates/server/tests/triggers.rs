//! Tests d'intégration des déclencheurs (PostgreSQL requis : `TEST_DATABASE_URL`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use mini8n_server::config::CookieSecure;
use mini8n_server::{AppState, Config, MIGRATOR, app, scheduler};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:8080";

struct Env {
    app: Router,
    state: AppState,
    cookie: String,
    ws: String,
}

async fn setup(http_allow_private: bool) -> Option<Env> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db = PgPool::connect(&url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
        public_origins: vec![ORIGIN.into()],
        cookie_secure: CookieSecure::Always,
        trust_proxy: false,
        session_ttl_hours: 1,
        allow_registration: true,
        http_allow_private,
        runner_threads: 2,
        execution_timeout_secs: 30,
        executions_retention_days: 30,
        network_allowlist: vec![],
        credentials_key: [42u8; 32],
        connectors_url: None,
        connectors_token: None,
    };
    let state = AppState::new(db, config);
    let app = app(state.clone());
    let email = format!("trig-{}@exemple.test", uuid::Uuid::new_v4());
    let r = call(
        &app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email, "name": "T", "password": "un mot de passe solide"})),
        None,
        true,
    )
    .await;
    let cookie = r.cookie.expect("cookie");
    let ws = call(
        &app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Déclencheurs"})),
        Some(&cookie),
        true,
    )
    .await;
    let ws = ws.body["id"].as_str().unwrap().to_string();
    Some(Env {
        app,
        state,
        cookie,
        ws,
    })
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
    origin: bool,
) -> Res {
    call_with(app, method, path, body, cookie, origin, &[]).await
}

async fn call_with(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    origin: bool,
    headers: &[(&str, &str)],
) -> Res {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    if origin {
        req = req.header(header::ORIGIN, ORIGIN);
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
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
    Res {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    }
}

fn token() -> String {
    format!("tok_{}", uuid::Uuid::new_v4().simple())
}

fn node(id: &str, kind: &str, params: Value) -> Value {
    json!({ "id": id, "position": {"x": 0, "y": 0}, "data": { "kind": kind, "name": id, "params": params } })
}

fn edge(source: &str, target: &str) -> Value {
    json!({ "id": format!("e-{source}-{target}"), "source": source, "target": target, "targetHandle": "in-0" })
}

impl Env {
    async fn api(&self, method: &str, path: &str, body: Option<Value>) -> Res {
        call(&self.app, method, path, body, Some(&self.cookie), true).await
    }

    async fn workflow(&self, name: &str, nodes: Value, edges: Value) -> String {
        let r = self
            .api("POST", &format!("/api/workspaces/{}/workflows", self.ws), Some(json!({"name": name, "definition": {"version": 1, "nodes": nodes, "edges": edges}})))
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_string()
    }

    async fn activate(&self, wf: &str, active: bool) -> Res {
        self.api(
            "PUT",
            &format!("/api/workspaces/{}/workflows/{wf}/active", self.ws),
            Some(json!({"active": active})),
        )
        .await
    }

    /// Attend une exécution terminée correspondant au filtre.
    async fn wait_execution(&self, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let r = self
                .api(
                    "GET",
                    &format!("/api/workspaces/{}/executions", self.ws),
                    None,
                )
                .await;
            if let Some(e) = r
                .body
                .as_array()
                .and_then(|a| a.iter().find(|e| e["status"] != "running" && pred(e)))
            {
                return e.clone();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("exécution attendue introuvable");
    }

    async fn executions(&self) -> Vec<Value> {
        self.api(
            "GET",
            &format!("/api/workspaces/{}/executions", self.ws),
            None,
        )
        .await
        .body
        .as_array()
        .cloned()
        .unwrap_or_default()
    }
}

#[tokio::test]
async fn webhook_end_to_end() {
    let Some(env) = setup(false).await else {
        return;
    };
    let t = token();
    let wf = env
        .workflow(
            "Webhook",
            json!([
                node("hook", "trigger_webhook", json!({"token": t, "method": "POST", "secret": "s3cret", "respond": "résultat du workflow"})),
                node("calc", "formula", json!({"column": "total", "expression": "montant * 2"}))
            ]),
            json!([edge("hook", "calc")]),
        )
        .await;

    // Inactif : URL inconnue.
    let hook = format!("/api/hooks/{t}");
    assert_eq!(
        call(
            &env.app,
            "POST",
            &hook,
            Some(json!({"montant": 1})),
            None,
            false
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    let r = env.activate(&wf, true).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["active"], true);
    assert_eq!(r.body["triggers"][0]["token"], t.as_str());

    // Secret absent ou faux : refus.
    assert_eq!(
        call(
            &env.app,
            "POST",
            &hook,
            Some(json!({"montant": 1})),
            None,
            false
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let bad = call_with(
        &env.app,
        "POST",
        &hook,
        Some(json!({"montant": 1})),
        None,
        false,
        &[("x-mini8n-secret", "faux")],
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    // Mauvaise méthode.
    let get = call_with(
        &env.app,
        "GET",
        &hook,
        None,
        None,
        false,
        &[("x-mini8n-secret", "s3cret")],
    )
    .await;
    assert_eq!(get.status, StatusCode::METHOD_NOT_ALLOWED);

    // Appel valide (sans en-tête Origin, comme un service tiers) : réponse = sortie du workflow.
    let ok = call_with(
        &env.app,
        "POST",
        &hook,
        Some(json!({"montant": 21})),
        None,
        false,
        &[("x-mini8n-secret", "s3cret")],
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body[0]["total"], 42);
    assert_eq!(ok.body[0]["_methode"], "POST");

    let exec = env.wait_execution(|e| e["triggerKind"] == "webhook").await;
    assert_eq!(exec["status"], "success");
    let detail = env
        .api(
            "GET",
            &format!(
                "/api/workspaces/{}/executions/{}",
                env.ws,
                exec["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(detail.body["output"]["rows"][0]["total"], 42);

    // Désactivé : l'URL ne répond plus.
    env.activate(&wf, false).await;
    let after = call_with(
        &env.app,
        "POST",
        &hook,
        Some(json!({"montant": 1})),
        None,
        false,
        &[("x-mini8n-secret", "s3cret")],
    )
    .await;
    assert_eq!(after.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn activation_is_validated_and_follows_saves() {
    let Some(env) = setup(false).await else {
        return;
    };
    let manual = env
        .workflow(
            "Manuel",
            json!([node("m", "trigger_manual", json!({}))]),
            json!([]),
        )
        .await;
    let r = env.activate(&manual, true).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(
        r.body["error"]
            .as_str()
            .unwrap()
            .contains("déclencheur automatique")
    );

    let t = token();
    let wf = env
        .workflow(
            "Hook",
            json!([node("hook", "trigger_webhook", json!({"token": t}))]),
            json!([]),
        )
        .await;
    assert_eq!(env.activate(&wf, true).await.status, StatusCode::OK);
    // Un autre workflow ne peut pas réutiliser le même jeton.
    let dup = env
        .workflow(
            "Copie",
            json!([node("hook", "trigger_webhook", json!({"token": t}))]),
            json!([]),
        )
        .await;
    assert_eq!(env.activate(&dup, true).await.status, StatusCode::CONFLICT);

    // Sauvegarde d'une version invalide d'un workflow actif : désactivation avec motif.
    let path = format!("/api/workspaces/{}/workflows/{wf}", env.ws);
    let doc = env.api("GET", &path, None).await;
    let saved = env
        .api("PUT", &path, Some(json!({"name": "Hook", "version": doc.body["version"], "definition": {"version": 1, "nodes": [node("hook", "trigger_webhook", json!({"token": "court"}))], "edges": []}})))
        .await;
    assert_eq!(saved.status, StatusCode::OK);
    assert_eq!(saved.body["active"], false);
    assert!(
        saved.body["deactivated"]
            .as_str()
            .unwrap()
            .contains("régénérez")
    );
}

#[tokio::test]
async fn forms_are_public_but_same_origin() {
    let Some(env) = setup(false).await else {
        return;
    };
    let t = token();
    let wf = env
        .workflow(
            "Contact",
            json!([node("form", "trigger_form", json!({"token": t, "title": "Contact", "fields": "Nom | texte | requis\nÂge | nombre", "confirmation": "Merci !"}))]),
            json!([]),
        )
        .await;
    env.activate(&wf, true).await;
    let path = format!("/api/forms/{t}");
    let def = call(&env.app, "GET", &path, None, None, false).await;
    assert_eq!(def.status, StatusCode::OK);
    assert_eq!(def.body["fields"][1]["key"], "age");

    let values = json!({"values": {"nom": "Ada", "age": "36"}});
    assert_eq!(
        call(&env.app, "POST", &path, Some(values.clone()), None, false)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let invalid = call(
        &env.app,
        "POST",
        &path,
        Some(json!({"values": {"age": "x"}})),
        None,
        true,
    )
    .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    let ok = call(&env.app, "POST", &path, Some(values), None, true).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["message"], "Merci !");
    let exec = env.wait_execution(|e| e["triggerKind"] == "form").await;
    assert_eq!(exec["status"], "success");
}

#[tokio::test]
async fn subworkflows_and_error_triggers() {
    let Some(env) = setup(false).await else {
        return;
    };
    let child = env
        .workflow(
            "Enfant",
            json!([
                node("in", "trigger_workflow", json!({})),
                node(
                    "x2",
                    "formula",
                    json!({"column": "double", "expression": "n * 2"})
                )
            ]),
            json!([edge("in", "x2")]),
        )
        .await;
    let t = token();
    let parent = env
        .workflow(
            "Parent",
            json!([
                node(
                    "hook",
                    "trigger_webhook",
                    json!({"token": t, "respond": "résultat du workflow"})
                ),
                node("call", "subworkflow", json!({"workflow": child}))
            ]),
            json!([edge("hook", "call")]),
        )
        .await;
    env.activate(&parent, true).await;
    let r = call(
        &env.app,
        "POST",
        &format!("/api/hooks/{t}"),
        Some(json!({"n": 5})),
        None,
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body[0]["double"], 10);
    let sub = env.wait_execution(|e| e["triggerKind"] == "workflow").await;
    assert!(sub["parentId"].is_string());

    // Échec d'un workflow → le workflow « Sur erreur » est lancé.
    let watcher = env
        .workflow(
            "Alerte",
            json!([
                node("err", "trigger_error", json!({})),
                node("lim", "limit", json!({"count": 1}))
            ]),
            json!([edge("err", "lim")]),
        )
        .await;
    env.activate(&watcher, true).await;
    let t2 = token();
    let failing = env
        .workflow(
            "Fragile",
            json!([
                node("hook", "trigger_webhook", json!({"token": t2})),
                node("filtre", "filter", json!({"condition": "absente > 1"}))
            ]),
            json!([edge("hook", "filtre")]),
        )
        .await;
    env.activate(&failing, true).await;
    let r = call(
        &env.app,
        "POST",
        &format!("/api/hooks/{t2}"),
        Some(json!({"a": 1})),
        None,
        false,
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let failed = env.wait_execution(|e| e["workflowName"] == "Fragile").await;
    assert_eq!(failed["status"], "error");
    assert!(failed["error"].as_str().unwrap().contains("filtre"));
    let alert = env.wait_execution(|e| e["triggerKind"] == "error").await;
    assert_eq!(alert["status"], "success");
    assert_eq!(alert["workflowName"], "Alerte");
}

#[tokio::test]
async fn run_now_on_server() {
    let Some(env) = setup(false).await else {
        return;
    };
    let wf = env
        .workflow(
            "Manuel",
            json!([
                node("m", "trigger_manual", json!({})),
                node(
                    "calc",
                    "formula",
                    json!({"column": "ok", "expression": "true"})
                )
            ]),
            json!([edge("m", "calc")]),
        )
        .await;
    let r = env
        .api(
            "POST",
            &format!("/api/workspaces/{}/workflows/{wf}/run", env.ws),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["status"], "success");
}

/// Petit serveur HTTP local servant une liste d'éléments modifiable.
async fn items_server(items: Arc<Mutex<Vec<Value>>>) -> String {
    let router = Router::new().route(
        "/items",
        axum::routing::get(move || {
            let items = items.clone();
            async move { axum::Json(Value::Array(items.lock().unwrap().clone())) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}/items")
}

async fn make_due(env: &Env, wf: &str) {
    sqlx::query("UPDATE triggers SET next_run_at = now() - interval '1 second' WHERE workflow_id = $1::uuid")
        .bind(wf)
        .execute(&env.state.db)
        .await
        .unwrap();
}

async fn tick(env: &Env) {
    let ws: uuid::Uuid = env.ws.parse().unwrap();
    scheduler::run_due(&env.state.runner, Some(ws))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
}

#[tokio::test]
async fn poll_and_schedule() {
    let Some(env) = setup(true).await else { return };
    let items = Arc::new(Mutex::new(vec![
        json!({"id": 1, "titre": "a"}),
        json!({"id": 2, "titre": "b"}),
    ]));
    let url = items_server(items.clone()).await;
    let wf = env
        .workflow(
            "Surveillance",
            json!([node(
                "poll",
                "trigger_poll",
                json!({"url": url, "key": "id", "every": 1})
            )]),
            json!([]),
        )
        .await;
    assert_eq!(env.activate(&wf, true).await.status, StatusCode::OK);

    // Premier passage : mémorisation sans déclenchement.
    tick(&env).await;
    assert!(
        env.executions()
            .await
            .iter()
            .all(|e| e["triggerKind"] != "poll")
    );
    // Nouvel élément : une exécution avec cet élément seulement.
    items
        .lock()
        .unwrap()
        .insert(0, json!({"id": 3, "titre": "nouveau"}));
    make_due(&env, &wf).await;
    tick(&env).await;
    let exec = env.wait_execution(|e| e["triggerKind"] == "poll").await;
    assert_eq!(exec["status"], "success");
    let detail = env
        .api(
            "GET",
            &format!(
                "/api/workspaces/{}/executions/{}",
                env.ws,
                exec["id"].as_str().unwrap()
            ),
            None,
        )
        .await;
    assert_eq!(detail.body["output"]["total"], 1);
    assert_eq!(detail.body["output"]["rows"][0]["id"], 3);

    // Planification.
    let sched = env
        .workflow(
            "Planifié",
            json!([node(
                "cron",
                "trigger_schedule",
                json!({"mode": "intervalle", "every": 60})
            )]),
            json!([]),
        )
        .await;
    env.activate(&sched, true).await;
    make_due(&env, &sched).await;
    tick(&env).await;
    let exec = env.wait_execution(|e| e["triggerKind"] == "schedule").await;
    assert_eq!(exec["status"], "success");
    let info = env
        .api(
            "GET",
            &format!("/api/workspaces/{}/workflows/{sched}/triggers", env.ws),
            None,
        )
        .await;
    assert!(info.body["triggers"][0]["nextRunAt"].is_string());
}

#[tokio::test]
async fn server_side_http_blocks_private_networks() {
    let Some(env) = setup(false).await else {
        return;
    };
    let items = Arc::new(Mutex::new(vec![json!({"id": 1})]));
    let url = items_server(items).await; // 127.0.0.1 : interdit par défaut
    let wf = env
        .workflow(
            "SSRF",
            json!([node(
                "poll",
                "trigger_poll",
                json!({"url": url, "key": "id", "every": 1})
            )]),
            json!([]),
        )
        .await;
    env.activate(&wf, true).await;
    tick(&env).await;
    let info = env
        .api(
            "GET",
            &format!("/api/workspaces/{}/workflows/{wf}/triggers", env.ws),
            None,
        )
        .await;
    let err = info.body["triggers"][0]["lastError"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(err.contains("non autorisée"), "{err}");
}
