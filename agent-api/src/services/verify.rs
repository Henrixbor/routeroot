use std::sync::Arc;
use tokio::time::{Duration, sleep};

use crate::AppState;

/// After a deployment goes live, verify DNS resolution and HTTP health.
/// Runs as a background task per deployment.
pub async fn verify_deployment(state: Arc<AppState>, name: String) {
    // Wait for DNS propagation + container startup
    sleep(Duration::from_secs(10)).await;

    let domain = format!("{}.{}", name, state.config.domain);
    let url = format!("https://{domain}");

    // Step 1: DNS verification — check the domain resolves
    let dns_ok = verify_dns(&domain, &state.config.server_ip).await;

    // Step 2: HTTP verification — check the URL responds
    let http_ok = if dns_ok {
        verify_http(&url).await
    } else {
        false
    };

    let status = match (dns_ok, http_ok) {
        (true, true) => "verified",
        (true, false) => "dns_ok",
        (false, _) => "dns_failed",
    };

    if let Err(e) = state.db.update_deployment_verified(&name, status) {
        tracing::error!("verify: failed to update deployment '{name}': {e}");
    } else {
        tracing::info!("verify: deployment '{name}' status={status}");
    }
}

async fn verify_dns(domain: &str, expected_ip: &str) -> bool {
    // Use tokio's DNS resolution
    match tokio::net::lookup_host(format!("{domain}:80")).await {
        Ok(addrs) => {
            let resolved: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
            let ok = resolved.iter().any(|ip| ip == expected_ip);
            if !ok {
                tracing::warn!("verify: DNS for {domain} resolved to {resolved:?}, expected {expected_ip}");
            }
            ok
        }
        Err(e) => {
            tracing::warn!("verify: DNS lookup failed for {domain}: {e}");
            false
        }
    }
}

async fn verify_http(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .danger_accept_invalid_certs(true) // preview certs may not be ready yet
        .build()
        .unwrap();

    for attempt in 0..3 {
        if attempt > 0 {
            sleep(Duration::from_secs(5)).await;
        }
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if status < 500 {
                    tracing::info!("verify: HTTP check {url} → {status}");
                    return true;
                }
                tracing::warn!("verify: HTTP check {url} → {status} (attempt {attempt})");
            }
            Err(e) => {
                tracing::warn!("verify: HTTP check {url} failed: {e} (attempt {attempt})");
            }
        }
    }
    false
}
