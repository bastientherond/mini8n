//! Collaboration en direct sur le canevas : curseurs, sélections, déplacements et nœud
//! en cours de paramétrage des autres membres, et diffusion des sauvegardes d'un workflow.
//!
//! - Transport : WebSocket sur la même origine (cookie de session, traverse nginx et Caddy).
//! - Une « salle » par workflow ouvert ; la présence de chacun y est mémorisée pour les
//!   arrivants.
//! - Le serveur ne relaie jamais un message client tel quel : il le valide puis le réémet
//!   avec l'identité de l'émetteur (impossible de se faire passer pour un autre membre).
//! - L'accès est revérifié périodiquement (session révoquée, membre retiré).

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, header};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, AppResult};
use crate::workflows::valid_id;
use crate::workspaces::require_member;

const MAX_PEERS_PER_ROOM: usize = 25;
/// Un état d'exécution complet (résultats et colonnes de chaque nœud) peut être volumineux.
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Autant que de nœuds dans un workflow.
const MAX_IDS: usize = 500;
const MAX_COORD: f64 = 1e7;
const QUEUE: usize = 256;
const PING_EVERY: Duration = Duration::from_secs(25);
const IDLE_TIMEOUT: Duration = Duration::from_secs(70);
const RECHECK_EVERY: Duration = Duration::from_secs(60);
/// Messages entrants par seconde (en rafale : le double).
const MESSAGES_PER_SEC: f64 = 60.0;
const MAX_COLUMNS: usize = 1000;
const MAX_TEXT: usize = 4000;
const COLORS: u8 = 8;

/// Codes de fermeture (plage 4000-4999 réservée aux applications).
pub mod close {
    /// Accès retiré (session expirée, membre retiré, workspace supprimé) : ne pas se reconnecter.
    pub const FORBIDDEN: u16 = 4403;
    /// Workflow supprimé : ne pas se reconnecter.
    pub const GONE: u16 = 4410;
    /// Trop de monde sur ce workflow.
    pub const FULL: u16 = 4429;
    /// Client trop lent ou trop bavard : reconnexion possible.
    pub const RETRY: u16 = 1013;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.x.abs() < MAX_COORD
            && self.y.abs() < MAX_COORD
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Success,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Column {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
}

/// Résultat d'un nœud (sans les données, qui restent dans le navigateur qui exécute).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NodeResult {
    status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    rows: u64,
    #[serde(default)]
    columns: Vec<Column>,
    duration_ms: f64,
    #[serde(default)]
    bytes: u64,
}

/// Exécution en cours dans le navigateur d'un membre, affichée en direct chez les autres.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunState {
    /// Nœuds pas encore terminés.
    pending: Vec<String>,
    /// Nœud en cours d'exécution.
    active: Option<String>,
    results: HashMap<String, NodeResult>,
    /// Exécution terminée (bilan ci-dessous).
    #[serde(default)]
    done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<f64>,
}

impl RunState {
    fn valid(&self) -> bool {
        let short = |s: &str| s.chars().count() <= MAX_TEXT;
        let ms = |v: f64| v.is_finite() && v >= 0.0;
        self.pending.len() <= MAX_IDS
            && self.results.len() <= MAX_IDS
            && self.pending.iter().all(|id| valid_id(id))
            && self.active.as_deref().is_none_or(valid_id)
            && self.error.as_deref().is_none_or(short)
            && self.duration_ms.is_none_or(ms)
            && self.results.iter().all(|(id, r)| {
                valid_id(id)
                    && ms(r.duration_ms)
                    && r.error.as_deref().is_none_or(short)
                    && r.columns.len() <= MAX_COLUMNS
                    && r.columns
                        .iter()
                        .all(|c| c.name.len() <= 256 && c.data_type.len() <= 128)
            })
    }
}

/// Message d'un client.
#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
enum Action {
    /// Position du pointeur sur le canevas (`null` : hors du canevas).
    Cursor { at: Option<Point> },
    /// Nœuds sélectionnés.
    Select { nodes: Vec<String> },
    /// Nœud en cours de paramétrage.
    Focus { node: Option<String> },
    /// Nœuds en cours de déplacement (positions absolues) ; `done` à la fin du geste.
    Drag {
        positions: HashMap<String, Point>,
        #[serde(default)]
        done: bool,
    },
    /// Exécution du workflow dans son navigateur (`null` : abandonnée).
    Run { run: Option<RunState> },
}

impl Action {
    fn parse(text: &str) -> Option<Self> {
        let action: Self = serde_json::from_str(text).ok()?;
        let ok = match &action {
            Action::Cursor { at } => at.is_none_or(|p| p.valid()),
            Action::Select { nodes } => {
                nodes.len() <= MAX_IDS && nodes.iter().all(|id| valid_id(id))
            }
            Action::Focus { node } => node.as_deref().is_none_or(valid_id),
            Action::Drag { positions, .. } => {
                positions.len() <= MAX_IDS
                    && positions.iter().all(|(id, p)| valid_id(id) && p.valid())
            }
            Action::Run { run } => run.as_ref().is_none_or(RunState::valid),
        };
        ok.then_some(action)
    }
}

#[derive(Debug, Default, Clone, Serialize)]
struct Presence {
    cursor: Option<Point>,
    selection: Vec<String>,
    focus: Option<String>,
    /// Déplacement en cours (vide sinon).
    drag: HashMap<String, Point>,
    /// Exécution en cours (les arrivants la voient aussi).
    run: Option<RunState>,
}

enum Out {
    Text(Utf8Bytes),
    Close(u16, &'static str),
}

struct Peer {
    user: Uuid,
    name: String,
    color: u8,
    /// `None` : participant sans connexion (IA connectée par MCP).
    tx: Option<mpsc::Sender<Out>>,
    presence: Presence,
}

#[derive(Serialize)]
struct PeerView<'a> {
    id: u64,
    user: Uuid,
    name: &'a str,
    color: u8,
    /// IA connectée par MCP.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    agent: bool,
    #[serde(flatten)]
    presence: &'a Presence,
}

impl Peer {
    fn view(&self, id: u64) -> PeerView<'_> {
        PeerView {
            id,
            user: self.user,
            name: &self.name,
            color: self.color,
            agent: self.tx.is_none(),
            presence: &self.presence,
        }
    }
}

struct Room {
    workspace: Uuid,
    peers: HashMap<u64, Peer>,
}

impl Room {
    /// Envoie un message à tous les membres de la salle (sauf `except`).
    /// Un message `transient` (curseur, étape d'un déplacement) peut être perdu : il sera
    /// remplacé par le suivant. Sinon, un client dont la file est pleine est déconnecté
    /// (il se reconnectera et recevra un état complet).
    fn send(&mut self, text: Utf8Bytes, except: Option<u64>, transient: bool) {
        self.peers.retain(|id, peer| {
            if Some(*id) == except {
                return true;
            }
            let Some(tx) = &peer.tx else {
                return true;
            };
            match tx.try_send(Out::Text(text.clone())) {
                Ok(()) => true,
                Err(mpsc::error::TrySendError::Full(_)) => transient,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            }
        });
    }
}

fn text(value: serde_json::Value) -> Utf8Bytes {
    Utf8Bytes::from(value.to_string())
}

/// Salles ouvertes, par workflow (en mémoire : une seule instance de l'API).
#[derive(Default)]
pub struct Hub {
    next_id: AtomicU64,
    rooms: Mutex<HashMap<Uuid, Room>>,
}

impl Hub {
    fn rooms(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, Room>> {
        self.rooms.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Entre dans la salle d'un workflow : reçoit l'état des autres, qui sont prévenus.
    fn join(
        &self,
        workspace: Uuid,
        workflow: Uuid,
        user: &AuthUser,
        tx: mpsc::Sender<Out>,
    ) -> Option<u64> {
        self.enter(workspace, workflow, user.id, &user.name, Some(tx))
    }

    /// Une IA (jeton MCP) rejoint la salle : les membres la voient comme un participant
    /// (nœud en cours d'examen, exécutions), sans connexion WebSocket.
    pub fn agent_join(
        &self,
        workspace: Uuid,
        workflow: Uuid,
        user: Uuid,
        name: &str,
    ) -> Option<u64> {
        self.enter(workspace, workflow, user, name, None)
    }

    /// L'IA quitte la salle.
    pub fn agent_leave(&self, workflow: Uuid, id: u64) {
        self.leave(workflow, id);
    }

    /// L'IA est-elle toujours dans la salle (salle non supprimée, accès non retiré) ?
    pub fn agent_present(&self, workflow: Uuid, id: u64) -> bool {
        self.rooms()
            .get(&workflow)
            .is_some_and(|r| r.peers.contains_key(&id))
    }

    /// Nœud examiné par l'IA.
    pub fn agent_focus(&self, workflow: Uuid, id: u64, node: Option<String>) {
        if node.as_deref().is_none_or(valid_id) {
            self.act(workflow, id, Action::Focus { node });
        }
    }

    /// L'IA a fini avec `node` : son indicateur disparaît, sauf si elle est passée à un autre nœud.
    pub fn agent_blur(&self, workflow: Uuid, id: u64, node: Option<&str>) {
        let current = self
            .rooms()
            .get(&workflow)
            .and_then(|r| r.peers.get(&id))
            .map(|p| p.presence.focus.clone());
        if let Some(Some(focus)) = current
            && node.is_none_or(|n| n == focus)
        {
            self.act(workflow, id, Action::Focus { node: None });
        }
    }

    /// Exécution lancée par l'IA (même format que celle d'un navigateur).
    pub fn agent_run(&self, workflow: Uuid, id: u64, run: serde_json::Value) {
        match serde_json::from_value::<RunState>(run) {
            Ok(run) if run.valid() => self.act(workflow, id, Action::Run { run: Some(run) }),
            _ => {
                tracing::warn!(workflow_id = %workflow, "collaboration : exécution de l'IA ignorée")
            }
        }
    }

    fn enter(
        &self,
        workspace: Uuid,
        workflow: Uuid,
        user: Uuid,
        name: &str,
        tx: Option<mpsc::Sender<Out>>,
    ) -> Option<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let mut rooms = self.rooms();
        let room = rooms.entry(workflow).or_insert_with(|| Room {
            workspace,
            peers: HashMap::new(),
        });
        if room.peers.len() >= MAX_PEERS_PER_ROOM {
            return None;
        }
        // Première couleur libre dans la salle.
        let used: Vec<u8> = room.peers.values().map(|p| p.color).collect();
        let color = (0..COLORS)
            .find(|c| !used.contains(c))
            .unwrap_or((room.peers.len() % COLORS as usize) as u8);
        let peers: Vec<_> = room.peers.iter().map(|(id, p)| p.view(*id)).collect();
        if let Some(tx) = &tx {
            let welcome =
                text(json!({ "t": "welcome", "you": id, "color": color, "peers": peers }));
            tx.try_send(Out::Text(welcome)).ok()?;
        }
        let peer = Peer {
            user,
            name: name.to_string(),
            color,
            tx,
            presence: Presence::default(),
        };
        let joined = text(json!({ "t": "join", "peer": peer.view(id) }));
        room.send(joined, None, false);
        room.peers.insert(id, peer);
        Some(id)
    }

    fn leave(&self, workflow: Uuid, id: u64) {
        let mut rooms = self.rooms();
        let Some(room) = rooms.get_mut(&workflow) else {
            return;
        };
        if room.peers.remove(&id).is_some() {
            room.send(text(json!({ "t": "leave", "id": id })), None, false);
        }
        if room.peers.is_empty() {
            rooms.remove(&workflow);
        }
    }

    /// Applique l'action d'un membre et la relaie aux autres.
    fn act(&self, workflow: Uuid, id: u64, action: Action) {
        let mut rooms = self.rooms();
        let Some(room) = rooms.get_mut(&workflow) else {
            return;
        };
        let Some(peer) = room.peers.get_mut(&id) else {
            return;
        };
        let p = &mut peer.presence;
        let (msg, transient) = match action {
            Action::Cursor { at } => {
                p.cursor = at;
                (json!({ "t": "cursor", "id": id, "at": at }), at.is_some())
            }
            Action::Select { nodes } => {
                let msg = json!({ "t": "select", "id": id, "nodes": nodes });
                p.selection = nodes;
                (msg, false)
            }
            Action::Focus { node } => {
                let msg = json!({ "t": "focus", "id": id, "node": node });
                p.focus = node;
                (msg, false)
            }
            Action::Drag { positions, done } => {
                let msg = json!({ "t": "drag", "id": id, "positions": positions, "done": done });
                p.drag = if done { HashMap::new() } else { positions };
                (msg, !done)
            }
            Action::Run { run } => {
                let msg = json!({ "t": "run", "id": id, "run": run });
                // Une étape peut être remplacée par la suivante, pas la fin de l'exécution.
                let transient = run.as_ref().is_some_and(|r| !r.done);
                p.run = run.filter(|r| !r.done);
                (msg, transient)
            }
        };
        room.send(text(msg), Some(id), transient);
    }

    /// Envoie un événement à tous les clients connectés à un workflow.
    pub fn publish(&self, workflow: Uuid, event: serde_json::Value) {
        if let Some(room) = self.rooms().get_mut(&workflow) {
            room.send(text(event), None, false);
        }
    }

    /// Nouvelle version enregistrée d'un workflow.
    pub fn saved(&self, workflow: Uuid, by: Uuid, doc: &impl Serialize) {
        self.publish(workflow, json!({ "t": "saved", "by": by, "doc": doc }));
    }

    /// Workflow supprimé : tous ses clients sont déconnectés.
    pub fn deleted(&self, workflow: Uuid) {
        if let Some(room) = self.rooms().remove(&workflow) {
            for tx in room.peers.values().filter_map(|p| p.tx.as_ref()) {
                let _ = tx.try_send(Out::Close(close::GONE, "Workflow supprimé"));
            }
        }
    }

    /// Retire l'accès d'un utilisateur (ou de tous si `None`) aux workflows d'un workspace.
    pub fn revoke(&self, workspace: Uuid, user: Option<Uuid>) {
        let mut rooms = self.rooms();
        for room in rooms.values_mut().filter(|r| r.workspace == workspace) {
            let gone: Vec<u64> = room
                .peers
                .iter()
                .filter(|(_, p)| user.is_none_or(|u| u == p.user))
                .map(|(id, _)| *id)
                .collect();
            for id in gone {
                if let Some(peer) = room.peers.remove(&id) {
                    if let Some(tx) = &peer.tx {
                        let _ = tx.try_send(Out::Close(close::FORBIDDEN, "Accès retiré"));
                    }
                    room.send(text(json!({ "t": "leave", "id": id })), None, false);
                }
            }
        }
        rooms.retain(|_, r| !r.peers.is_empty());
    }

    /// Nombre de clients connectés à un workflow.
    pub fn count(&self, workflow: Uuid) -> usize {
        self.rooms().get(&workflow).map_or(0, |r| r.peers.len())
    }
}

/// `GET /api/workspaces/{ws}/workflows/{id}/live` : ouverture du WebSocket.
pub async fn connect(
    State(state): State<AppState>,
    user: AuthUser,
    Path((ws, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> AppResult<Response> {
    // Le navigateur envoie toujours Origin à l'ouverture d'un WebSocket, mais n'applique
    // pas la politique de même origine : sans ce contrôle, n'importe quel site pourrait
    // ouvrir la connexion avec le cookie du visiteur.
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    if !origin.is_some_and(|o| state.config.public_origins.iter().any(|a| a == o)) {
        tracing::warn!(?origin, "WebSocket : origine refusée");
        return Err(AppError::Forbidden("Origine non autorisée".into()));
    }
    state.limits.live.limit(&user.id.to_string())?;
    require_member(&state.db, ws, user.id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workflows WHERE id = $1 AND workspace_id = $2)",
    )
    .bind(id)
    .bind(ws)
    .fetch_one(&state.db)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    Ok(upgrade
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| session(state, socket, ws, id, user)))
}

/// Accès toujours valide : session non expirée et membre du workspace.
async fn still_allowed(state: &AppState, user: &AuthUser, ws: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sessions s
            JOIN workspace_members m ON m.user_id = s.user_id AND m.workspace_id = $2
            WHERE s.token_hash = $1 AND s.expires_at > now())",
    )
    .bind(user.session_hash())
    .bind(ws)
    .fetch_one(&state.db)
    .await
}

/// Seau à jetons local à une connexion.
struct Budget {
    tokens: f64,
    at: Instant,
}

impl Budget {
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.at).as_secs_f64() * MESSAGES_PER_SEC)
            .min(MESSAGES_PER_SEC * 2.0);
        self.at = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

async fn session(state: AppState, mut socket: WebSocket, ws: Uuid, workflow: Uuid, user: AuthUser) {
    let hub = state.live.clone();
    let (tx, mut rx) = mpsc::channel(QUEUE);
    let Some(id) = hub.join(ws, workflow, &user, tx) else {
        let _ = socket
            .send(close_frame(
                close::FULL,
                "Trop de participants sur ce workflow",
            ))
            .await;
        return;
    };
    tracing::debug!(workflow_id = %workflow, user_id = %user.id, peer = id, "collaboration : connexion");

    let mut ping = tokio::time::interval(PING_EVERY);
    let mut recheck =
        tokio::time::interval_at(tokio::time::Instant::now() + RECHECK_EVERY, RECHECK_EVERY);
    let mut last_seen = Instant::now();
    let mut budget = Budget {
        tokens: MESSAGES_PER_SEC * 2.0,
        at: Instant::now(),
    };
    let mut ignored = 0u32;

    let reason = loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(msg))) => {
                    last_seen = Instant::now();
                    if !budget.take() {
                        ignored += 1;
                        if ignored > 200 {
                            break Some((close::RETRY, "Trop de messages"));
                        }
                        continue;
                    }
                    match Action::parse(msg.as_str()) {
                        Some(action) => hub.act(workflow, id, action),
                        None => tracing::debug!(peer = id, "collaboration : message invalide ignoré"),
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => last_seen = Instant::now(),
                Some(Ok(Message::Binary(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => break None,
            },
            out = rx.recv() => match out {
                Some(Out::Text(msg)) => {
                    if socket.send(Message::Text(msg)).await.is_err() {
                        break None;
                    }
                }
                Some(Out::Close(code, why)) => break Some((code, why)),
                // Retiré de la salle (file pleine).
                None => break Some((close::RETRY, "Connexion trop lente")),
            },
            _ = ping.tick() => {
                if last_seen.elapsed() > IDLE_TIMEOUT {
                    break None;
                }
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break None;
                }
            }
            _ = recheck.tick() => match still_allowed(&state, &user, ws).await {
                Ok(true) => {}
                Ok(false) => break Some((close::FORBIDDEN, "Accès retiré")),
                Err(e) => tracing::warn!(%e, "collaboration : vérification de l'accès"),
            },
        }
    };
    hub.leave(workflow, id);
    if let Some((code, why)) = reason {
        let _ = socket.send(close_frame(code, why)).await;
    }
    tracing::debug!(workflow_id = %workflow, peer = id, "collaboration : déconnexion");
}

fn close_frame(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(CloseFrame {
        code,
        reason: Utf8Bytes::from_static(reason),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_validated() {
        assert!(Action::parse(r#"{"t":"cursor","at":{"x":1.5,"y":-3}}"#).is_some());
        assert!(Action::parse(r#"{"t":"cursor","at":null}"#).is_some());
        assert!(Action::parse(r#"{"t":"cursor","at":{"x":1e12,"y":0}}"#).is_none());
        assert!(Action::parse(r#"{"t":"select","nodes":["csv-1a2b"]}"#).is_some());
        assert!(Action::parse(r#"{"t":"select","nodes":["<img>"]}"#).is_none());
        assert!(Action::parse(r#"{"t":"focus","node":null}"#).is_some());
        assert!(Action::parse(r#"{"t":"drag","positions":{"a":{"x":0,"y":0}}}"#).is_some());
        assert!(Action::parse(r#"{"t":"drag","positions":{"../a":{"x":0,"y":0}}}"#).is_none());
        // Un client ne choisit ni son identité ni le type d'événement diffusé.
        assert!(Action::parse(r#"{"t":"saved","doc":{}}"#).is_none());
        assert!(Action::parse("pas du json").is_none());
        let run = r#"{"t":"run","run":{"pending":["b"],"active":"b","results":{"a":{"status":"success","rows":4,"columns":[{"name":"prix","type":"Float64"}],"durationMs":1.5,"bytes":900}}}}"#;
        assert!(Action::parse(run).is_some());
        assert!(Action::parse(r#"{"t":"run","run":null}"#).is_some());
        assert!(Action::parse(&run.replace("success", "hacked")).is_none());
        assert!(Action::parse(&run.replace(r#""active":"b""#, r#""active":"<b>""#)).is_none());
    }

    fn user(name: &str) -> AuthUser {
        AuthUser::for_tests(Uuid::new_v4(), name)
    }

    fn drain(rx: &mut mpsc::Receiver<Out>) -> Vec<serde_json::Value> {
        let mut out = vec![];
        while let Ok(msg) = rx.try_recv() {
            if let Out::Text(t) = msg {
                out.push(serde_json::from_str(t.as_str()).unwrap());
            }
        }
        out
    }

    #[test]
    fn presence_is_relayed_with_sender_identity() {
        let hub = Hub::default();
        let (ws, wf) = (Uuid::new_v4(), Uuid::new_v4());
        let (ta, mut ra) = mpsc::channel(QUEUE);
        let (tb, mut rb) = mpsc::channel(QUEUE);
        let a = hub.join(ws, wf, &user("Alice"), ta).unwrap();
        hub.act(
            wf,
            a,
            Action::parse(r#"{"t":"select","nodes":["n1"]}"#).unwrap(),
        );
        let b = hub.join(ws, wf, &user("Bob"), tb).unwrap();

        // Bob reçoit l'état d'Alice à son arrivée, avec une autre couleur.
        let welcome = &drain(&mut rb)[0];
        assert_eq!(welcome["you"], b);
        assert_eq!(welcome["peers"][0]["name"], "Alice");
        assert_eq!(welcome["peers"][0]["selection"][0], "n1");
        assert_ne!(welcome["color"], welcome["peers"][0]["color"]);
        assert_eq!(drain(&mut ra).last().unwrap()["t"], "join");

        hub.act(
            wf,
            b,
            Action::parse(r#"{"t":"cursor","at":{"x":10,"y":20}}"#).unwrap(),
        );
        let got = drain(&mut ra);
        assert_eq!(got[0]["id"], b);
        assert_eq!(got[0]["at"]["x"], 10.0);
        assert!(drain(&mut rb).is_empty(), "pas d'écho à l'émetteur");

        hub.saved(wf, Uuid::new_v4(), &json!({ "version": 2 }));
        assert_eq!(drain(&mut ra)[0]["doc"]["version"], 2);
        assert_eq!(drain(&mut rb)[0]["t"], "saved");

        hub.leave(wf, b);
        assert_eq!(drain(&mut ra)[0]["t"], "leave");
        hub.revoke(ws, None);
        assert!(matches!(ra.try_recv(), Ok(Out::Close(close::FORBIDDEN, _))));
        assert_eq!(hub.count(wf), 0);
    }
}
