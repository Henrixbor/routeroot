use reqwest::Client;
use serde_json::json;

use crate::error::AppError;

pub struct ProxyService {
    client: Client,
    admin_url: String,
}

impl ProxyService {
    pub fn new(admin_url: &str) -> Self {
        Self {
            client: Client::new(),
            admin_url: admin_url.trim_end_matches('/').to_string(),
        }
    }

    /// Register a route: subdomain.domain → localhost:port
    pub async fn add_route(&self, subdomain: &str, domain: &str, target_port: u16) -> Result<(), AppError> {
        let host = format!("{subdomain}.{domain}");
        let route_id = format!("agentdns-{subdomain}");

        let route = json!({
            "@id": route_id,
            "match": [{ "host": [host] }],
            "handle": [{
                "handler": "reverse_proxy",
                "upstreams": [{ "dial": format!("host.docker.internal:{target_port}") }]
            }]
        });

        let url = format!("{}/config/apps/http/servers/srv0/routes", self.admin_url);

        let resp = self.client
            .post(&url)
            .json(&route)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!("caddy route add failed: {body}")));
        }

        tracing::info!("Added Caddy route: {host} → localhost:{target_port}");
        Ok(())
    }

    /// Remove a route by subdomain
    pub async fn remove_route(&self, subdomain: &str) -> Result<(), AppError> {
        let route_id = format!("agentdns-{subdomain}");
        let url = format!("{}/id/{route_id}", self.admin_url);

        self.client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;

        tracing::info!("Removed Caddy route: {subdomain}");
        Ok(())
    }
}
