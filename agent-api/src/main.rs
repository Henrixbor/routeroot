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

    // Replace Caddy's Caddyfile config with clean JSON config at startup,
    // then re-register routes for all active deployments from the DB
    tokio::spawn({
        let state_clone = state.clone();
        async move {
            // Wait for Caddy to be ready
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let tls_check_url = "http://agent-api:8053/api/tls-check";
            if let Err(e) = state_clone.proxy.init_caddy_config(&state_clone.config.domains, tls_check_url).await {
                tracing::warn!("Failed to initialize Caddy config: {e}");
                return;
            }

            // Re-register proxy routes for all running deployments
            let deployments = match state_clone.db.list_deployments() {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("Failed to list deployments for route restore: {e}");
                    return;
                }
            };

            let mut restored = 0u32;
            for dep in &deployments {
                if dep.status != "running" {
                    continue;
                }
                let Some(port) = dep.port else { continue };

                // Re-add subdomain route
                if let Err(e) = state_clone.proxy.add_route(&dep.name, &state_clone.config.domain, port).await {
                    tracing::warn!(name = %dep.name, "failed to restore subdomain route: {e}");
                    continue;
                }

                // Re-add path route if the URL indicates path-based routing
                let domain_prefix = format!("https://{}/", state_clone.config.domain);
                if dep.url.starts_with(&domain_prefix) {
                    let path_prefix = dep.url.strip_prefix(&domain_prefix).unwrap_or("");
                    if !path_prefix.is_empty() {
                        if let Err(e) = state_clone.proxy.add_path_route(path_prefix, &state_clone.config.domain, port).await {
                            tracing::warn!(name = %dep.name, path = %path_prefix, "failed to restore path route: {e}");
                        }
                    }
                }

                restored += 1;
            }

            if restored > 0 {
                tracing::info!(count = restored, "restored proxy routes for active deployments");
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
