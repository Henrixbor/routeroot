use std::fs;
use crate::error::AppError;

pub struct DnsService {
    zone_file_dir: String,
    domains: Vec<String>,
    server_ip: String,
}

impl DnsService {
    pub fn new(zone_file_dir: &str, domains: &[String], server_ip: &str) -> Self {
        Self {
            zone_file_dir: zone_file_dir.to_string(),
            domains: domains.to_vec(),
            server_ip: server_ip.to_string(),
        }
    }

    /// Regenerate zone files for all managed domains with current records.
    /// CoreDNS auto-reloads on file change (5s interval).
    pub fn write_zone(&self, extra_records: &[(String, String, String)]) -> Result<(), AppError> {
        for domain in &self.domains {
            self.write_zone_for_domain(domain, extra_records)?;
        }
        Ok(())
    }

    fn write_zone_for_domain(&self, domain: &str, extra_records: &[(String, String, String)]) -> Result<(), AppError> {
        let serial = chrono::Utc::now().format("%Y%m%d%H").to_string();

        let mut zone = format!(
            "$ORIGIN {domain}.
$TTL 300

@       IN SOA  ns1.{domain}. admin.{domain}. (
                {serial}   ; serial
                3600        ; refresh
                900         ; retry
                604800      ; expire
                300         ; minimum TTL
)

@       IN NS   ns1.{domain}.
@       IN NS   ns2.{domain}.

ns1     IN A    {ip}
ns2     IN A    {ip}

@       IN A    {ip}

; Wildcard — all subdomains resolve to this server
*       IN A    {ip}

; Custom records
",
            domain = domain,
            ip = self.server_ip,
            serial = serial,
        );

        for (name, record_type, value) in extra_records {
            if !Self::is_valid_dns_name(name) {
                tracing::warn!("Skipping invalid DNS record name: {name}");
                continue;
            }
            if !Self::is_valid_record_type(record_type) {
                tracing::warn!("Skipping invalid DNS record type: {record_type}");
                continue;
            }
            if !Self::is_valid_record_value(value) {
                tracing::warn!("Skipping invalid DNS record value: {value}");
                continue;
            }
            zone.push_str(&format!("{name}    IN {record_type}    {value}\n"));
        }

        let zone_file_path = format!("{}/db.{}", self.zone_file_dir, domain);
        fs::write(&zone_file_path, zone)
            .map_err(|e| AppError::Internal(format!("failed to write zone file {zone_file_path}: {e}")))?;

        tracing::info!("Zone file updated for {domain} with {} custom records", extra_records.len());
        Ok(())
    }

    /// Validate DNS record name: alphanumeric, hyphens, dots, underscores, @, *
    fn is_valid_dns_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 253
            && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '@' || c == '*')
    }

    /// Validate DNS record type: known safe types only
    fn is_valid_record_type(record_type: &str) -> bool {
        matches!(record_type.to_uppercase().as_str(),
            "A" | "AAAA" | "CNAME" | "MX" | "TXT" | "SRV" | "PTR" | "CAA" | "NS" | "SOA"
        )
    }

    /// Validate DNS record value: no newlines, semicolons, or parentheses (zone file metacharacters)
    fn is_valid_record_value(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 4096
            && !value.contains('\n')
            && !value.contains('\r')
            && !value.contains(';')
            && !value.contains('(')
            && !value.contains(')')
    }
}
