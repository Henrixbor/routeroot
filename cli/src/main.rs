use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "routeroot", about = "RouteRoot CLI — deploy branches as live URLs")]
struct Cli {
    /// API server URL
    #[arg(long, env = "ROUTEROOT_URL", default_value = "http://localhost:8053")]
    server: String,

    /// API key (required — set ROUTEROOT_API_KEY env var or pass --key)
    #[arg(long, env = "ROUTEROOT_API_KEY")]
    key: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy a repo branch directly
    Deploy {
        repo: String,
        #[arg(short, long, default_value = "main")]
        branch: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        ttl: Option<String>,
        #[arg(short, long, default_value = "preview")]
        environment: String,
        /// Deploy at domain/prefix instead of subdomain
        #[arg(long)]
        path_prefix: Option<String>,
    },
    /// Create a deployment plan (dry-run)
    Plan {
        repo: String,
        #[arg(short, long, default_value = "main")]
        branch: String,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        ttl: Option<String>,
    },
    /// Apply a pending deployment plan
    Apply {
        /// Plan ID
        plan_id: String,
    },
    /// List deployment plans
    Plans,
    /// Promote a deployment to staging or production
    Promote {
        name: String,
        /// Target environment (staging or production)
        target: String,
    },
    /// List active deployments
    Ls,
    /// Get deployment details
    Status { name: String },
    /// Get deployment logs
    Logs { name: String },
    /// Tear down a deployment
    Down { name: String },
    /// Manage DNS records
    Record {
        #[command(subcommand)]
        action: RecordAction,
    },
    /// Manage custom domain mappings
    Domain {
        #[command(subcommand)]
        action: DomainAction,
    },
    /// View audit log
    Audit {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// Check system health
    Health,
    /// Show setup instructions for connecting Claude Code, CLI, and webhooks
    Setup {
        /// Automatically configure MCP server in ~/.claude/mcp.json
        #[arg(long)]
        configure_mcp: bool,
    },
}

#[derive(Subcommand)]
enum RecordAction {
    /// Add a DNS record
    Add {
        name: String,
        #[arg(short = 't', long, default_value = "A")]
        record_type: String,
        value: String,
    },
    /// List DNS records
    Ls,
    /// Remove a DNS record
    Rm { name: String },
}

#[derive(Subcommand)]
enum DomainAction {
    /// Map a custom domain to a deployment (e.g. client.com -> my-app)
    Map {
        /// Custom domain (e.g. app.client.com)
        domain: String,
        /// Deployment name to route to
        deployment: String,
    },
    /// List all custom domain mappings
    Ls,
    /// Remove a custom domain mapping
    Rm {
        /// Custom domain to remove
        domain: String,
    },
}

#[derive(Deserialize)]
struct DeployResponse {
    name: String,
    url: String,
    status: String,
    environment: Option<String>,
}

#[derive(Deserialize)]
struct Deployment {
    name: String,
    repo: String,
    branch: String,
    status: String,
    verified: Option<String>,
    environment: Option<String>,
    url: String,
    created_at: String,
}

#[derive(Deserialize)]
struct DnsRecord {
    name: String,
    record_type: String,
    value: String,
}

#[derive(Deserialize)]
struct HealthResponse {
    status: String,
    domain: String,
    #[serde(default)]
    domains: Vec<String>,
    active_deployments: usize,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct DeployPlan {
    id: String,
    name: String,
    repo: String,
    branch: String,
    environment: String,
    url: String,
    actions: String,
    status: String,
    created_at: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct AuditEvent {
    action: String,
    resource_type: String,
    resource_name: String,
    actor: String,
    details: String,
    created_at: String,
}

fn main() {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::new();
    let base = cli.server.trim_end_matches('/');
    let auth = format!("Bearer {}", cli.key);

    match cli.command {
        Commands::Deploy { repo, branch, name, ttl, environment, path_prefix } => {
            let body = serde_json::json!({
                "repo": repo, "branch": branch, "name": name, "ttl": ttl, "environment": environment, "path_prefix": path_prefix,
            });
            match api_post::<DeployResponse>(&client, &format!("{base}/api/deploy"), &auth, &body) {
                Ok(resp) => {
                    println!("Deploying '{}' ...", resp.name);
                    println!("URL:         {}", resp.url);
                    println!("Status:      {}", resp.status);
                    println!("Environment: {}", resp.environment.unwrap_or_else(|| "preview".into()));
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Plan { repo, branch, name, ttl } => {
            let body = serde_json::json!({
                "repo": repo, "branch": branch, "name": name, "ttl": ttl,
            });
            match api_post::<DeployPlan>(&client, &format!("{base}/api/plan"), &auth, &body) {
                Ok(plan) => {
                    println!("Plan created: {}", plan.id);
                    println!("Name:         {}", plan.name);
                    println!("URL:          {}", plan.url);
                    println!("Environment:  {}", plan.environment);
                    println!("Status:       {}", plan.status);
                    println!();
                    println!("Actions:");
                    if let Ok(actions) = serde_json::from_str::<Vec<serde_json::Value>>(&plan.actions) {
                        for (i, action) in actions.iter().enumerate() {
                            println!("  {}. {}", i + 1, action.get("action").and_then(|a| a.as_str()).unwrap_or("?"));
                        }
                    }
                    println!();
                    println!("To apply: routeroot apply {}", plan.id);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Apply { plan_id } => {
            match api_post::<DeployResponse>(&client, &format!("{base}/api/plan/{plan_id}/apply"), &auth, &serde_json::json!({})) {
                Ok(resp) => {
                    println!("Plan applied! Deploying '{}' ...", resp.name);
                    println!("URL:    {}", resp.url);
                    println!("Status: {}", resp.status);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Plans => {
            match api_get::<Vec<DeployPlan>>(&client, &format!("{base}/api/plans"), &auth) {
                Ok(plans) => {
                    if plans.is_empty() {
                        println!("No plans.");
                        return;
                    }
                    println!("{:<38} {:<20} {:<10} {:<10} {:<30}", "ID", "NAME", "ENV", "STATUS", "CREATED");
                    println!("{}", "-".repeat(108));
                    for p in plans {
                        println!("{:<38} {:<20} {:<10} {:<10} {:<30}", p.id, p.name, p.environment, p.status, p.created_at);
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Promote { name, target } => {
            let body = serde_json::json!({ "target": target });
            match api_post::<DeployResponse>(&client, &format!("{base}/api/deploy/{name}/promote"), &auth, &body) {
                Ok(resp) => {
                    println!("Promoted '{}' to {}", resp.name, resp.environment.unwrap_or(target));
                    println!("URL: {}", resp.url);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Ls => {
            match api_get::<Vec<Deployment>>(&client, &format!("{base}/api/deployments"), &auth) {
                Ok(deployments) => {
                    if deployments.is_empty() {
                        println!("No active deployments.");
                        return;
                    }
                    println!("{:<22} {:<10} {:<10} {:<10} {:<12} {:<35}", "NAME", "STATUS", "VERIFIED", "ENV", "BRANCH", "URL");
                    println!("{}", "-".repeat(109));
                    for d in deployments {
                        println!("{:<22} {:<10} {:<10} {:<10} {:<12} {:<35}",
                            d.name,
                            d.status,
                            d.verified.as_deref().unwrap_or("-"),
                            d.environment.as_deref().unwrap_or("preview"),
                            d.branch,
                            d.url,
                        );
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Status { name } => {
            match api_get::<Deployment>(&client, &format!("{base}/api/deployments/{name}"), &auth) {
                Ok(d) => {
                    println!("Name:        {}", d.name);
                    println!("Repo:        {}", d.repo);
                    println!("Branch:      {}", d.branch);
                    println!("Status:      {}", d.status);
                    println!("Verified:    {}", d.verified.as_deref().unwrap_or("-"));
                    println!("Environment: {}", d.environment.as_deref().unwrap_or("preview"));
                    println!("URL:         {}", d.url);
                    println!("Created:     {}", d.created_at);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Logs { name } => {
            match api_get::<Vec<String>>(&client, &format!("{base}/api/deployments/{name}/logs"), &auth) {
                Ok(logs) => {
                    for line in logs {
                        println!("{line}");
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Down { name } => {
            match client.delete(format!("{base}/api/deploy/{name}"))
                .header("Authorization", &auth)
                .send() {
                Ok(_) => println!("Deployment '{name}' torn down."),
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Record { action } => match action {
            RecordAction::Add { name, record_type, value } => {
                let body = serde_json::json!({
                    "name": name, "record_type": record_type, "value": value,
                });
                match api_post::<DnsRecord>(&client, &format!("{base}/api/records"), &auth, &body) {
                    Ok(_) => println!("Record added: {name} {record_type} {value}"),
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            RecordAction::Ls => {
                match api_get::<Vec<DnsRecord>>(&client, &format!("{base}/api/records"), &auth) {
                    Ok(records) => {
                        if records.is_empty() {
                            println!("No custom DNS records.");
                            return;
                        }
                        println!("{:<25} {:<8} {:<30}", "NAME", "TYPE", "VALUE");
                        println!("{}", "-".repeat(63));
                        for r in records {
                            println!("{:<25} {:<8} {:<30}", r.name, r.record_type, r.value);
                        }
                    }
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            RecordAction::Rm { name } => {
                match client.delete(format!("{base}/api/records/{name}"))
                    .header("Authorization", &auth)
                    .send() {
                    Ok(_) => println!("Record '{name}' deleted."),
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
        },

        Commands::Audit { limit } => {
            match api_get::<Vec<AuditEvent>>(&client, &format!("{base}/api/audit?limit={limit}"), &auth) {
                Ok(events) => {
                    if events.is_empty() {
                        println!("No audit events.");
                        return;
                    }
                    println!("{:<20} {:<18} {:<14} {:<20} {:<30}", "TIMESTAMP", "ACTION", "TYPE", "RESOURCE", "ACTOR");
                    println!("{}", "-".repeat(102));
                    for e in events {
                        let ts = e.created_at.get(..19).unwrap_or(&e.created_at);
                        println!("{:<20} {:<18} {:<14} {:<20} {:<30}", ts, e.action, e.resource_type, e.resource_name, e.actor);
                    }
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Domain { action } => match action {
            DomainAction::Map { domain, deployment } => {
                let body = serde_json::json!({
                    "domain": domain, "deployment_name": deployment,
                });
                match api_post::<serde_json::Value>(&client, &format!("{base}/api/domains"), &auth, &body) {
                    Ok(resp) => {
                        println!("Custom domain mapped: {} -> {}", domain, deployment);
                        if let Some(instructions) = resp.get("instructions").and_then(|v| v.as_str()) {
                            println!();
                            println!("Next step:");
                            println!("  {}", instructions);
                        }
                    }
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            DomainAction::Ls => {
                match api_get::<Vec<serde_json::Value>>(&client, &format!("{base}/api/domains"), &auth) {
                    Ok(domains) => {
                        if domains.is_empty() {
                            println!("No custom domain mappings.");
                            return;
                        }
                        println!("{:<30} {:<20} {:<10}", "DOMAIN", "DEPLOYMENT", "VERIFIED");
                        println!("{}", "-".repeat(60));
                        for d in domains {
                            println!("{:<30} {:<20} {:<10}",
                                d.get("domain").and_then(|v| v.as_str()).unwrap_or("-"),
                                d.get("deployment_name").and_then(|v| v.as_str()).unwrap_or("-"),
                                d.get("verified").and_then(|v| v.as_bool()).map(|b| if b { "yes" } else { "no" }).unwrap_or("-"),
                            );
                        }
                    }
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            DomainAction::Rm { domain } => {
                match client.delete(format!("{base}/api/domains/{domain}"))
                    .header("Authorization", &auth)
                    .send() {
                    Ok(_) => println!("Custom domain '{}' removed.", domain),
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
        },

        Commands::Health => {
            match api_get::<HealthResponse>(&client, &format!("{base}/api/health"), "") {
                Ok(h) => {
                    println!("Status:      {}", h.status);
                    println!("Domain:      {}", h.domain);
                    if !h.domains.is_empty() {
                        println!("Domains:     {}", h.domains.join(", "));
                    }
                    println!("Deployments: {}", h.active_deployments);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
        }

        Commands::Setup { configure_mcp } => {
            // Try to detect the domain from health endpoint
            let domain_hint = api_get::<HealthResponse>(&client, &format!("{base}/api/health"), "")
                .ok()
                .map(|h| h.domain)
                .unwrap_or_else(|| "YOUR_DOMAIN".into());
            let api_url_hint = if base.contains("localhost") || base.contains("127.0.0.1") {
                format!("https://api.{domain_hint}")
            } else {
                base.to_string()
            };

            println!("RouteRoot Setup Guide");
            println!("=====================");
            println!();
            println!("1. CONNECT CLI");
            println!("   export ROUTEROOT_URL={api_url_hint}");
            println!("   export ROUTEROOT_API_KEY=YOUR_API_KEY");
            println!("   routeroot health");
            println!();
            println!("2. CONNECT CLAUDE CODE (MCP)");
            println!("   Build:  cargo install --path mcp-server");
            println!();
            println!("   Add to ~/.claude/mcp.json:");
            println!("   {{");
            println!("     \"mcpServers\": {{");
            println!("       \"routeroot\": {{");
            println!("         \"command\": \"routeroot-mcp\",");
            println!("         \"env\": {{");
            println!("           \"ROUTEROOT_URL\": \"{api_url_hint}\",");
            println!("           \"ROUTEROOT_API_KEY\": \"YOUR_API_KEY\"");
            println!("         }}");
            println!("       }}");
            println!("     }}");
            println!("   }}");
            println!();

            // Auto-configure MCP if requested
            let mcp_path = dirs_next::home_dir()
                .map(|h| h.join(".claude").join("mcp.json"));
            if configure_mcp {
                if let Some(ref path) = mcp_path {
                    let mcp_result = configure_mcp_json(path, &api_url_hint, &cli.key);
                    match mcp_result {
                        Ok(created) => {
                            if created {
                                println!("   MCP configured! Created ~/.claude/mcp.json with RouteRoot server.");
                            } else {
                                println!("   MCP configured! Updated ~/.claude/mcp.json with RouteRoot server.");
                            }
                            println!("   Restart Claude Code to activate 15 RouteRoot tools.");
                        }
                        Err(e) => {
                            println!("   Failed to configure MCP: {e}");
                            println!("   Manually create the file above.");
                        }
                    }
                }
            } else if let Some(ref path) = mcp_path {
                if path.exists() {
                    // Check if routeroot is already configured
                    let has_routeroot = std::fs::read_to_string(path)
                        .ok()
                        .map(|c| c.contains("routeroot"))
                        .unwrap_or(false);
                    if has_routeroot {
                        println!("   Detected: MCP already configured in ~/.claude/mcp.json");
                    } else {
                        println!("   Detected: ~/.claude/mcp.json exists (RouteRoot not yet added)");
                        println!("   Run with --configure-mcp to auto-add the RouteRoot MCP server.");
                    }
                } else {
                    println!("   Note: ~/.claude/mcp.json not found.");
                    println!("   Run with --configure-mcp to create it, or create manually.");
                }
            }
            println!();
            println!("   Restart Claude Code — 15 tools become available:");
            println!("   deploy_preview, list_deployments, get_deployment, teardown,");
            println!("   get_logs, create_dns_record, list_dns_records, delete_dns_record,");
            println!("   health, promote, plan_deploy, apply_plan,");
            println!("   map_custom_domain, list_custom_domains, delete_custom_domain");
            println!();
            println!("3. DEPLOY YOUR FIRST BRANCH");
            println!("   routeroot deploy https://github.com/user/repo --branch main");
            println!("   routeroot ls");
            println!();
            println!("4. CUSTOM DOMAINS");
            println!("   routeroot domain map app.client.com my-deployment");
            println!("   Then add a CNAME at client.com's DNS: app -> my-deployment.{domain_hint}");
            println!();
            println!("5. PATH ROUTING");
            println!("   routeroot deploy https://github.com/user/repo --path-prefix client/staging");
            println!("   => https://{domain_hint}/client/staging");
            println!();
            println!("6. GITHUB WEBHOOKS");
            println!("   Repo Settings -> Webhooks -> Add:");
            println!("   URL: {api_url_hint}/api/webhook/github");
            println!("   Content type: application/json");
            println!("   Events: Push");
            println!();
            println!("7. PROMOTE");
            println!("   routeroot promote my-app staging");
            println!("   routeroot promote my-app production");
            println!();
            println!("Full docs: https://github.com/Vibeyard/AgentDNS");
        }
    }
}

fn api_get<T: serde::de::DeserializeOwned>(client: &reqwest::blocking::Client, url: &str, auth: &str) -> Result<T, String> {
    let mut req = client.get(url);
    if !auth.is_empty() {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status}: {body}"));
    }
    resp.json().map_err(|e| format!("invalid response: {e}"))
}

/// Configure MCP server in ~/.claude/mcp.json. Returns Ok(true) if file was created, Ok(false) if updated.
fn configure_mcp_json(path: &std::path::Path, api_url: &str, api_key: &str) -> Result<bool, String> {
    // Ensure parent dir exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }

    let (mut config, created) = if path.exists() {
        let content = std::fs::read_to_string(path).map_err(|e| format!("read: {e}"))?;
        let parsed: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("parse: {e}"))?;
        (parsed, false)
    } else {
        (serde_json::json!({}), true)
    };

    // Ensure mcpServers object exists
    if config.get("mcpServers").is_none() {
        config["mcpServers"] = serde_json::json!({});
    }

    // Add/update routeroot entry
    config["mcpServers"]["routeroot"] = serde_json::json!({
        "command": "routeroot-mcp",
        "env": {
            "ROUTEROOT_URL": api_url,
            "ROUTEROOT_API_KEY": api_key
        }
    });

    let output = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(path, output).map_err(|e| format!("write: {e}"))?;
    Ok(created)
}

fn api_post<T: serde::de::DeserializeOwned>(client: &reqwest::blocking::Client, url: &str, auth: &str, body: &serde_json::Value) -> Result<T, String> {
    let resp = client.post(url)
        .header("Authorization", auth)
        .json(body)
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("{status}: {body}"));
    }
    resp.json().map_err(|e| format!("invalid response: {e}"))
}
