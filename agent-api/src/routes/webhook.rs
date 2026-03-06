use axum::{Json, extract::State, http::HeaderMap};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use serde::Deserialize;
use std::sync::Arc;

use crate::{AppState, error::AppError};

type HmacSha256 = Hmac<Sha256>;

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
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    // Verify webhook signature if secret is configured
    if let Some(ref secret) = state.config.github_webhook_secret {
        let signature = headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        verify_signature(secret, &body, signature)?;
    }

    let payload: GitHubPushEvent = serde_json::from_slice(&body)
        .map_err(|e| AppError::BadRequest(format!("invalid payload: {e}")))?;

    let branch = payload.git_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&payload.git_ref)
        .to_string();

    let name = super::deploy::sanitize_name(
        &format!("{}-{}", payload.repository.name, &branch)
    );

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
    if let Some(deployment) = state.db.get_deployment(&name)? {
        if let Some(ref cid) = deployment.container_id {
            state.docker.stop_container(cid).await.ok();
        }
        state.proxy.remove_route(&name).await.ok();
        state.db.delete_deployment(&name)?;
    }

    let req = super::deploy::CreateDeployRequest {
        repo: payload.repository.clone_url,
        branch: Some(branch),
        name: Some(name.clone()),
        ttl: None,
        environment: Some("preview".into()),
        path_prefix: None,
    };

    let result = super::deploy::create_deployment(State(state), Json(req)).await?;
    Ok(Json(serde_json::json!({ "action": "deploying", "name": result.name, "url": result.url })))
}

fn verify_signature(secret: &str, body: &[u8], signature: &str) -> Result<(), AppError> {
    let sig_hex = signature.strip_prefix("sha256=").unwrap_or(signature);

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| AppError::Internal("invalid webhook secret".into()))?;
    mac.update(body);

    let expected = hex::decode(sig_hex)
        .map_err(|_| AppError::Unauthorized)?;

    mac.verify_slice(&expected)
        .map_err(|_| AppError::Unauthorized)?;

    Ok(())
}
