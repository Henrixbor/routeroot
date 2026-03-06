mod auth;
mod config;
mod db;
mod error;
mod routes;
mod services;

use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;
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
    // JSON structured logging for agent-parseable output
    let json_logging = std::env::var("ROUTEROOT_LOG_FORMAT").unwrap_or_default() == "json";

    if json_logging {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env().add_directive("agent_api=info".parse()?))
            .with_target(true)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env().add_directive("agent_api=info".parse()?))
            .init();
    }

    let config = config::Config::from_env();
    tracing::info!(
        domain = %config.domain,
        server_ip = %config.server_ip,
        max_deployments = config.max_deployments,
        max_memory_mb = config.max_memory_mb,
        "RouteRoot starting"
    );

    let db = db::Database::new(&config.database_path)?;
    db.migrate()?;

    let docker = services::docker::DockerService::new()?;
    let proxy = services::proxy::ProxyService::new(&config.caddy_admin_url);
    let dns = services::dns::DnsService::new(&config.zone_file_dir, &config.domains, &config.server_ip);

    let state = Arc::new(AppState { config, db, docker, proxy, dns });

    // Start cleanup task
    let cleanup_state = state.clone();
    tokio::spawn(async move {
        services::cleanup::run_cleanup_loop(cleanup_state).await;
    });

    // Replace Caddy's Caddyfile config with clean JSON config at startup
    tokio::spawn({
        let proxy = services::proxy::ProxyService::new(&state.config.caddy_admin_url);
        let domains = state.config.domains.clone();
        async move {
            // Wait for Caddy to be ready
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let tls_check_url = "http://agent-api:8053/api/tls-check";
            if let Err(e) = proxy.init_caddy_config(&domains, tls_check_url).await {
                tracing::warn!("Failed to initialize Caddy config: {e}");
            }
        }
    });

    let app = Router::new()
        .nest("/api", routes::api_router(state.clone()))
        .layer(TraceLayer::new_for_http())
        .layer({
            use tower_http::cors::{AllowOrigin, AllowHeaders, AllowMethods};
            use axum::http::{Method, HeaderName};
            let origins: Vec<_> = state.config.domains.iter()
                .flat_map(|d| [
                    format!("https://{d}").parse().ok(),
                    format!("https://api.{d}").parse().ok(),
                ])
                .flatten()
                .collect();
            tower_http::cors::CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods(AllowMethods::list([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS]))
                .allow_headers(AllowHeaders::list([
                    HeaderName::from_static("authorization"),
                    HeaderName::from_static("content-type"),
                ]))
        });

    let addr = "0.0.0.0:8053";
    tracing::info!("RouteRoot API listening on {addr}");
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
