use std::fs;
use crate::error::AppError;

pub struct DnsService {
    zone_file_path: String,
    domain: String,
    server_ip: String,
}

impl DnsService {
    pub fn new(zone_file_path: &str, domain: &str, server_ip: &str) -> Self {
        Self {
            zone_file_path: zone_file_path.to_string(),
            domain: domain.to_string(),
            server_ip: server_ip.to_string(),
        }
    }

    /// Regenerate the zone file with current records.
    /// CoreDNS auto-reloads on file change (5s interval).
    pub fn write_zone(&self, extra_records: &[(String, String, String)]) -> Result<(), AppError> {
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
            domain = self.domain,
            ip = self.server_ip,
            serial = serial,
        );

        for (name, record_type, value) in extra_records {
            zone.push_str(&format!("{name}    IN {record_type}    {value}\n"));
        }

        fs::write(&self.zone_file_path, zone)
            .map_err(|e| AppError::Internal(format!("failed to write zone file: {e}")))?;

        tracing::info!("Zone file updated with {} custom records", extra_records.len());
        Ok(())
    }
}
