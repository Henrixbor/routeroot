use axum::{
    extract::Request,
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

use crate::{AppState, error::AppError};

type HmacSha256 = Hmac<Sha256>;

/// Constant-time API key verification to prevent timing attacks.
fn verify_api_key(provided: &str, expected: &str) -> bool {
    // HMAC-based constant-time comparison: compute HMAC of a fixed message
    // with both keys and compare using verify_slice (which uses subtle::ConstantTimeEq)
    let Ok(mut mac) = HmacSha256::new_from_slice(provided.as_bytes()) else {
        return false;
    };
    mac.update(b"routeroot-key-verify");

    let Ok(mut expected_mac) = HmacSha256::new_from_slice(expected.as_bytes()) else {
        return false;
    };
    expected_mac.update(b"routeroot-key-verify");
    let expected_result = expected_mac.finalize().into_bytes();

    mac.verify_slice(&expected_result).is_ok()
}

pub async fn require_api_key(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or(auth_header);

    if !verify_api_key(token, &state.config.api_key) {
        return Err(AppError::Unauthorized);
    }

    Ok(next.run(request).await)
}
