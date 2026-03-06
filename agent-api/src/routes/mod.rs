pub mod deploy;
pub mod domains;
pub mod health;
pub mod managed_domains;
pub mod records;
pub mod webhook;

use axum::{Router, middleware, routing::{delete, get, post}};
use std::sync::Arc;

use crate::{AppState, auth};

pub fn api_router(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .route("/health", get(health::health))
        .route("/tls-check", get(health::tls_check))
        .with_state(state.clone());

    let protected = Router::new()
        // Deploy
        .route("/deploy", post(deploy::create_deployment))
        .route("/deploy/{name}", delete(deploy::delete_deployment))
        .route("/deploy/{name}/promote", post(deploy::promote_deployment))
        // Plan/Apply
        .route("/plan", post(deploy::create_plan))
        .route("/plan/{plan_id}/apply", post(deploy::apply_plan))
        .route("/plans", get(deploy::list_plans))
        // Deployments
        .route("/deployments", get(deploy::list_deployments))
        .route("/deployments/{name}", get(deploy::get_deployment))
        .route("/deployments/{name}/logs", get(deploy::get_deployment_logs))
        // DNS Records
        .route("/records", post(records::create_record))
        .route("/records", get(records::list_records))
        .route("/records/{name}", delete(records::delete_record))
        // Custom Domains
        .route("/domains", post(domains::map_custom_domain))
        .route("/domains", get(domains::list_custom_domains))
        .route("/domains/{domain}", delete(domains::delete_custom_domain))
        // Managed Domains
        .route("/managed-domains", post(managed_domains::add_managed_domain))
        .route("/managed-domains", get(managed_domains::list_managed_domains))
        .route("/managed-domains/{domain}", delete(managed_domains::remove_managed_domain))
        // Audit
        .route("/audit", get(deploy::list_audit))
        // Webhooks
        .route("/webhook/github", post(webhook::github_webhook))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key))
        .with_state(state);

    public.merge(protected)
}
