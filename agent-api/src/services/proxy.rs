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
        let route_id = format!("routeroot-{subdomain}");

        let route = json!({
            "@id": route_id,
            "terminal": true,
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

    /// Register a custom domain route: customdomain.com → localhost:port
    pub async fn add_custom_domain_route(&self, custom_domain: &str, target_port: u16) -> Result<(), AppError> {
        let route_id = format!("routeroot-custom-{}", custom_domain.replace('.', "-"));

        let route = json!({
            "@id": route_id,
            "terminal": true,
            "match": [{ "host": [custom_domain] }],
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
            return Err(AppError::Internal(format!("caddy custom domain route failed: {body}")));
        }

        tracing::info!("Added Caddy custom domain route: {custom_domain} → localhost:{target_port}");
        Ok(())
    }

    /// Remove a custom domain route
    pub async fn remove_custom_domain_route(&self, custom_domain: &str) -> Result<(), AppError> {
        let route_id = format!("routeroot-custom-{}", custom_domain.replace('.', "-"));
        let url = format!("{}/id/{route_id}", self.admin_url);

        self.client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;

        tracing::info!("Removed Caddy custom domain route: {custom_domain}");
        Ok(())
    }

    /// Register a path-based route: domain/path/* → localhost:port
    /// Inserts before the root domain catch-all by reading, splicing, and replacing the routes array.
    pub async fn add_path_route(&self, path_prefix: &str, domain: &str, target_port: u16) -> Result<(), AppError> {
        let route_id = format!("routeroot-path-{}", path_prefix.replace('/', "-"));

        let route = json!({
            "@id": route_id,
            "terminal": true,
            "match": [{
                "host": [domain],
                "path": [format!("/{}/*", path_prefix)]
            }],
            "handle": [
                {
                    "handler": "rewrite",
                    "strip_path_prefix": format!("/{}", path_prefix)
                },
                {
                    "handler": "reverse_proxy",
                    "upstreams": [{ "dial": format!("host.docker.internal:{target_port}") }]
                }
            ]
        });

        // Read current routes, find root domain index, insert before it
        let url = format!("{}/config/apps/http/servers/srv0/routes", self.admin_url);
        let resp = self.client.get(&url).send().await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;
        let mut routes: Vec<serde_json::Value> = resp.json().await
            .map_err(|e| AppError::Internal(format!("failed to parse routes: {e}")))?;

        // Find the root domain catch-all (matches domain without path constraints)
        let insert_idx = routes.iter().position(|r| {
            let hosts = r.get("match").and_then(|m| m.as_array()).and_then(|a| a.first())
                .and_then(|m| m.get("host")).and_then(|h| h.as_array());
            let has_path = r.get("match").and_then(|m| m.as_array()).and_then(|a| a.first())
                .and_then(|m| m.get("path")).is_some();
            hosts.map_or(false, |h| h.iter().any(|v| v.as_str() == Some(domain))) && !has_path
        }).unwrap_or(routes.len());

        routes.insert(insert_idx, route);

        // Replace entire routes array
        let resp = self.client.patch(&url)
            .json(&routes)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!("caddy path route failed: {body}")));
        }

        tracing::info!("Added Caddy path route: {domain}/{path_prefix} → localhost:{target_port} (at index {insert_idx})");
        Ok(())
    }

    /// Remove a path-based route
    pub async fn remove_path_route(&self, path_prefix: &str) -> Result<(), AppError> {
        let route_id = format!("routeroot-path-{}", path_prefix.replace('/', "-"));
        let url = format!("{}/id/{route_id}", self.admin_url);

        self.client
            .delete(&url)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy admin request failed: {e}")))?;

        tracing::info!("Removed Caddy path route: {path_prefix}");
        Ok(())
    }

    /// Replace Caddy's entire config with a clean JSON config.
    /// Supports multiple domains — each gets api.domain + root domain routes + wildcard TLS.
    pub async fn init_caddy_config(&self, domains: &[String], tls_check_url: &str) -> Result<(), AppError> {
        // Build TLS policies for all domain wildcards
        let policies: Vec<serde_json::Value> = domains.iter().map(|domain| {
            json!({
                "subjects": [format!("*.{domain}")],
                "issuers": [{ "module": "acme" }],
                "on_demand": true
            })
        }).collect();

        // Build routes: api.domain + root domain for each domain
        let mut routes: Vec<serde_json::Value> = Vec::new();
        for domain in domains {
            let api_host = format!("api.{domain}");
            routes.push(json!({
                "match": [{ "host": [&api_host] }],
                "handle": [{
                    "handler": "reverse_proxy",
                    "upstreams": [{ "dial": "agent-api:8053" }]
                }],
                "terminal": true
            }));
        }
        for domain in domains {
            routes.push(json!({
                "match": [{ "host": [domain] }],
                "handle": [{
                    "handler": "reverse_proxy",
                    "upstreams": [{ "dial": "agent-api:8053" }]
                }]
                // NOT terminal — path routes appended later take priority
            }));
        }

        let config = json!({
            "admin": { "listen": ":2019" },
            "apps": {
                "tls": {
                    "automation": {
                        "on_demand": {
                            "permission": {
                                "module": "http",
                                "endpoint": tls_check_url
                            }
                        },
                        "policies": policies
                    }
                },
                "http": {
                    "servers": {
                        "srv0": {
                            "listen": [":443", ":80"],
                            "routes": routes
                        }
                    }
                }
            }
        });

        let url = format!("{}/config/", self.admin_url);
        let resp = self.client.put(&url)
            .json(&config)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("caddy config load failed: {e}")))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!("caddy config load failed: {body}")));
        }

        tracing::info!("Initialized Caddy config via JSON API for domains {:?}", domains);
        Ok(())
    }

    /// Remove a route by subdomain
    pub async fn remove_route(&self, subdomain: &str) -> Result<(), AppError> {
        let route_id = format!("routeroot-{subdomain}");
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
