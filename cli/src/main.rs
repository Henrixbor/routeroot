use clap::{Parser, Subcommand};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "routeroot", about = "RouteRoot CLI — deploy branches as live URLs")]
struct Cli {
    /// API server URL
    #[arg(long, env = "ROUTEROOT_URL", default_value = "http://localhost:8053")]
    server: String,

    /// API key
    #[arg(long, env = "ROUTEROOT_API_KEY", default_value = "dev-key")]
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
    /// View audit log
    Audit {
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// Check system health
    Health,
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
        Commands::Deploy { repo, branch, name, ttl, environment } => {
            let body = serde_json::json!({
                "repo": repo, "branch": branch, "name": name, "ttl": ttl, "environment": environment,
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

        Commands::Health => {
            match api_get::<HealthResponse>(&client, &format!("{base}/api/health"), "") {
                Ok(h) => {
                    println!("Status:      {}", h.status);
                    println!("Domain:      {}", h.domain);
                    println!("Deployments: {}", h.active_deployments);
                }
                Err(e) => eprintln!("Error: {e}"),
            }
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
