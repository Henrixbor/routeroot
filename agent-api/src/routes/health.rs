use axum::{Json, extract::{Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub domain: String,
    pub server_ip: String,
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
        server_ip: state.config.server_ip.clone(),
        active_deployments: count,
        max_deployments: state.config.max_deployments,
        features: vec![
            "deploy".into(), "plan_apply".into(), "promote".into(),
            "dns_records".into(), "audit_log".into(), "verification".into(),
            "github_webhook".into(), "mcp_server".into(),
            "auto_detect_node".into(), "auto_detect_rust".into(),
            "auto_detect_go".into(), "auto_detect_python".into(),
            "auto_detect_static".into(),
        ],
    })
}

#[derive(Deserialize)]
pub struct TlsCheckQuery {
    pub domain: Option<String>,
}

/// Called by Caddy's on_demand_tls to verify a subdomain is valid before issuing a cert.
pub async fn tls_check(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TlsCheckQuery>,
) -> axum::http::StatusCode {
    let Some(domain) = q.domain else {
        return axum::http::StatusCode::BAD_REQUEST;
    };

    if domain.ends_with(&format!(".{}", state.config.domain)) {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::NOT_FOUND
    }
}
