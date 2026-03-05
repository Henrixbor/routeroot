use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Parser)]
#[command(name = "agentdns", about = "AgentDNS CLI — deploy branches as live URLs")]
struct Cli {
    /// API server URL
    #[arg(long, env = "AGENTDNS_URL", default_value = "http://localhost:8053")]
    server: String,

    /// API key
    #[arg(long, env = "AGENTDNS_API_KEY", default_value = "dev-key")]
    key: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy a repo branch
    Deploy {
        /// Git repo URL
        repo: String,
        /// Branch name
        #[arg(short, long, default_value = "main")]
        branch: String,
        /// Custom subdomain name
        #[arg(short, long)]
        name: Option<String>,
        /// Time-to-live (e.g. 24h, 7d)
        #[arg(short, long)]
        ttl: Option<String>,
    },
    /// List active deployments
    Ls,
    /// Get deployment details
    Status {
        /// Deployment name
        name: String,
    },
    /// Get deployment logs
    Logs {
        /// Deployment name
        name: String,
    },
    /// Tear down a deployment
    Down {
        /// Deployment name
        name: String,
    },
    /// Manage DNS records
    Record {
        #[command(subcommand)]
        action: RecordAction,
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
}

#[derive(Deserialize)]
struct Deployment {
    name: String,
    repo: String,
    branch: String,
    status: String,
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

fn main() {
    let cli = Cli::parse();
    let client = reqwest::blocking::Client::new();
    let base = cli.server.trim_end_matches('/');
    let auth = format!("Bearer {}", cli.key);

    match cli.command {
        Commands::Deploy { repo, branch, name, ttl } => {
            let body = serde_json::json!({
                "repo": repo,
                "branch": branch,
                "name": name,
                "ttl": ttl,
            });
            let resp: DeployResponse = client
                .post(format!("{base}/api/deploy"))
                .header("Authorization", &auth)
                .json(&body)
                .send()
                .expect("request failed")
                .json()
                .expect("invalid response");

            println!("Deploying '{}' ...", resp.name);
            println!("URL: {}", resp.url);
            println!("Status: {}", resp.status);
        }

        Commands::Ls => {
            let deployments: Vec<Deployment> = client
                .get(format!("{base}/api/deployments"))
                .header("Authorization", &auth)
                .send()
                .expect("request failed")
                .json()
                .expect("invalid response");

            if deployments.is_empty() {
                println!("No active deployments.");
                return;
            }

            println!("{:<25} {:<12} {:<15} {:<40}", "NAME", "STATUS", "BRANCH", "URL");
            println!("{}", "-".repeat(92));
            for d in deployments {
                println!("{:<25} {:<12} {:<15} {:<40}", d.name, d.status, d.branch, d.url);
            }
        }

        Commands::Status { name } => {
            let d: Deployment = client
                .get(format!("{base}/api/deployments/{name}"))
                .header("Authorization", &auth)
                .send()
                .expect("request failed")
                .json()
                .expect("invalid response");

            println!("Name:    {}", d.name);
            println!("Repo:    {}", d.repo);
            println!("Branch:  {}", d.branch);
            println!("Status:  {}", d.status);
            println!("URL:     {}", d.url);
            println!("Created: {}", d.created_at);
        }

        Commands::Logs { name } => {
            let logs: Vec<String> = client
                .get(format!("{base}/api/deployments/{name}/logs"))
                .header("Authorization", &auth)
                .send()
                .expect("request failed")
                .json()
                .expect("invalid response");

            for line in logs {
                println!("{line}");
            }
        }

        Commands::Down { name } => {
            client
                .delete(format!("{base}/api/deploy/{name}"))
                .header("Authorization", &auth)
                .send()
                .expect("request failed");

            println!("Deployment '{name}' torn down.");
        }

        Commands::Record { action } => match action {
            RecordAction::Add { name, record_type, value } => {
                let body = serde_json::json!({
                    "name": name,
                    "record_type": record_type,
                    "value": value,
                });
                client
                    .post(format!("{base}/api/records"))
                    .header("Authorization", &auth)
                    .json(&body)
                    .send()
                    .expect("request failed");

                println!("Record added: {name} {record_type} {value}");
            }
            RecordAction::Ls => {
                let records: Vec<DnsRecord> = client
                    .get(format!("{base}/api/records"))
                    .header("Authorization", &auth)
                    .send()
                    .expect("request failed")
                    .json()
                    .expect("invalid response");

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
            RecordAction::Rm { name } => {
                client
                    .delete(format!("{base}/api/records/{name}"))
                    .header("Authorization", &auth)
                    .send()
                    .expect("request failed");

                println!("Record '{name}' deleted.");
            }
        },

        Commands::Health => {
            let h: HealthResponse = client
                .get(format!("{base}/api/health"))
                .send()
                .expect("request failed")
                .json()
                .expect("invalid response");

            println!("Status:      {}", h.status);
            println!("Domain:      {}", h.domain);
            println!("Deployments: {}", h.active_deployments);
        }
    }
}
