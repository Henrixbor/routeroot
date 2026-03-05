mod auth;
mod config;
mod db;
mod error;
mod routes;
mod services;

use axum::{Router, middleware};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub config: config::Config,
    pub db: db::Database,
    pub docker: services::docker::DockerService,
    pub proxy: services::proxy::ProxyService,
    pub dns: services::dns::DnsService,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("agent_api=info".parse()?))
        .init();

    let config = config::Config::from_env();
    let db = db::Database::new(&config.database_path)?;
    db.migrate()?;

    let docker = services::docker::DockerService::new()?;
    let proxy = services::proxy::ProxyService::new(&config.caddy_admin_url);
    let dns = services::dns::DnsService::new(&config.zone_file_path, &config.domain, &config.server_ip);

    let state = Arc::new(AppState { config, db, docker, proxy, dns });

    // Start cleanup task
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        services::cleanup::run_cleanup_loop(cleanup_state).await;
    });

    let app = Router::new()
        .nest("/api", routes::api_router(state.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:8053";
    tracing::info!("AgentDNS API listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
