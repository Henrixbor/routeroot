use axum::{Json, extract::{Path, State}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{AppState, db::{AuditEvent, Deployment, DeployPlan}, error::AppError, services::{builder, verify}};

#[derive(Deserialize)]
pub struct CreateDeployRequest {
    pub repo: String,
    pub branch: Option<String>,
    pub name: Option<String>,
    pub ttl: Option<String>,
    pub environment: Option<String>,
}

#[derive(Serialize)]
pub struct DeployResponse {
    pub name: String,
    pub url: String,
    pub status: String,
    pub environment: String,
}

#[derive(Deserialize)]
pub struct PromoteRequest {
    pub target: String, // "staging" or "production"
}

// --- Plan/Apply ---

pub async fn create_plan(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDeployRequest>,
) -> Result<Json<DeployPlan>, AppError> {
    let branch = req.branch.unwrap_or_else(|| "main".into());
    let environment = req.environment.as_deref().unwrap_or("preview");
    let name = req.name.unwrap_or_else(|| {
        sanitize_name(&format!("{}-{}", repo_short_name(&req.repo), &branch))
    });

    // Check limits
    let count = state.db.count_active_deployments()?;
    if count >= state.config.max_deployments {
        return Err(AppError::LimitReached(format!(
            "max {} deployments reached", state.config.max_deployments
        )));
    }

    if state.db.get_deployment(&name)?.is_some() {
        return Err(AppError::Conflict(format!("deployment '{name}' already exists")));
    }

    let url = format!("https://{}.{}", name, state.config.domain);
    let port = allocate_port(&name);

    let actions = serde_json::json!([
        {"action": "clone_repo", "repo": req.repo, "branch": branch},
        {"action": "build_image", "tag": format!("agentdns-{name}:latest")},
        {"action": "create_container", "name": name, "port": port, "memory_mb": state.config.max_memory_mb, "cpus": state.config.max_cpus},
        {"action": "add_proxy_route", "subdomain": name, "target_port": port},
        {"action": "verify_deployment", "url": url, "dns_check": true, "http_check": true},
    ]);

    let plan = DeployPlan {
        id: uuid::Uuid::new_v4().to_string(),
        repo: req.repo,
        branch,
        name,
        environment: environment.to_string(),
        url,
        ttl: req.ttl,
        actions: actions.to_string(),
        status: "pending".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state.db.insert_plan(&plan)?;
    audit(&state, "plan_created", "plan", &plan.id, &serde_json::json!({"name": plan.name, "repo": plan.repo}));

    Ok(Json(plan))
}

pub async fn apply_plan(
    State(state): State<Arc<AppState>>,
    Path(plan_id): Path<String>,
) -> Result<Json<DeployResponse>, AppError> {
    let plan = state.db.get_plan(&plan_id)?
        .ok_or_else(|| AppError::NotFound(format!("plan '{plan_id}' not found")))?;

    if plan.status != "pending" {
        return Err(AppError::BadRequest(format!("plan is already '{}'", plan.status)));
    }

    state.db.update_plan_status(&plan_id, "applied")?;

    // Execute the deploy using the plan details
    let req = CreateDeployRequest {
        repo: plan.repo,
        branch: Some(plan.branch),
        name: Some(plan.name),
        ttl: plan.ttl,
        environment: Some(plan.environment),
    };

    audit(&state, "plan_applied", "plan", &plan_id, &serde_json::json!({}));

    create_deployment(State(state), Json(req)).await
}

pub async fn list_plans(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DeployPlan>>, AppError> {
    Ok(Json(state.db.list_plans()?))
}

// --- Deploy ---

pub async fn create_deployment(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateDeployRequest>,
) -> Result<Json<DeployResponse>, AppError> {
    let branch = req.branch.unwrap_or_else(|| "main".into());
    let environment = req.environment.unwrap_or_else(|| "preview".into());
    let name = req.name.unwrap_or_else(|| {
        sanitize_name(&format!("{}-{}", repo_short_name(&req.repo), &branch))
    });

    // Check limits
    let count = state.db.count_active_deployments()?;
    if count >= state.config.max_deployments {
        return Err(AppError::LimitReached(format!(
            "max {} deployments reached", state.config.max_deployments
        )));
    }

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
        verified: None,
        environment: environment.clone(),
        url: url.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        expires_at: Some(expires_at.to_rfc3339()),
    };

    state.db.insert_deployment(&deployment)?;
    tracing::info!(
        name = %name,
        repo = %req.repo,
        branch = %branch,
        environment = %environment,
        url = %url,
        ttl_secs = ttl_secs,
        "deployment created"
    );
    audit(&state, "deploy_started", "deployment", &name, &serde_json::json!({
        "repo": req.repo, "branch": branch, "environment": environment
    }));

    // Spawn build + deploy + verify in background
    let state_clone = state.clone();
    let name_clone = name.clone();
    let repo_clone = req.repo.clone();
    let branch_clone = branch.clone();

    tokio::spawn(async move {
        match do_build_and_deploy(&state_clone, &name_clone, &repo_clone, &branch_clone).await {
            Ok(_) => {
                tracing::info!("deployment '{name_clone}' is live, starting verification");
                audit(&state_clone, "deploy_live", "deployment", &name_clone, &serde_json::json!({}));
                // Start verification
                verify::verify_deployment(state_clone, name_clone).await;
            }
            Err(e) => {
                tracing::error!("deployment '{name_clone}' failed: {e}");
                state_clone.db.update_deployment_status(&name_clone, "failed", None, None).ok();
                audit(&state_clone, "deploy_failed", "deployment", &name_clone, &serde_json::json!({"error": e.to_string()}));
            }
        }
    });

    Ok(Json(DeployResponse {
        name,
        url,
        status: "building".into(),
        environment,
    }))
}

async fn do_build_and_deploy(
    state: &AppState,
    name: &str,
    repo: &str,
    branch: &str,
) -> Result<(), AppError> {
    tracing::info!(name = %name, phase = "clone_build", "starting clone and build");
    let (image_tag, container_port) = builder::clone_and_build(repo, branch, name).await?;
    let port = allocate_port(name);

    tracing::info!(name = %name, phase = "container", image = %image_tag, host_port = port, container_port = container_port, "starting container");
    let container_id = state.docker.run_container(
        name,
        &image_tag,
        port,
        container_port,
        state.config.max_memory_mb,
        state.config.max_cpus,
    ).await?;

    tracing::info!(name = %name, phase = "proxy", "registering proxy route");
    state.proxy.add_route(name, &state.config.domain, port).await?;
    state.db.update_deployment_status(name, "running", Some(&container_id), Some(port))?;

    tracing::info!(name = %name, phase = "complete", container_id = %container_id, host_port = port, "deployment live");
    Ok(())
}

// --- Promote ---

pub async fn promote_deployment(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<PromoteRequest>,
) -> Result<Json<DeployResponse>, AppError> {
    let target = req.target.to_lowercase();
    if target != "staging" && target != "production" {
        return Err(AppError::BadRequest("target must be 'staging' or 'production'".into()));
    }

    let deployment = state.db.get_deployment(&name)?
        .ok_or_else(|| AppError::NotFound(format!("deployment '{name}' not found")))?;

    if deployment.status != "running" {
        return Err(AppError::BadRequest(format!("deployment must be running, got '{}'", deployment.status)));
    }

    // Update environment
    let new_url = format!("https://{}.{}", name, state.config.domain);
    state.db.update_deployment_environment(&name, &target, &new_url)?;

    // Remove TTL for promoted deployments (don't auto-expire production)
    if target == "production" {
        state.db.clear_deployment_expiry(&name).ok();
    }

    audit(&state, "deploy_promoted", "deployment", &name, &serde_json::json!({
        "from": deployment.environment, "to": target
    }));

    Ok(Json(DeployResponse {
        name,
        url: new_url,
        status: deployment.status,
        environment: target,
    }))
}

// --- CRUD ---

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
    audit(&state, "deploy_deleted", "deployment", &name, &serde_json::json!({}));

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

// --- Audit ---

pub async fn list_audit(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<crate::db::AuditEvent>>, AppError> {
    Ok(Json(state.db.list_audit(100)?))
}

fn audit(state: &AppState, action: &str, resource_type: &str, resource_name: &str, details: &serde_json::Value) {
    let event = AuditEvent {
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

// --- Helpers ---

pub(crate) fn sanitize_name(s: &str) -> String {
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
