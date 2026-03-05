use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{AppState, db::Deployment, error::AppError, services::builder};

#[derive(Deserialize)]
pub struct CreateDeployRequest {
    pub repo: String,
    pub branch: Option<String>,
    pub name: Option<String>,
    pub ttl: Option<String>,
}

#[derive(Serialize)]
pub struct DeployResponse {
    pub name: String,
    pub url: String,
    pub status: String,
}

pub async fn create_deployment(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDeployRequest>,
) -> Result<Json<DeployResponse>, AppError> {
    let branch = req.branch.unwrap_or_else(|| "main".into());
    let name = req.name.unwrap_or_else(|| {
        sanitize_name(&format!("{}-{}", repo_short_name(&req.repo), &branch))
    });

    // Check limits
    let count = state.db.count_active_deployments()?;
    if count >= state.config.max_deployments {
        return Err(AppError::LimitReached(format!(
            "max {0} deployments reached", state.config.max_deployments
        )));
    }

    // Check for existing
    if state.db.get_deployment(&name)?.is_some() {
        return Err(AppError::Conflict(format!("deployment '{name}' already exists")));
    }

    let url = format!("https://{}.{}", name, state.config.domain);
    let ttl_secs = req.ttl.as_deref().map(parse_ttl).unwrap_or(state.config.default_ttl_secs);
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(ttl_secs as i64);

    let deployment = Deployment {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.clone(),
        repo: req.repo.clone(),
        branch: branch.clone(),
        container_id: None,
        port: None,
        status: "building".into(),
        url: url.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        expires_at: Some(expires_at.to_rfc3339()),
    };

    state.db.insert_deployment(&deployment)?;

    // Spawn build + deploy in background
    let state_clone = state.clone();
    let name_clone = name.clone();
    let repo_clone = req.repo.clone();
    let branch_clone = branch.clone();

    tokio::spawn(async move {
        match do_build_and_deploy(&state_clone, &name_clone, &repo_clone, &branch_clone).await {
            Ok(_) => tracing::info!("deployment '{name_clone}' is live"),
            Err(e) => {
                tracing::error!("deployment '{name_clone}' failed: {e}");
                state_clone.db.update_deployment_status(&name_clone, "failed", None, None).ok();
            }
        }
    });

    Ok(Json(DeployResponse {
        name,
        url,
        status: "building".into(),
    }))
}

async fn do_build_and_deploy(
    state: &AppState,
    name: &str,
    repo: &str,
    branch: &str,
) -> Result<(), AppError> {
    // Clone and build
    let (image_tag, container_port) = builder::clone_and_build(repo, branch, name).await?;

    // Allocate a host port (simple: 32000 + hash of name)
    let port = allocate_port(name);

    // Run container
    let container_id = state.docker.run_container(
        name,
        &image_tag,
        port,
        container_port,
        state.config.max_memory_mb,
        state.config.max_cpus,
    ).await?;

    // Register proxy route
    state.proxy.add_route(name, &state.config.domain, port).await?;

    // Update DB
    state.db.update_deployment_status(name, "running", Some(&container_id), Some(port))?;

    Ok(())
}

pub async fn delete_deployment(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deployment = state.db.get_deployment(&name)?
        .ok_or_else(|| AppError::NotFound(format!("deployment '{name}' not found")))?;

    if let Some(ref container_id) = deployment.container_id {
        state.docker.stop_container(container_id).await?;
    }

    state.proxy.remove_route(&name).await.ok();
    state.db.delete_deployment(&name)?;

    Ok(Json(serde_json::json!({ "deleted": name })))
}

pub async fn list_deployments(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Deployment>>, AppError> {
    Ok(Json(state.db.list_deployments()?))
}

pub async fn get_deployment(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Deployment>, AppError> {
    state.db.get_deployment(&name)?
        .map(Json)
        .ok_or_else(|| AppError::NotFound(format!("deployment '{name}' not found")))
}

pub async fn get_deployment_logs(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<Vec<String>>, AppError> {
    let deployment = state.db.get_deployment(&name)?
        .ok_or_else(|| AppError::NotFound(format!("deployment '{name}' not found")))?;

    let container_id = deployment.container_id
        .ok_or_else(|| AppError::BadRequest("no container for this deployment".into()))?;

    let logs = state.docker.get_logs(&container_id, 100).await?;
    Ok(Json(logs))
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn repo_short_name(repo: &str) -> String {
    repo.rsplit('/')
        .next()
        .unwrap_or("app")
        .trim_end_matches(".git")
        .to_string()
}

fn allocate_port(name: &str) -> u16 {
    let hash: u32 = name.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    32000 + (hash % 1000) as u16
}

fn parse_ttl(s: &str) -> u64 {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        h.parse::<u64>().unwrap_or(48) * 3600
    } else if let Some(d) = s.strip_suffix('d') {
        d.parse::<u64>().unwrap_or(2) * 86400
    } else {
        s.parse::<u64>().unwrap_or(172800)
    }
}
