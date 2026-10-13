use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use mini8n_server::{AppState, Config, MIGRATOR, app, auth, mcp, runner, scheduler};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

/// `mini8n-server healthcheck` : sonde HTTP minimale (l'image ne contient ni shell ni curl).
fn healthcheck() -> ! {
    let port = std::env::var("BIND_ADDR")
        .ok()
        .and_then(|a| a.parse::<SocketAddr>().ok())
        .map_or(3000, |a| a.port());
    let ok = (|| -> std::io::Result<bool> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut buf = [0u8; 16];
        let n = s.read(&mut buf)?;
        Ok(buf[..n].starts_with(b"HTTP/1.1 200"))
    })()
    .unwrap_or(false);
    std::process::exit(if ok { 0 } else { 1 })
}

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        healthcheck();
    }

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn"));
    if std::env::var("LOG_FORMAT").is_ok_and(|v| v == "json") {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("configuration invalide : {e}");
            std::process::exit(2);
        }
    };
    match config.cookie_secure {
        mini8n_server::config::CookieSecure::Never => {
            tracing::warn!("COOKIE_SECURE=false : à n'utiliser qu'en développement")
        }
        mini8n_server::config::CookieSecure::Auto if !config.trust_proxy => tracing::warn!(
            "COOKIE_SECURE=auto sans TRUST_PROXY : les cookies ne seront jamais Secure"
        ),
        _ => {}
    }

    // La base peut démarrer après l'API : quelques tentatives de connexion.
    let mut attempt = 0;
    let db = loop {
        match PgPoolOptions::new()
            .max_connections(config.database_max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.database_url)
            .await
        {
            Ok(pool) => break pool,
            Err(e) if attempt < 15 => {
                attempt += 1;
                tracing::warn!("base indisponible ({e}), nouvelle tentative…");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!("connexion à la base impossible : {e}");
                std::process::exit(1);
            }
        }
    };
    if let Err(e) = MIGRATOR.run(&db).await {
        tracing::error!("migrations : {e}");
        std::process::exit(1);
    }

    // Exécutions orphelines (serveur arrêté ou planté pendant l'exécution) : au démarrage,
    // puis régulièrement (une autre instance a pu tomber).
    {
        let (db, timeout) = (db.clone(), config.execution_timeout_secs);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(300));
            loop {
                tick.tick().await;
                match runner::recover_interrupted(&db, timeout).await {
                    Ok(n) if n > 0 => {
                        tracing::warn!(n, "exécutions interrompues marquées en erreur")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::error!(%e, "reprise des exécutions"),
                }
            }
        });
    }
    auth::spawn_session_cleanup(Arc::new(db.clone()));
    let retention = config.executions_retention_days;
    let bind = config.bind;
    let state = AppState::new(db, config);
    scheduler::spawn(state.runner.clone());
    mcp::spawn_sweeper(state.clone());
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                match runner::purge_old(&db, retention).await {
                    Ok(n) if n > 0 => tracing::info!(n, "anciennes exécutions supprimées"),
                    Ok(_) => {}
                    Err(e) => tracing::error!(%e, "purge des exécutions"),
                }
            }
        });
    }
    let router = app(state);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("écoute sur {bind} impossible : {e}");
            std::process::exit(1);
        });
    tracing::info!("API mini8n à l'écoute sur {bind}");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .expect("serveur HTTP");
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("arrêt demandé");
}
