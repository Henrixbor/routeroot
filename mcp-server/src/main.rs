use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, Write};

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    result: Value,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct JsonRpcError {
    jsonrpc: String,
    id: Value,
    error: JsonRpcErrorBody,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct JsonRpcErrorBody {
    code: i64,
    message: String,
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

struct Config {
    api_url: String,
    api_key: String,
}

impl Config {
    fn from_env() -> Self {
        let api_key = env::var("ROUTEROOT_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            eprintln!("[routeroot-mcp] ERROR: ROUTEROOT_API_KEY environment variable is required");
            std::process::exit(1);
        }
        let api_url = env::var("ROUTEROOT_URL").unwrap_or_else(|_| {
            eprintln!("[routeroot-mcp] WARN: ROUTEROOT_URL not set, using http://localhost:8053");
            "http://localhost:8053".into()
        });
        Self { api_url, api_key }
    }
}

// ---------------------------------------------------------------------------
// HTTP helper
// ---------------------------------------------------------------------------

async fn api_request(
    client: &reqwest::Client,
    cfg: &Config,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let url = format!("{}{}", cfg.api_url, path);
    eprintln!("[routeroot-mcp] {} {}", method, url);

    let builder = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "DELETE" => client.delete(&url),
        _ => return Err(format!("unsupported HTTP method: {}", method)),
    };

    let builder = builder.header("Authorization", format!("Bearer {}", cfg.api_key));

    let builder = if let Some(b) = body {
        builder.json(&b)
    } else {
        builder
    };

    let resp = builder.send().await.map_err(|e| format!("request failed: {}", e))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("failed to read response: {}", e))?;

    if !status.is_success() {
        return Err(format!("API returned {} — {}", status, text));
    }

    serde_json::from_str(&text).map_err(|_| text)
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": "deploy_preview",
                "description": "Deploy a git repo branch as a live preview URL via RouteRoot. Creates a subdomain (repo-branch.yourdomain) by default. Use path_prefix for path-based routing (yourdomain/prefix). Returns the live URL immediately.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string", "description": "Git repository URL (e.g. https://github.com/user/repo)" },
                        "branch": { "type": "string", "description": "Branch to deploy (default: main)" },
                        "name": { "type": "string", "description": "Optional deployment name" },
                        "ttl": { "type": "string", "description": "Time to live, e.g. '24h'" },
                        "path_prefix": { "type": "string", "description": "Optional: deploy at yourdomain/prefix instead of subdomain" }
                    },
                    "required": ["repo"]
                }
            },
            {
                "name": "list_deployments",
                "description": "List all active RouteRoot deployments.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "get_deployment",
                "description": "Get details of a specific RouteRoot deployment.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Deployment name" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "teardown",
                "description": "Tear down a RouteRoot deployment.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Deployment name" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "get_logs",
                "description": "Get container logs for a RouteRoot deployment.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Deployment name" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "create_dns_record",
                "description": "Create a custom DNS record via RouteRoot.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "DNS record name" },
                        "record_type": { "type": "string", "description": "Record type (default: A)" },
                        "value": { "type": "string", "description": "Record value (e.g. IP address)" }
                    },
                    "required": ["name", "value"]
                }
            },
            {
                "name": "list_dns_records",
                "description": "List all custom DNS records managed by RouteRoot.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "delete_dns_record",
                "description": "Delete a DNS record from RouteRoot.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "DNS record name to delete" }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "health",
                "description": "Check RouteRoot system health.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "promote",
                "description": "Promote a deployment to a different environment (staging or production).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Deployment name" },
                        "target": { "type": "string", "description": "Target environment: 'staging' or 'production'" }
                    },
                    "required": ["name", "target"]
                }
            },
            {
                "name": "plan_deploy",
                "description": "Create a deployment plan without executing it. Returns what DNS records, routes, and containers will be created.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string", "description": "Git repository URL" },
                        "branch": { "type": "string", "description": "Branch to deploy (default: main)" },
                        "name": { "type": "string", "description": "Optional deployment name" },
                        "ttl": { "type": "string", "description": "Time to live, e.g. '24h'" }
                    },
                    "required": ["repo"]
                }
            },
            {
                "name": "apply_plan",
                "description": "Execute a previously created deployment plan.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "plan_id": { "type": "string", "description": "Plan ID returned by plan_deploy" }
                    },
                    "required": ["plan_id"]
                }
            },
            {
                "name": "map_custom_domain",
                "description": "Map a custom domain (e.g. client.com) to an existing deployment. The domain owner must add a CNAME record pointing to the deployment subdomain.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "domain": { "type": "string", "description": "Custom domain to map (e.g. app.client.com)" },
                        "deployment_name": { "type": "string", "description": "Name of the deployment to route to" }
                    },
                    "required": ["domain", "deployment_name"]
                }
            },
            {
                "name": "list_custom_domains",
                "description": "List all custom domain mappings.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "delete_custom_domain",
                "description": "Remove a custom domain mapping.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "domain": { "type": "string", "description": "Custom domain to remove" }
                    },
                    "required": ["domain"]
                }
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

async fn call_tool(
    client: &reqwest::Client,
    cfg: &Config,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "deploy_preview" => {
            let repo = args.get("repo").and_then(|v| v.as_str()).ok_or("missing required param: repo")?;
            let mut body = json!({ "repo": repo });
            if let Some(b) = args.get("branch").and_then(|v| v.as_str()) {
                body["branch"] = json!(b);
            }
            if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                body["name"] = json!(n);
            }
            if let Some(t) = args.get("ttl").and_then(|v| v.as_str()) {
                body["ttl"] = json!(t);
            }
            if let Some(p) = args.get("path_prefix").and_then(|v| v.as_str()) {
                body["path_prefix"] = json!(p);
            }
            api_request(client, cfg, "POST", "/api/deploy", Some(body)).await
        }

        "list_deployments" => {
            api_request(client, cfg, "GET", "/api/deployments", None).await
        }

        "get_deployment" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            api_request(client, cfg, "GET", &format!("/api/deployments/{}", name), None).await
        }

        "teardown" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            api_request(client, cfg, "DELETE", &format!("/api/deploy/{}", name), None).await
        }

        "get_logs" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            api_request(client, cfg, "GET", &format!("/api/deployments/{}/logs", name), None).await
        }

        "create_dns_record" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            let value = args.get("value").and_then(|v| v.as_str()).ok_or("missing required param: value")?;
            let record_type = args.get("record_type").and_then(|v| v.as_str()).unwrap_or("A");
            let body = json!({
                "name": name,
                "record_type": record_type,
                "value": value
            });
            api_request(client, cfg, "POST", "/api/records", Some(body)).await
        }

        "list_dns_records" => {
            api_request(client, cfg, "GET", "/api/records", None).await
        }

        "delete_dns_record" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            api_request(client, cfg, "DELETE", &format!("/api/records/{}", name), None).await
        }

        "health" => {
            api_request(client, cfg, "GET", "/api/health", None).await
        }

        "promote" => {
            let name = args.get("name").and_then(|v| v.as_str()).ok_or("missing required param: name")?;
            let target = args.get("target").and_then(|v| v.as_str()).ok_or("missing required param: target")?;
            let body = json!({ "target": target });
            api_request(client, cfg, "POST", &format!("/api/deploy/{}/promote", name), Some(body)).await
        }

        "plan_deploy" => {
            let repo = args.get("repo").and_then(|v| v.as_str()).ok_or("missing required param: repo")?;
            let mut body = json!({ "repo": repo });
            if let Some(b) = args.get("branch").and_then(|v| v.as_str()) {
                body["branch"] = json!(b);
            }
            if let Some(n) = args.get("name").and_then(|v| v.as_str()) {
                body["name"] = json!(n);
            }
            if let Some(t) = args.get("ttl").and_then(|v| v.as_str()) {
                body["ttl"] = json!(t);
            }
            api_request(client, cfg, "POST", "/api/plan", Some(body)).await
        }

        "apply_plan" => {
            let plan_id = args.get("plan_id").and_then(|v| v.as_str()).ok_or("missing required param: plan_id")?;
            api_request(client, cfg, "POST", &format!("/api/plan/{}/apply", plan_id), None).await
        }

        "map_custom_domain" => {
            let domain = args.get("domain").and_then(|v| v.as_str()).ok_or("missing required param: domain")?;
            let deployment_name = args.get("deployment_name").and_then(|v| v.as_str()).ok_or("missing required param: deployment_name")?;
            let body = json!({
                "domain": domain,
                "deployment_name": deployment_name
            });
            api_request(client, cfg, "POST", "/api/domains", Some(body)).await
        }

        "list_custom_domains" => {
            api_request(client, cfg, "GET", "/api/domains", None).await
        }

        "delete_custom_domain" => {
            let domain = args.get("domain").and_then(|v| v.as_str()).ok_or("missing required param: domain")?;
            api_request(client, cfg, "DELETE", &format!("/api/domains/{}", domain), None).await
        }

        _ => Err(format!("unknown tool: {}", name)),
    }
}

// ---------------------------------------------------------------------------
// Request handler
// ---------------------------------------------------------------------------

async fn handle_request(
    client: &reqwest::Client,
    cfg: &Config,
    req: &JsonRpcRequest,
) -> Option<Value> {
    match req.method.as_str() {
        "initialize" => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "routeroot-mcp",
                    "version": "0.1.0"
                }
            });
            Some(json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": result
            }))
        }

        "notifications/initialized" => {
            eprintln!("[routeroot-mcp] client initialized");
            None // notifications have no response
        }

        "tools/list" => {
            let defs = tool_definitions();
            Some(json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": defs
            }))
        }

        "tools/call" => {
            let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = req.params.get("arguments").cloned().unwrap_or(json!({}));

            eprintln!("[routeroot-mcp] calling tool: {} with args: {}", tool_name, arguments);

            match call_tool(client, cfg, tool_name, &arguments).await {
                Ok(val) => {
                    let text = serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string());
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "result": {
                            "content": [
                                { "type": "text", "text": text }
                            ]
                        }
                    }))
                }
                Err(e) => {
                    eprintln!("[routeroot-mcp] tool error: {}", e);
                    Some(json!({
                        "jsonrpc": "2.0",
                        "id": req.id,
                        "result": {
                            "content": [
                                { "type": "text", "text": format!("Error: {}", e) }
                            ],
                            "isError": true
                        }
                    }))
                }
            }
        }

        // Handle ping
        "ping" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "result": {}
            }))
        }

        // Unknown methods that have an id get an error response
        _ => {
            if req.id.is_some() {
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": req.id,
                    "error": {
                        "code": -32601,
                        "message": format!("method not found: {}", req.method)
                    }
                }))
            } else {
                // Unknown notification — ignore
                eprintln!("[routeroot-mcp] ignoring unknown notification: {}", req.method);
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    eprintln!("[routeroot-mcp] starting MCP server (stdio transport)");

    let cfg = Config::from_env();
    eprintln!("[routeroot-mcp] API URL: {}", cfg.api_url);

    let client = reqwest::Client::new();
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[routeroot-mcp] stdin read error: {}", e);
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        eprintln!("[routeroot-mcp] <- {}", trimmed);

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[routeroot-mcp] parse error: {}", e);
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("parse error: {}", e)
                    }
                });
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", err);
                let _ = out.flush();
                continue;
            }
        };

        if let Some(response) = handle_request(&client, &cfg, &req).await {
            let response_str = serde_json::to_string(&response).unwrap();
            eprintln!("[routeroot-mcp] -> {}", response_str);
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", response_str);
            let _ = out.flush();
        }
    }

    eprintln!("[routeroot-mcp] shutting down");
}
