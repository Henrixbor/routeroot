use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{AppState, error::AppError};

#[derive(Deserialize)]
pub struct AddDomainRequest {
    pub domain: String,
}

#[derive(Serialize)]
pub struct ManagedDomainResponse {
    pub domain: String,
    pub server_ip: String,
    pub created_at: String,
    pub status: String,
    pub instructions: Option<String>,
}

/// Add a new managed domain dynamically.
/// Creates zone file, updates Corefile, restarts CoreDNS, adds Caddy routes + TLS.
pub async fn add_managed_domain(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AddDomainRequest>,
) -> Result<Json<ManagedDomainResponse>, AppError> {
    let domain = req.domain.trim().to_lowercase();

    // Validate domain format
    if domain.is_empty() || domain.len() > 253
        || !domain.contains('.')
        || domain.chars().any(|c| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
    {
        return Err(AppError::BadRequest("invalid domain format".into()));
    }

    // Check if already managed
    if state.is_domain_managed(&domain) {
        return Err(AppError::Conflict(format!("domain '{domain}' is already managed")));
    }

    let server_ip = state.config.server_ip.clone();

    // 1. Store in DB
    state.db.insert_managed_domain(&domain, &server_ip)?;

    // 2. Write zone file
    state.dns.write_zone_for_new_domain(&domain, &server_ip)?;

    // 3. Regenerate Corefile with all domains
    let all_domains = state.all_domains();
    state.dns.write_corefile(&all_domains)?;

    // 4. Restart CoreDNS to pick up new zone block
    // The container name follows docker-compose naming: routeroot-coredns-1
    // Try common naming patterns
    let coredns_restarted = restart_coredns(&state).await;
    if !coredns_restarted {
        tracing::warn!("Could not restart CoreDNS container — new domain DNS may not work until manual restart");
    }

    // 5. Add Caddy routes + TLS policy
    let tls_check_url = "http://agent-api:8053/api/tls-check";
    if let Err(e) = state.proxy.add_domain(&domain, tls_check_url).await {
        tracing::warn!("Failed to add Caddy config for {domain}: {e}");
        // Don't fail — DNS is set up, Caddy can be fixed with a restart
    }

    let instructions = format!(
        "Domain '{domain}' added. Configure DNS at your registrar:\n\
        1. Set nameservers: ns1.{domain}, ns2.{domain}\n\
        2. Create glue records: ns1.{domain} -> {server_ip}, ns2.{domain} -> {server_ip}"
    );

    audit(&state, "domain_added", "managed_domain", &domain, &serde_json::json!({ "server_ip": server_ip }));

    Ok(Json(ManagedDomainResponse {
        domain,
        server_ip,
        created_at: chrono::Utc::now().to_rfc3339(),
        status: if coredns_restarted { "active".into() } else { "pending_restart".into() },
        instructions: Some(instructions),
    }))
}

/// List all managed domains (config + dynamically added).
pub async fn list_managed_domains(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ManagedDomainResponse>>, AppError> {
    let mut result = Vec::new();

    // Config domains (always present)
    for domain in &state.config.domains {
        result.push(ManagedDomainResponse {
            domain: domain.clone(),
            server_ip: state.config.server_ip.clone(),
            created_at: "config".into(),
            status: "active".into(),
            instructions: None,
        });
    }

    // DB domains (dynamically added)
    for (domain, server_ip, created_at) in state.db.list_managed_domains()? {
        if !state.config.domains.contains(&domain) {
            result.push(ManagedDomainResponse {
                domain,
                server_ip,
                created_at,
                status: "active".into(),
                instructions: None,
            });
        }
    }

    Ok(Json(result))
}

/// Remove a dynamically added domain. Config domains cannot be removed.
pub async fn remove_managed_domain(
    State(state): State<Arc<AppState>>,
    Path(domain): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Can't remove config domains
    if state.config.is_managed_domain(&domain) {
        return Err(AppError::BadRequest(format!(
            "'{domain}' is a config domain (set via ROUTEROOT_DOMAINS). Remove it from .env instead."
        )));
    }

    // Check it exists in DB
    if !state.db.is_managed_domain_in_db(&domain)? {
        return Err(AppError::NotFound(format!("domain '{domain}' not found")));
    }

    // 1. Remove from DB
    state.db.delete_managed_domain(&domain)?;

    // 2. Remove zone file
    state.dns.remove_zone_file(&domain)?;

    // 3. Regenerate Corefile
    let all_domains = state.all_domains();
    state.dns.write_corefile(&all_domains)?;

    // 4. Restart CoreDNS
    restart_coredns(&state).await;

    // 5. Remove Caddy routes
    state.proxy.remove_domain(&domain).await?;

    audit(&state, "domain_removed", "managed_domain", &domain, &serde_json::json!({}));

    Ok(Json(serde_json::json!({ "deleted": domain })))
}

/// Try to restart the CoreDNS container. Returns true if successful.
async fn restart_coredns(state: &AppState) -> bool {
    // Try common container names
    for name in &["routeroot-coredns-1", "routeroot_coredns_1", "coredns"] {
        if state.docker.restart_container_by_name(name).await.is_ok() {
            tracing::info!("Restarted CoreDNS container ({name})");
            return true;
        }
    }
    false
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
