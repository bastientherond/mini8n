//! Serveur MCP (IA connectées) de bout en bout. Nécessite PostgreSQL :
//! `TEST_DATABASE_URL=postgres://… cargo test -p mini8n-server` (ignoré sinon).

use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::StreamExt;
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
    setup_with(false).await
}

/// `allow_private` : bases de données locales joignables (tests de lecture SQL).
async fn setup_with(allow_private: bool) -> Option<Server> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let db = PgPool::connect(&url).await.expect("connexion");
    MIGRATOR.run(&db).await.expect("migrations");
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: url,
        database_max_connections: 10,
        public_origins: vec![ORIGIN.into()],
        cookie_secure: CookieSecure::Never,
        trust_proxy: false,
        session_ttl_hours: 1,
        allow_registration: true,
        mail: None,
        http_allow_private: allow_private,
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
    let email = format!("mcp-{}@exemple.test", uuid::Uuid::new_v4());
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

/// Appel MCP (JSON-RPC) avec un jeton.
async fn mcp(
    app: &axum::Router,
    token: Option<&str>,
    origin: Option<&str>,
    msg: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/mcp")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream");
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    if let Some(o) = origin {
        req = req.header(header::ORIGIN, o);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(msg.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Appel d'outil : (résultat structuré, erreur métier ?).
async fn call(app: &axum::Router, token: &str, name: &str, args: Value) -> (Value, bool) {
    let (status, res) = mcp(
        app,
        Some(token),
        None,
        json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": name, "arguments": args}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{res}");
    let r = &res["result"];
    let error = r["isError"].as_bool().unwrap_or(false);
    (
        if error {
            r["content"][0]["text"].clone()
        } else {
            r["structuredContent"].clone()
        },
        error,
    )
}

/// Attend un message du WebSocket qui satisfait `pred`.
async fn wait_for(socket: &mut Socket, pred: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..50 {
        let msg = next(socket).await;
        if pred(&msg) {
            return msg;
        }
    }
    panic!("message attendu non reçu");
}

#[tokio::test]
async fn ai_debugs_a_workflow_live() {
    let Some(server) = setup().await else { return };
    let app = &server.app;
    let (alice, _) = register(app, "Alice").await;
    let (_, ws, _) = api(
        app,
        "POST",
        "/api/workspaces",
        json!({"name": "Équipe"}),
        Some(&alice),
    )
    .await;
    let ws = ws["id"].as_str().unwrap().to_string();
    let data = r#"[{"produit": "Clavier", "prix": 49.9}, {"produit": "Souris", "prix": 19.5}, {"produit": "Câble", "prix": 5}]"#;
    let def = json!({
        "nodes": [
            {"id": "go", "position": {"x": 0, "y": 0}, "data": {"kind": "trigger_manual", "name": "Départ", "params": {}}},
            {"id": "src", "position": {"x": 260, "y": 0}, "data": {"kind": "manual", "name": "Produits", "params": {"data": data}}},
            {"id": "cher", "position": {"x": 520, "y": 0}, "data": {"kind": "filter", "name": "Chers", "params": {"condition": "$json.prx > 10"}}}
        ],
        "edges": [
            {"id": "e1", "source": "go", "target": "src", "targetHandle": "in-0"},
            {"id": "e2", "source": "src", "target": "cher", "targetHandle": "in-0"}
        ]
    });
    let (_, wf, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/workflows"),
        json!({"name": "Ventes", "definition": def}),
        Some(&alice),
    )
    .await;
    let wf = wf["id"].as_str().unwrap().to_string();

    // Sans jeton (ou avec un faux) : refusé.
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}});
    let (status, _) = mcp(app, None, None, init.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = mcp(app, Some("m8n_faux"), None, init.clone()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, created, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/tokens"),
        json!({"name": "Claude", "expiresInDays": 30}),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("m8n_"));
    let (_, list, _) = api(
        app,
        "GET",
        &format!("/api/workspaces/{ws}/tokens"),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert!(
        list[0].get("token").is_none(),
        "le jeton n'est affiché qu'à la création"
    );

    // Un site tiers ne peut pas utiliser le jeton depuis un navigateur.
    let (status, _) = mcp(
        app,
        Some(&token),
        Some("https://malveillant.exemple"),
        init.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, res) = mcp(app, Some(&token), None, init).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(res["result"]["serverInfo"]["name"], "mini8n");
    let (status, _) = mcp(
        app,
        Some(&token),
        None,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let (_, res) = mcp(
        app,
        Some(&token),
        None,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    )
    .await;
    assert!(
        res["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "run_workflow")
    );

    // Alice a l'éditeur ouvert : elle voit l'IA arriver, exécuter et corriger.
    let live = format!("/api/workspaces/{ws}/workflows/{wf}/live");
    let mut socket = open(&server, &live, &alice, ORIGIN).await.unwrap();
    assert_eq!(next(&mut socket).await["t"], "welcome");

    let (workflows, _) = call(app, &token, "list_workflows", json!({})).await;
    assert_eq!(workflows["workflows"][0]["openBy"], 1);
    let (view, _) = call(app, &token, "get_workflow", json!({"workflow_id": wf})).await;
    assert_eq!(view["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(view["nodes"][2]["inputs"][0]["from"], json!(["src"]));
    let joined = wait_for(&mut socket, |m| m["t"] == "join").await;
    assert_eq!(joined["peer"]["name"], "Claude · IA de Alice");

    // Première exécution : l'erreur de colonne est signalée sur le bon nœud.
    let (run, _) = call(app, &token, "run_workflow", json!({"workflow_id": wf})).await;
    assert_eq!(run["status"], "error", "{run}");
    let failed = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "cher")
        .unwrap();
    assert!(
        failed["error"].as_str().unwrap().contains("prx"),
        "{failed}"
    );
    let src = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "src")
        .unwrap();
    assert_eq!(src["rows"], 3);
    assert_eq!(src["sample"].as_array().unwrap().len(), 3);
    let done = wait_for(&mut socket, |m| m["t"] == "run" && m["run"]["done"] == true).await;
    assert_eq!(done["run"]["ok"], false);
    assert_eq!(done["run"]["results"]["src"]["rows"], 3);

    let (page, _) = call(
        app,
        &token,
        "get_node_output",
        json!({"execution_id": run["executionId"], "node_id": "src", "offset": 1, "limit": 1}),
    )
    .await;
    assert_eq!(page["totalRows"], 3);
    assert_eq!(page["rows"], json!([{"produit": "Souris", "prix": 19.5}]));

    // Correction : enregistrée et diffusée à l'éditeur ouvert.
    let (bad, error) = call(
        app,
        &token,
        "update_node",
        json!({"workflow_id": wf, "node_id": "cher", "params": {"nope": 1}}),
    )
    .await;
    assert!(
        error && bad.as_str().unwrap().contains("Paramètre inconnu"),
        "{bad}"
    );
    let (fixed, _) = call(
        app,
        &token,
        "update_node",
        json!({"workflow_id": wf, "node_id": "cher", "params": {"condition": "$json.prix > 10"}}),
    )
    .await;
    assert_eq!(fixed["version"], 2);
    let saved = wait_for(&mut socket, |m| m["t"] == "saved").await;
    assert_eq!(
        saved["doc"]["definition"]["nodes"][2]["data"]["params"]["condition"],
        "$json.prix > 10"
    );

    let (added, _) = call(app, &token, "add_node", json!({"workflow_id": wf, "kind": "sort", "params": {"by": "prix"}, "connect_from": "cher"})).await;
    let sort = added["node_id"].as_str().unwrap().to_string();
    let (_, error) = call(
        app,
        &token,
        "connect_nodes",
        json!({"workflow_id": wf, "source": sort, "target": "src"}),
    )
    .await;
    assert!(error, "boucle refusée");

    let (run, _) = call(
        app,
        &token,
        "run_workflow",
        json!({"workflow_id": wf, "node_id": sort, "sample_rows": 5}),
    )
    .await;
    assert_eq!(run["status"], "success", "{run}");
    // Exécution terminée : l'indicateur de l'IA disparaît du canevas.
    wait_for(&mut socket, |m| m["t"] == "focus" && m["node"].is_null()).await;
    let last = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == sort.as_str())
        .unwrap();
    assert_eq!(
        last["sample"],
        json!([{"produit": "Souris", "prix": 19.5}, {"produit": "Clavier", "prix": 49.9}])
    );

    let (history, _) = call(app, &token, "list_executions", json!({"workflow_id": wf})).await;
    assert_eq!(history["executions"].as_array().unwrap().len(), 2);
    let (detail, _) = call(
        app,
        &token,
        "get_execution",
        json!({"execution_id": run["executionId"]}),
    )
    .await;
    assert_eq!(detail["status"], "success");

    // Création d'un workflow et identifiants disponibles (sans secret).
    let (created_wf, _) = call(app, &token, "create_workflow", json!({"name": "Rapport"})).await;
    let new_id = created_wf["workflow_id"].as_str().unwrap();
    let (node, _) = call(
        app,
        &token,
        "add_node",
        json!({"workflow_id": new_id, "kind": "trigger_manual"}),
    )
    .await;
    assert!(
        node["node_id"]
            .as_str()
            .unwrap()
            .starts_with("trigger_manual-")
    );
    let (creds, _) = call(app, &token, "list_credentials", json!({})).await;
    assert_eq!(creds["credentials"], json!([]));

    // Un autre workspace reste inaccessible avec ce jeton.
    let (_, other, _) = api(
        app,
        "POST",
        "/api/workspaces",
        json!({"name": "Privé"}),
        Some(&alice),
    )
    .await;
    let other = other["id"].as_str().unwrap();
    let (_, owf, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{other}/workflows"),
        json!({"name": "Secret", "definition": {"nodes": [], "edges": []}}),
        Some(&alice),
    )
    .await;
    let (msg, error) = call(
        app,
        &token,
        "get_workflow",
        json!({"workflow_id": owf["id"]}),
    )
    .await;
    assert!(error && msg.as_str().unwrap().contains("introuvable"));

    // Jeton révoqué : l'IA quitte l'éditeur et ne peut plus rien faire.
    let id = created["id"].as_str().unwrap();
    let (status, _, _) = api(
        app,
        "DELETE",
        &format!("/api/workspaces/{ws}/tokens/{id}"),
        Value::Null,
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    wait_for(&mut socket, |m| m["t"] == "leave").await;
    let (status, _) = mcp(
        app,
        Some(&token),
        None,
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// Portées des jetons : lecture seule, test (simulation, workflows inactifs), complet.
#[tokio::test]
async fn token_scopes_limit_what_the_ai_can_do() {
    let Some(server) = setup_with(true).await else {
        return;
    };
    let app = &server.app;
    let (alice, _) = register(app, "Alice").await;
    let (_, ws, _) = api(
        app,
        "POST",
        "/api/workspaces",
        json!({"name": "Portées"}),
        Some(&alice),
    )
    .await;
    let ws = ws["id"].as_str().unwrap().to_string();
    let token = |scope: &'static str| {
        let ws = ws.clone();
        let alice = alice.clone();
        async move {
            let (status, t, _) = api(
                app,
                "POST",
                &format!("/api/workspaces/{ws}/tokens"),
                json!({"name": scope, "scope": scope}),
                Some(&alice),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{t}");
            assert_eq!(t["scope"], scope);
            t["token"].as_str().unwrap().to_string()
        }
    };
    let (read, test, full) = (
        token("lecture").await,
        token("test").await,
        token("complet").await,
    );

    // Base de test (lecture) et workflow : déclencheur → lecture SQL → e-mail.
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let u = url::Url::parse(&url).unwrap();
    let (_, cred, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/credentials"),
        json!({"name": "Test", "kind": "postgres", "data": {
            "host": u.host_str().unwrap(), "port": u.port().unwrap_or(5432), "database": u.path().trim_start_matches('/'),
            "user": u.username(), "password": u.password().unwrap_or_default(), "ssl": "désactivé"}}),
        Some(&alice),
    )
    .await;
    let (_, smtp, _) = api(
        app,
        "POST",
        &format!("/api/workspaces/{ws}/credentials"),
        json!({"name": "SMTP", "kind": "smtp", "data": {"host": "smtp.exemple.test", "port": "587", "security": "STARTTLS", "user": "", "password": "", "from": "a@exemple.test"}}),
        Some(&alice),
    )
    .await;
    let (wf, _) = call(app, &test, "create_workflow", json!({"name": "Rapport"})).await;
    let wf = wf["workflow_id"].as_str().unwrap().to_string();
    let (go, _) = call(
        app,
        &test,
        "add_node",
        json!({"workflow_id": wf, "kind": "trigger_schedule"}),
    )
    .await;
    let (q, _) = call(app, &test, "add_node", json!({"workflow_id": wf, "kind": "postgres_query", "connect_from": go["node_id"],
        "params": {"credential": cred["id"], "query": "SELECT current_setting('transaction_read_only') AS ro"}})).await;
    let (mail, _) = call(app, &test, "add_node", json!({"workflow_id": wf, "kind": "email_send", "name": "Envoi", "connect_from": q["node_id"],
        "params": {"credential": smtp["id"], "to": "x@exemple.test"}})).await;
    let q = q["node_id"].as_str().unwrap();

    // Lecture : ni exécution ni modification ; les outils proposés le reflètent.
    let (_, listed) = mcp(
        app,
        Some(&read),
        None,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
    )
    .await;
    let names: Vec<&str> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"get_workflow")
            && !names.contains(&"run_workflow")
            && !names.contains(&"update_node"),
        "{names:?}"
    );
    let (msg, error) = call(app, &read, "run_workflow", json!({"workflow_id": wf})).await;
    assert!(
        error && msg.as_str().unwrap().contains("lecture seule"),
        "{msg}"
    );
    let (_, error) = call(app, &read, "get_workflow", json!({"workflow_id": wf})).await;
    assert!(!error);

    // Test : simulation — l'e-mail est neutralisé (rien n'est envoyé, aucune erreur SMTP),
    // la base est lue en lecture seule.
    let (run, _) = call(app, &test, "run_workflow", json!({"workflow_id": wf})).await;
    assert_eq!(run["status"], "success", "{run}");
    assert_eq!(run["simulation"]["neutralised"], json!(["Envoi"]));
    let read_only = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == q)
        .unwrap();
    assert_eq!(read_only["sample"], json!([{"ro": "on"}]), "{read_only}");

    // Contournements de la lecture seule (revue de sécurité) : refusés avant exécution, ou
    // bloqués par la transaction en lecture seule.
    for attack in [
        "SET default_transaction_read_only = off; COMMIT; CREATE TABLE mini8n_intrus (x int)",
        "BEGIN READ WRITE; CREATE TABLE mini8n_intrus (x int); COMMIT",
        "SELECT set_config('transaction_read_only', 'off', false)",
    ] {
        call(
            app,
            &test,
            "update_node",
            json!({"workflow_id": wf, "node_id": q, "params": {"query": attack}}),
        )
        .await;
        let (run, _) = call(app, &test, "run_workflow", json!({"workflow_id": wf})).await;
        let node = run["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == q)
            .unwrap();
        assert_eq!(node["status"], "error", "{attack} : {node}");
    }
    let (probe, _) = call(
        app,
        &full,
        "run_workflow",
        json!({"workflow_id": wf, "node_id": q}),
    )
    .await;
    call(app, &test, "update_node", json!({"workflow_id": wf, "node_id": q, "params": {"query": "SELECT to_regclass('mini8n_intrus')::text AS t"}})).await;
    let (check, _) = call(
        app,
        &full,
        "run_workflow",
        json!({"workflow_id": wf, "node_id": q}),
    )
    .await;
    let node = check["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == q)
        .unwrap();
    assert_eq!(
        node["sample"],
        json!([{"t": null}]),
        "aucune table créée ({probe})"
    );
    call(app, &test, "update_node", json!({"workflow_id": wf, "node_id": q, "params": {"query": "SELECT current_setting('transaction_read_only') AS ro"}})).await;

    // Complet : exécution réelle (l'envoi est tenté et échoue : serveur SMTP fictif).
    let (run, _) = call(app, &full, "run_workflow", json!({"workflow_id": wf})).await;
    assert!(run.get("simulation").is_none());
    let sent = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == mail["node_id"])
        .unwrap();
    assert_eq!(sent["status"], "error", "{sent}");
    let real = run["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == q)
        .unwrap();
    assert_eq!(real["sample"], json!([{"ro": "off"}]));

    // Workflow appelable par d'autres (« Appelé par un workflow ») : il s'exécute réellement
    // depuis un workflow actif, un jeton « test » ne peut ni le modifier ni le rendre appelable.
    let (child, _) = call(
        app,
        &test,
        "create_workflow",
        json!({"name": "Sous-workflow"}),
    )
    .await;
    let child = child["workflow_id"].as_str().unwrap();
    let (msg, error) = call(
        app,
        &test,
        "add_node",
        json!({"workflow_id": child, "kind": "trigger_workflow"}),
    )
    .await;
    assert!(error && msg.as_str().unwrap().contains("appelé"), "{msg}");
    let (_, error) = call(
        app,
        &full,
        "add_node",
        json!({"workflow_id": child, "kind": "trigger_workflow"}),
    )
    .await;
    assert!(!error);
    let (msg, error) = call(
        app,
        &test,
        "add_node",
        json!({"workflow_id": child, "kind": "filter"}),
    )
    .await;
    assert!(error && msg.as_str().unwrap().contains("appelé"), "{msg}");

    // Workflow actif : seul un jeton complet peut le modifier.
    let (status, _, _) = api(
        app,
        "PUT",
        &format!("/api/workspaces/{ws}/workflows/{wf}/active"),
        json!({"active": true}),
        Some(&alice),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (msg, error) = call(
        app,
        &test,
        "update_node",
        json!({"workflow_id": wf, "node_id": q, "name": "Lecture"}),
    )
    .await;
    assert!(error && msg.as_str().unwrap().contains("actif"), "{msg}");
    let (_, error) = call(
        app,
        &full,
        "update_node",
        json!({"workflow_id": wf, "node_id": q, "name": "Lecture"}),
    )
    .await;
    assert!(!error);
}
