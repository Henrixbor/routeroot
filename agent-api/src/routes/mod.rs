pub mod deploy;
pub mod health;
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
        .route("/deploy", post(deploy::create_deployment))
        .route("/deploy/{name}", delete(deploy::delete_deployment))
        .route("/deployments", get(deploy::list_deployments))
        .route("/deployments/{name}", get(deploy::get_deployment))
        .route("/deployments/{name}/logs", get(deploy::get_deployment_logs))
        .route("/records", post(records::create_record))
        .route("/records", get(records::list_records))
        .route("/records/{name}", delete(records::delete_record))
        .route("/webhook/github", post(webhook::github_webhook))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_api_key))
        .with_state(state);

    public.merge(protected)
}
