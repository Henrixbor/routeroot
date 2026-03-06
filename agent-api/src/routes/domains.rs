use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{AppState, db::CustomDomain, error::AppError};

#[derive(Deserialize)]
pub struct MapDomainRequest {
    pub domain: String,
    pub deployment_name: String,
}

#[derive(Serialize)]
pub struct DomainResponse {
    pub domain: String,
    pub deployment_name: String,
    pub instructions: String,
}

pub async fn map_custom_domain(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MapDomainRequest>,
) -> Result<Json<DomainResponse>, AppError> {
    // Verify deployment exists and is running
    let deployment = state.db.get_deployment(&req.deployment_name)?
        .ok_or_else(|| AppError::NotFound(format!("deployment '{}' not found", req.deployment_name)))?;

    if deployment.status != "running" {
        return Err(AppError::BadRequest(format!(
            "deployment must be running, got '{}'", deployment.status
        )));
    }

    let port = deployment.port
        .ok_or_else(|| AppError::BadRequest("deployment has no port assigned".into()))?;

    // Check if domain is already mapped
    if state.db.is_custom_domain(&req.domain)? {
        return Err(AppError::Conflict(format!("domain '{}' is already mapped", req.domain)));
    }

    // Store mapping
    let custom = CustomDomain {
        id: uuid::Uuid::new_v4().to_string(),
        domain: req.domain.clone(),
        deployment_name: req.deployment_name.clone(),
        verified: false,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.insert_custom_domain(&custom)?;

    // Add Caddy route for the custom domain
    state.proxy.add_custom_domain_route(&req.domain, port).await?;

    let instructions = format!(
        "Add a CNAME record at your DNS provider: {} -> {}.{}",
        req.domain, req.deployment_name, state.config.domain
    );

    audit(&state, "custom_domain_mapped", "domain", &req.domain, &serde_json::json!({
        "deployment": req.deployment_name
    }));

    tracing::info!(
        domain = %req.domain,
        deployment = %req.deployment_name,
        "custom domain mapped"
    );

    Ok(Json(DomainResponse {
        domain: req.domain,
        deployment_name: req.deployment_name,
        instructions,
    }))
}

pub async fn list_custom_domains(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<CustomDomain>>, AppError> {
    Ok(Json(state.db.list_custom_domains()?))
}

pub async fn delete_custom_domain(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.db.is_custom_domain(&domain)? {
        return Err(AppError::NotFound(format!("custom domain '{}' not found", domain)));
    }

    state.proxy.remove_custom_domain_route(&domain).await.ok();
    state.db.delete_custom_domain(&domain)?;

    audit(&state, "custom_domain_removed", "domain", &domain, &serde_json::json!({}));

    Ok(Json(serde_json::json!({ "deleted": domain })))
}

fn audit(state: &AppState, action: &str, resource_type: &str, resource_name: &str, details: &serde_json::Value) {
    let event = crate::db::AuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_name: resource_name.to_string(),
        actor: "api".to_string(),
        details: details.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.insert_audit(&event).ok();
}
