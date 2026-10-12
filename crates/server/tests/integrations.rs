//! Tests d'intégration des nœuds d'intégration (PostgreSQL requis : `TEST_DATABASE_URL` ;
//! MySQL / MariaDB facultatif : `TEST_MYSQL` = `hôte:port:base:utilisateur:mot_de_passe`).

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use http_body_util::BodyExt;
use mini8n_engine::data;
use mini8n_server::config::CookieSecure;
use mini8n_server::{AppState, Config, MIGRATOR, app};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:8080";

struct Env {
    app: Router,
    db: PgPool,
    cookie: String,
    ws: String,
}

fn db_parts(url: &str) -> (String, u16, String, String, String) {
    // postgres://user:password@host:port/database
    let u = url::Url::parse(url).unwrap();
    (
        u.host_str().unwrap().to_string(),
        u.port().unwrap_or(5432),
        u.path().trim_start_matches('/').to_string(),
        u.username().to_string(),
        u.password().unwrap_or_default().to_string(),
    )
}

async fn setup(allow_private: bool) -> Option<Env> {
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
        http_allow_private: allow_private,
        runner_threads: 2,
        execution_timeout_secs: 30,
        executions_retention_days: 30,
        network_allowlist: vec![],
        credentials_key: [42u8; 32],
        connectors_url: std::env::var("TEST_CONNECTORS_URL").ok(),
        connectors_token: std::env::var("TEST_CONNECTORS_TOKEN").ok(),
    };
    let state = AppState::new(db.clone(), config);
    let app = app(state);
    let email = format!("integ-{}@exemple.test", uuid::Uuid::new_v4());
    let r = call(
        &app,
        "POST",
        "/api/auth/register",
        Some(json!({"email": email, "name": "I", "password": "un mot de passe solide"})),
        None,
    )
    .await;
    let cookie = r.cookie.unwrap();
    let ws = call(
        &app,
        "POST",
        "/api/workspaces",
        Some(json!({"name": "Intégrations"})),
        Some(&cookie),
    )
    .await;
    Some(Env {
        app,
        db,
        cookie,
        ws: ws.body["id"].as_str().unwrap().to_string(),
    })
}

struct Res {
    status: StatusCode,
    body: Value,
    bytes: Vec<u8>,
    cookie: Option<String>,
}

async fn send(app: &Router, req: Request<Body>) -> Res {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let cookie = res
        .headers()
        .get(header::SET_COOKIE)
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    Res {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        bytes,
        cookie,
    }
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> Res {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let req = match body {
        Some(b) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    send(app, req).await
}

fn b64(v: Value) -> String {
    let batch = data::json_to_batch(&v).unwrap();
    base64::engine::general_purpose::STANDARD.encode(data::to_parquet(&batch).unwrap())
}

impl Env {
    async fn api(&self, method: &str, path: &str, body: Option<Value>) -> Res {
        call(
            &self.app,
            method,
            &format!("/api/workspaces/{}{path}", self.ws),
            body,
            Some(&self.cookie),
        )
        .await
    }

    async fn credential(&self, kind: &str, data: Value) -> String {
        let r = self.api("POST", "/credentials", Some(json!({"name": format!("{kind}-{}", uuid::Uuid::new_v4()), "kind": kind, "data": data}))).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
        r.body["id"].as_str().unwrap().to_string()
    }

    /// Exécute une intégration comme l'éditeur ; renvoie les lignes JSON de la sortie.
    async fn run(&self, kind: &str, params: Value, inputs: Vec<Value>) -> Result<Value, String> {
        let body = json!({
            "node": {"id": "n", "kind": kind, "name": kind, "params": params},
            "inputs": inputs.into_iter().map(b64).collect::<Vec<_>>(),
        });
        let r = self.api("POST", "/integrations/run", Some(body)).await;
        if r.status != StatusCode::OK {
            return Err(r.body["error"].as_str().unwrap_or("?").to_string());
        }
        Ok(data::batch_to_json(&data::from_parquet(r.bytes).unwrap()).unwrap())
    }
}

fn pg_credential() -> Option<Value> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let (host, port, database, user, password) = db_parts(&url);
    Some(
        json!({"host": host, "port": port, "database": database, "user": user, "password": password, "ssl": "désactivé"}),
    )
}

#[tokio::test]
async fn credentials_are_write_only_and_encrypted() {
    let Some(env) = setup(true).await else { return };
    let id = env.credential("postgres", pg_credential().unwrap()).await;
    let list = env.api("GET", "/credentials", None).await;
    let item = list
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id.as_str())
        .unwrap()
        .clone();
    assert!(item["summary"].get("password").is_none(), "{item}");
    assert_eq!(item["summary"]["password_defini"], true);
    // Chiffré au repos : le JSON en clair n'apparaît pas en base.
    let secret: Vec<u8> = sqlx::query_scalar("SELECT secret FROM credentials WHERE id = $1::uuid")
        .bind(&id)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&secret).contains("\"password\""));
    // Test de connexion.
    let t = env
        .api("POST", &format!("/credentials/{id}/test"), None)
        .await;
    assert_eq!(t.body["ok"], true, "{:?}", t.body);
    // Modification sans ressaisir le mot de passe : la connexion fonctionne toujours.
    let mut data = pg_credential().unwrap();
    data["password"] = json!("");
    let up = env
        .api(
            "PUT",
            &format!("/credentials/{id}"),
            Some(json!({"name": "renommé", "data": data})),
        )
        .await;
    assert_eq!(up.status, StatusCode::OK);
    assert_eq!(
        env.api("POST", &format!("/credentials/{id}/test"), None)
            .await
            .body["ok"],
        true
    );

    // Un autre workspace ne peut pas utiliser cet identifiant.
    let other = setup(true).await.unwrap();
    let err = other
        .run(
            "postgres_query",
            json!({"credential": id, "query": "SELECT 1"}),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(err.contains("introuvable"), "{err}");
}

#[tokio::test]
async fn postgres_write_query_and_upsert() {
    let Some(env) = setup(true).await else { return };
    let cred = env.credential("postgres", pg_credential().unwrap()).await;
    let table = format!("integ_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE TABLE {table} (id int PRIMARY KEY, nom text, prix numeric(10,2), cree date, actif boolean, meta jsonb)"))
        .execute(&env.db)
        .await
        .unwrap();

    let rows = json!([
        {"id": 1, "Nom": "Clavier", "prix": 49.9, "cree": "2026-10-01", "actif": true, "meta": "{\"couleur\": \"noir\"}"},
        {"id": 2, "Nom": "Souris", "prix": 19.5, "cree": "2026-10-02", "actif": false, "meta": null}
    ]);
    let out = env
        .run(
            "postgres_write",
            json!({"credential": cred, "table": table, "mode": "insertion"}),
            vec![rows],
        )
        .await
        .unwrap();
    assert_eq!(out[0]["lignes"], 2);

    let read = env
        .run(
            "postgres_query",
            json!({"credential": cred, "query": format!("SELECT * FROM {table} ORDER BY id")}),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(read[0]["nom"], "Clavier");
    assert_eq!(read[0]["prix"], 49.9);
    assert_eq!(read[0]["cree"], "2026-10-01");
    assert_eq!(read[0]["actif"], true);
    assert_eq!(read[0]["meta.couleur"], "noir");
    assert_eq!(read[1]["actif"], false);

    // Mise à jour sur la clé primaire.
    let upd = json!([{"id": 2, "nom": "Souris sans fil", "prix": 25}, {"id": 3, "nom": "Écran", "prix": 199}]);
    env.run("postgres_write", json!({"credential": cred, "table": table, "mode": "insertion ou mise à jour", "key": "id"}), vec![upd])
        .await
        .unwrap();
    let read = env
        .run("postgres_query", json!({"credential": cred, "query": format!("SELECT id, nom, prix FROM {table} ORDER BY id")}), vec![])
        .await
        .unwrap();
    assert_eq!(read.as_array().unwrap().len(), 3);
    assert_eq!(read[1]["nom"], "Souris sans fil");
    assert_eq!(read[1]["prix"], 25.0);

    // Injection via le nom de table : refusée.
    let err = env
        .run(
            "postgres_write",
            json!({"credential": cred, "table": "x; DROP TABLE users", "mode": "insertion"}),
            vec![json!([{"a": 1}])],
        )
        .await
        .unwrap_err();
    assert!(err.contains("Nom de table invalide"));
    // Erreur SQL lisible.
    let err = env
        .run(
            "postgres_query",
            json!({"credential": cred, "query": "SELECT * FROM table_inexistante"}),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(err.contains("table_inexistante"), "{err}");
    sqlx::query(&format!("DROP TABLE {table}"))
        .execute(&env.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn server_side_workflow_uses_integrations() {
    let Some(env) = setup(true).await else { return };
    let cred = env.credential("postgres", pg_credential().unwrap()).await;
    let table = format!("integ_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!(
        "CREATE TABLE {table} (commande text, montant numeric)"
    ))
    .execute(&env.db)
    .await
    .unwrap();
    let token = format!("tok_{}", uuid::Uuid::new_v4().simple());
    let node = |id: &str, kind: &str, params: Value| json!({"id": id, "position": {"x": 0, "y": 0}, "data": {"kind": kind, "name": id, "params": params}});
    let def = json!({"version": 1,
    "nodes": [
        node("hook", "trigger_webhook", json!({"token": token, "respond": "résultat du workflow"})),
        node("cols", "select", json!({"mode": "garder", "columns": "commande, montant"})),
        node("db", "postgres_write", json!({"credential": cred, "table": table, "mode": "insertion"})),
    ],
    "edges": [
        {"id": "e1", "source": "hook", "target": "cols", "targetHandle": "in-0"},
        {"id": "e2", "source": "cols", "target": "db", "targetHandle": "in-0"}
    ]});
    let wf = env
        .api(
            "POST",
            "/workflows",
            Some(json!({"name": "Commandes", "definition": def})),
        )
        .await;
    let wf_id = wf.body["id"].as_str().unwrap().to_string();
    assert_eq!(
        env.api(
            "PUT",
            &format!("/workflows/{wf_id}/active"),
            Some(json!({"active": true}))
        )
        .await
        .status,
        StatusCode::OK
    );
    let hook = call(
        &env.app,
        "POST",
        &format!("/api/hooks/{token}"),
        Some(json!({"commande": "C-42", "montant": 99.5})),
        None,
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{:?}", hook.body);
    assert_eq!(hook.body[0]["lignes"], 1);
    let (cmd, montant): (String, f64) =
        sqlx::query_as(&format!("SELECT commande, montant::float8 FROM {table}"))
            .fetch_one(&env.db)
            .await
            .unwrap();
    assert_eq!((cmd.as_str(), montant), ("C-42", 99.5));
    sqlx::query(&format!("DROP TABLE {table}"))
        .execute(&env.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn files_write_read_upload_download() {
    let Some(env) = setup(false).await else {
        return;
    };
    let rows = json!([{"produit": "Clavier", "prix": 49.9}, {"produit": "Souris", "prix": 19.5}]);
    let out = env
        .run(
            "file_write",
            json!({"name": "ventes-{lignes}", "format": "excel"}),
            vec![rows],
        )
        .await
        .unwrap();
    assert_eq!(out[0]["fichier"], "ventes-2.xlsx");
    let file_id = out[0]["id"].as_str().unwrap().to_string();
    let read = env
        .run("file_read", json!({"file": file_id}), vec![])
        .await
        .unwrap();
    assert_eq!(read[1]["produit"], "Souris");
    assert_eq!(read[1]["prix"], 19.5);

    // Téléversement d'un CSV au séparateur « ; » puis lecture.
    let req = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/workspaces/{}/files?name=../../clients.csv",
            env.ws
        ))
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, &env.cookie)
        .body(Body::from("nom;ville\nAda;Paris\nAlan;Londres\n"))
        .unwrap();
    let up = send(&env.app, req).await;
    assert_eq!(up.status, StatusCode::CREATED, "{:?}", up.body);
    assert_eq!(up.body["name"], "clients.csv");
    let csv_id = up.body["id"].as_str().unwrap().to_string();
    let read = env
        .run("file_read", json!({"file": csv_id}), vec![])
        .await
        .unwrap();
    assert_eq!(read[1]["ville"], "Londres");

    // Téléchargement : pièce jointe, jamais interprétée par le navigateur.
    let dl = env
        .api("GET", &format!("/files/{csv_id}/content"), None)
        .await;
    assert_eq!(dl.status, StatusCode::OK);
    assert!(String::from_utf8_lossy(&dl.bytes).starts_with("nom;ville"));
    let list = env.api("GET", "/files", None).await;
    assert_eq!(list.body.as_array().unwrap().len(), 2);
    assert_eq!(
        env.api("DELETE", &format!("/files/{csv_id}"), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn private_databases_are_blocked_by_default() {
    let Some(env) = setup(false).await else {
        return;
    };
    let cred = env.credential("postgres", pg_credential().unwrap()).await;
    let err = env
        .run(
            "postgres_query",
            json!({"credential": cred, "query": "SELECT 1"}),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(err.contains("non autorisée"), "{err}");
    let t = env
        .api("POST", &format!("/credentials/{cred}/test"), None)
        .await;
    assert_eq!(t.body["ok"], false);
}

#[tokio::test]
async fn mysql_write_and_query() {
    let Ok(spec) = std::env::var("TEST_MYSQL") else {
        return;
    };
    let Some(env) = setup(true).await else { return };
    let p: Vec<&str> = spec.split(':').collect();
    let cred = env
        .credential("mysql", json!({"host": p[0], "port": p[1], "database": p[2], "user": p[3], "password": p[4], "ssl": "désactivé"}))
        .await;
    let table = format!("integ_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    env.run("mysql_query", json!({"credential": cred, "query": format!("CREATE TABLE {table} (id INT PRIMARY KEY, nom VARCHAR(50), prix DECIMAL(10,2), cree DATE)")}), vec![])
        .await
        .unwrap();
    let rows = json!([{"id": 1, "nom": "Clavier", "prix": 49.9, "cree": "2026-10-01"}, {"id": 2, "nom": "Souris", "prix": null, "cree": null}]);
    env.run(
        "mysql_write",
        json!({"credential": cred, "table": table, "mode": "insertion"}),
        vec![rows],
    )
    .await
    .unwrap();
    env.run(
        "mysql_write",
        json!({"credential": cred, "table": table, "mode": "insertion ou mise à jour"}),
        vec![json!([{"id": 2, "nom": "Souris v2", "prix": 25}])],
    )
    .await
    .unwrap();
    let read = env
        .run(
            "mysql_query",
            json!({"credential": cred, "query": format!("SELECT * FROM {table} ORDER BY id")}),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(read[0]["prix"], 49.9);
    assert_eq!(read[0]["cree"], "2026-10-01");
    assert_eq!(read[1]["nom"], "Souris v2");
    assert_eq!(read.as_array().unwrap().len(), 2);
    env.run(
        "mysql_query",
        json!({"credential": cred, "query": format!("DROP TABLE {table}")}),
        vec![],
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(10)).await;
}

/// Oracle via le service connectors (`TEST_ORACLE` = `hôte:port:service:utilisateur:mot_de_passe`,
/// `TEST_CONNECTORS_URL`, `TEST_CONNECTORS_TOKEN`).
#[tokio::test]
async fn oracle_through_connectors() {
    let Ok(spec) = std::env::var("TEST_ORACLE") else {
        return;
    };
    let Some(env) = setup(true).await else { return };
    let p: Vec<&str> = spec.split(':').collect();
    let cred = env
        .credential(
            "oracle",
            json!({"host": p[0], "port": p[1], "service": p[2], "user": p[3], "password": p[4]}),
        )
        .await;
    let t = env
        .api("POST", &format!("/credentials/{cred}/test"), None)
        .await;
    assert_eq!(t.body["ok"], true, "{:?}", t.body);

    let table =
        format!("INTEG_{}", &uuid::Uuid::new_v4().simple().to_string()[..10]).to_uppercase();
    env.run(
        "oracle_query",
        json!({"credential": cred, "query": format!("CREATE TABLE {table} (ID NUMBER PRIMARY KEY, NOM VARCHAR2(50), PRIX NUMBER(10,2), CREE DATE, ACTIF NUMBER(1))")}),
        vec![],
    )
    .await
    .unwrap();
    let rows = json!([
        {"ID": 1, "NOM": null, "PRIX": 49.9, "ACTIF": true},
        {"ID": 2, "NOM": "Souris", "PRIX": null, "ACTIF": false}
    ]);
    let out = env
        .run(
            "oracle_write",
            json!({"credential": cred, "table": table}),
            vec![rows],
        )
        .await
        .unwrap();
    assert_eq!(out[0]["lignes"], 2);
    let read = env
        .run(
            "oracle_query",
            json!({"credential": cred, "query": format!("SELECT ID, NOM, PRIX, ACTIF, DATE '2026-10-12' AS JOUR FROM {table} ORDER BY ID")}),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(read[0]["PRIX"], 49.9);
    assert!(read[0]["NOM"].is_null());
    assert_eq!(read[0]["ACTIF"], 1);
    assert_eq!(read[1]["NOM"], "Souris");
    assert!(read[1]["JOUR"].as_str().unwrap().starts_with("2026-10-1"));
    let err = env
        .run(
            "oracle_query",
            json!({"credential": cred, "query": "SELECT * FROM TABLE_INEXISTANTE"}),
            vec![],
        )
        .await
        .unwrap_err();
    assert!(err.contains("ORA-00942"), "{err}");
    env.run(
        "oracle_query",
        json!({"credential": cred, "query": format!("DROP TABLE {table}")}),
        vec![],
    )
    .await
    .unwrap();
}
