use std::env;

#[derive(Clone)]
pub struct Config {
    pub domain: String,
    pub server_ip: String,
    pub api_key: String,
    pub max_deployments: usize,
    pub default_ttl_secs: u64,
    pub max_memory_mb: u64,
    pub max_cpus: u64,
    pub caddy_admin_url: String,
    pub github_webhook_secret: Option<String>,
    pub database_path: String,
    pub zone_file_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            domain: env::var("AGENTDNS_DOMAIN").unwrap_or_else(|_| "agentdns.dev".into()),
            server_ip: env::var("AGENTDNS_SERVER_IP").unwrap_or_else(|_| "127.0.0.1".into()),
            api_key: env::var("AGENTDNS_API_KEY").unwrap_or_else(|_| "dev-key".into()),
            max_deployments: env::var("AGENTDNS_MAX_DEPLOYMENTS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(20),
            default_ttl_secs: parse_duration_secs(
                &env::var("AGENTDNS_DEFAULT_TTL").unwrap_or_else(|_| "48h".into())
            ),
            max_memory_mb: env::var("AGENTDNS_MAX_MEMORY")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(2048),
            max_cpus: env::var("AGENTDNS_MAX_CPUS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(2),
            caddy_admin_url: env::var("AGENTDNS_CADDY_ADMIN")
                .unwrap_or_else(|_| "http://localhost:2019".into()),
            github_webhook_secret: env::var("AGENTDNS_GITHUB_WEBHOOK_SECRET").ok(),
            database_path: env::var("DATABASE_PATH")
                .unwrap_or_else(|_| "/data/agentdns.db".into()),
            zone_file_path: env::var("ZONE_FILE_PATH")
                .unwrap_or_else(|_| "/dns-zones/db.agentdns.dev".into()),
        }
    }
}

fn parse_duration_secs(s: &str) -> u64 {
    let s = s.trim();
    if let Some(h) = s.strip_suffix('h') {
        h.parse::<u64>().unwrap_or(48) * 3600
    } else if let Some(m) = s.strip_suffix('m') {
        m.parse::<u64>().unwrap_or(2880) * 60
    } else if let Some(d) = s.strip_suffix('d') {
        d.parse::<u64>().unwrap_or(2) * 86400
    } else {
        s.parse::<u64>().unwrap_or(172800)
    }
}
