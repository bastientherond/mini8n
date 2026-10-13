//! Supervision : `GET /api/metrics` au format texte de Prometheus.
//!
//! Protégé par un jeton dédié (`METRICS_TOKEN`, en-tête `Authorization: Bearer …`) ; sans
//! jeton configuré, l'URL répond 404. Les compteurs d'exécutions sont lus en base (ils
//! survivent aux redémarrages) ; les autres valeurs décrivent l'instance.

use std::fmt::Write;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::AppState;

/// Comparaison en temps constant (jeton).
fn same_secret(a: &str, b: &str) -> bool {
    let (ha, hb) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    ha.iter()
        .zip(hb.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn metric(out: &mut String, name: &str, kind: &str, help: &str, samples: &[(&str, f64)]) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
    for (labels, value) in samples {
        let _ = writeln!(out, "{name}{labels} {value}");
    }
}

pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(expected) = &state.config.metrics_token else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !same_secret(given, expected) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut out = String::with_capacity(4096);
    metric(
        &mut out,
        "mini8n_build_info",
        "gauge",
        "Version du serveur.",
        &[(
            &format!("{{version=\"{}\"}}", env!("CARGO_PKG_VERSION")),
            1.0,
        )],
    );

    // Base de données : joignable, et occupation du pool de connexions.
    let db_up = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    metric(
        &mut out,
        "mini8n_database_up",
        "gauge",
        "PostgreSQL répond (1) ou non (0).",
        &[("", f64::from(u8::from(db_up)))],
    );
    metric(
        &mut out,
        "mini8n_database_connections",
        "gauge",
        "Connexions du pool.",
        &[
            ("{state=\"open\"}", f64::from(state.db.size())),
            ("{state=\"idle\"}", state.db.num_idle() as f64),
        ],
    );
    metric(
        &mut out,
        "mini8n_database_connections_max",
        "gauge",
        "Taille maximale du pool (DATABASE_MAX_CONNECTIONS).",
        &[("", f64::from(state.config.database_max_connections))],
    );

    // Exécutions côté serveur.
    let (queued, capacity) = state.runner.queue();
    metric(
        &mut out,
        "mini8n_runner_queue",
        "gauge",
        "Exécutions en attente dans la file.",
        &[("", queued as f64)],
    );
    metric(
        &mut out,
        "mini8n_runner_queue_capacity",
        "gauge",
        "Capacité de la file (au-delà : refus 429).",
        &[("", capacity as f64)],
    );
    if db_up {
        let by_status: Vec<(String, i64)> = sqlx::query_as(
            "SELECT status, count(*) FROM executions WHERE started_at > now() - interval '24 hours' GROUP BY status",
        )
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        let samples: Vec<(String, f64)> = ["success", "error", "running"]
            .iter()
            .map(|s| {
                let n = by_status
                    .iter()
                    .find(|(st, _)| st == s)
                    .map_or(0, |(_, n)| *n);
                (format!("{{status=\"{s}\"}}"), n as f64)
            })
            .collect();
        let samples: Vec<(&str, f64)> = samples.iter().map(|(l, v)| (l.as_str(), *v)).collect();
        metric(
            &mut out,
            "mini8n_executions_24h",
            "gauge",
            "Exécutions démarrées depuis 24 h, par statut.",
            &samples,
        );
        let active: i64 = sqlx::query_scalar("SELECT count(*) FROM workflows WHERE active")
            .fetch_one(&state.db)
            .await
            .unwrap_or_default();
        metric(
            &mut out,
            "mini8n_workflows_active",
            "gauge",
            "Workflows actifs (déclencheurs automatiques).",
            &[("", active as f64)],
        );
        let last_error: Option<f64> = sqlx::query_scalar(
            "SELECT extract(epoch FROM max(started_at))::float8 FROM executions WHERE status = 'error'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap_or_default();
        metric(
            &mut out,
            "mini8n_last_execution_error_timestamp_seconds",
            "gauge",
            "Date de la dernière exécution en échec (epoch).",
            &[("", last_error.unwrap_or(0.0))],
        );
    }

    // Collaboration en direct.
    let (rooms, peers) = state.live.totals();
    metric(
        &mut out,
        "mini8n_live_rooms",
        "gauge",
        "Workflows ouverts en collaboration.",
        &[("", rooms as f64)],
    );
    metric(
        &mut out,
        "mini8n_live_participants",
        "gauge",
        "Participants connectés (IA comprises).",
        &[("", peers as f64)],
    );

    let mut res = (StatusCode::OK, out).into_response();
    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    res
}
