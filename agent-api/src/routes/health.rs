use axum::{Json, extract::{Query, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub domain: String,
    pub active_deployments: usize,
}

pub async fn health(
    State(state): State<Arc<AppState>>,
) -> Json<HealthResponse> {
    let count = state.db.count_active_deployments().unwrap_or(0);
    Json(HealthResponse {
        status: "ok".into(),
        domain: state.config.domain.clone(),
        active_deployments: count,
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

    // Allow any subdomain of our domain
    if domain.ends_with(&format!(".{}", state.config.domain)) {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::NOT_FOUND
    }
}
