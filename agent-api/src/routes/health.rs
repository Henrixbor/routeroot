use axum::{Json, extract::{Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub domain: String,
    pub domains: Vec<String>,
    pub active_deployments: usize,
    pub max_deployments: usize,
    pub features: Vec<String>,
}

pub async fn health(
    State(state): State<Arc<AppState>>,
) -> Json<HealthResponse> {
    let count = state.db.count_active_deployments().unwrap_or(0);
    Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        domain: state.config.domain.clone(),
        domains: state.config.domains.clone(),
        active_deployments: count,
        max_deployments: state.config.max_deployments,
        features: vec![
            "deploy".into(), "plan_apply".into(), "promote".into(),
            "dns_records".into(), "audit_log".into(), "verification".into(),
            "github_webhook".into(), "mcp_server".into(),
            "auto_detect_node".into(), "auto_detect_rust".into(),
            "auto_detect_go".into(), "auto_detect_python".into(),
            "auto_detect_static".into(), "multi_domain".into(),
        ],
    })
}

#[derive(Deserialize)]
pub struct TlsCheckQuery {
    pub domain: Option<String>,
}

/// Called by Caddy's on_demand_tls to verify a subdomain is valid before issuing a cert.
/// Only approves certs for:
/// 1. Managed domains themselves
/// 2. Subdomains of managed domains that have an active deployment
/// 3. Custom domains mapped to active deployments
pub async fn tls_check(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TlsCheckQuery>,
) -> axum::http::StatusCode {
    let Some(domain) = q.domain else {
        return axum::http::StatusCode::BAD_REQUEST;
    };

    // Allow managed domains themselves
    if state.config.is_managed_domain(&domain) {
        return axum::http::StatusCode::OK;
    }

    // Allow subdomains only if there's a matching deployment or it's api.*
    if state.config.is_managed_subdomain(&domain) {
        // Always allow api.domain
        for d in &state.config.domains {
            if domain == format!("api.{d}") {
                return axum::http::StatusCode::OK;
            }
        }
        // Check if there's an active deployment for this subdomain
        let subdomain = domain.split('.').next().unwrap_or("");
        if let Ok(Some(deployment)) = state.db.get_deployment(subdomain) {
            if deployment.status == "running" || deployment.status == "building" {
                return axum::http::StatusCode::OK;
            }
        }
        return axum::http::StatusCode::NOT_FOUND;
    }

    // Allow verified custom domains mapped to active deployments
    if state.db.is_custom_domain(&domain).unwrap_or(false) {
        return axum::http::StatusCode::OK;
    }

    axum::http::StatusCode::NOT_FOUND
}
