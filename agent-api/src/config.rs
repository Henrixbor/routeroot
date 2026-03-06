use std::env;

#[derive(Clone)]
pub struct Config {
    pub domain: String,
    pub domains: Vec<String>,
    pub server_ip: String,
    pub api_key: String,
    pub max_deployments: usize,
    pub default_ttl_secs: u64,
    pub max_memory_mb: u64,
    pub max_cpus: u64,
    pub caddy_admin_url: String,
    pub github_webhook_secret: Option<String>,
    pub database_path: String,
    pub zone_file_dir: String,
    pub allowed_repo_hosts: Vec<String>,
}

const INSECURE_KEYS: &[&str] = &["dev-key", "change-me", "change-me-to-a-secure-key", "test", ""];

impl Config {
    pub fn from_env() -> Self {
        let api_key = env::var("ROUTEROOT_API_KEY").unwrap_or_default();
        if INSECURE_KEYS.iter().any(|k| api_key.eq_ignore_ascii_case(k)) || api_key.len() < 16 {
            eprintln!("FATAL: ROUTEROOT_API_KEY is missing, too short (min 16 chars), or set to a known insecure default.");
            eprintln!("       Generate one with: openssl rand -hex 32");
            std::process::exit(1);
        }

        let primary_domain = env::var("ROUTEROOT_DOMAIN").unwrap_or_else(|_| "routeroot.dev".into());

        // Multi-domain: ROUTEROOT_DOMAINS=routeroot.dev,vibeyard.io
        // Falls back to single ROUTEROOT_DOMAIN if not set
        let domains: Vec<String> = env::var("ROUTEROOT_DOMAINS")
            .unwrap_or_else(|_| primary_domain.clone())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let allowed_repo_hosts: Vec<String> = env::var("ROUTEROOT_ALLOWED_REPO_HOSTS")
            .unwrap_or_else(|_| "github.com,gitlab.com,bitbucket.org".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            domain: domains.first().cloned().unwrap_or(primary_domain),
            domains,
            server_ip: env::var("ROUTEROOT_SERVER_IP").unwrap_or_else(|_| "127.0.0.1".into()),
            api_key,
            max_deployments: env::var("ROUTEROOT_MAX_DEPLOYMENTS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(20),
            default_ttl_secs: parse_duration_secs(
                &env::var("ROUTEROOT_DEFAULT_TTL").unwrap_or_else(|_| "48h".into())
            ),
            max_memory_mb: env::var("ROUTEROOT_MAX_MEMORY")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(2048),
            max_cpus: env::var("ROUTEROOT_MAX_CPUS")
                .ok().and_then(|v| v.parse().ok()).unwrap_or(2),
            caddy_admin_url: env::var("ROUTEROOT_CADDY_ADMIN")
                .unwrap_or_else(|_| "http://localhost:2019".into()),
            github_webhook_secret: env::var("ROUTEROOT_GITHUB_WEBHOOK_SECRET").ok(),
            database_path: env::var("DATABASE_PATH")
                .unwrap_or_else(|_| "/data/routeroot.db".into()),
            zone_file_dir: env::var("ZONE_FILE_DIR")
                .unwrap_or_else(|_| env::var("ZONE_FILE_PATH")
                    .map(|p| p.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_else(|| "/dns-zones".into()))
                    .unwrap_or_else(|_| "/dns-zones".into())),
            allowed_repo_hosts,
        }
    }

    /// Check if a domain is one of our configured domains
    pub fn is_managed_domain(&self, domain: &str) -> bool {
        self.domains.iter().any(|d| d == domain)
    }

    /// Check if a hostname is a subdomain of any managed domain
    pub fn is_managed_subdomain(&self, hostname: &str) -> bool {
        self.domains.iter().any(|d| hostname.ends_with(&format!(".{d}")))
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
