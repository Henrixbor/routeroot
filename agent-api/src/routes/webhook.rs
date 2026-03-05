use axum::{Json, extract::State, http::HeaderMap};
use serde::Deserialize;
use std::sync::Arc;

use crate::{AppState, error::AppError};

#[derive(Deserialize)]
pub struct GitHubPushEvent {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub repository: GitHubRepo,
    pub deleted: Option<bool>,
}

#[derive(Deserialize)]
pub struct GitHubRepo {
    pub clone_url: String,
    pub name: String,
}

pub async fn github_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GitHubPushEvent>,
) -> Result<Json<serde_json::Value>, AppError> {
    // TODO: verify X-Hub-Signature-256 with webhook secret

    let branch = payload.git_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&payload.git_ref)
        .to_string();

    let name = format!("{}-{}", payload.repository.name, sanitize(&branch));

    if payload.deleted.unwrap_or(false) {
        // Branch deleted — tear down
        if let Some(deployment) = state.db.get_deployment(&name)? {
            if let Some(ref cid) = deployment.container_id {
                state.docker.stop_container(cid).await.ok();
            }
            state.proxy.remove_route(&name).await.ok();
            state.db.delete_deployment(&name)?;
            tracing::info!("webhook: torn down deployment '{name}' (branch deleted)");
        }
        return Ok(Json(serde_json::json!({ "action": "deleted", "name": name })));
    }

    // Branch pushed — deploy or redeploy
    // If exists, tear down first
    if let Some(deployment) = state.db.get_deployment(&name)? {
        if let Some(ref cid) = deployment.container_id {
            state.docker.stop_container(cid).await.ok();
        }
        state.proxy.remove_route(&name).await.ok();
        state.db.delete_deployment(&name)?;
    }

    // Trigger deploy via the same create flow
    let req = super::deploy::CreateDeployRequest {
        repo: payload.repository.clone_url,
        branch: Some(branch),
        name: Some(name.clone()),
        ttl: None,
    };

    // Reuse create_deployment logic by calling it inline
    // For now, return accepted and let it build
    Ok(Json(serde_json::json!({ "action": "deploying", "name": name })))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
