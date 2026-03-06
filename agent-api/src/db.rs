use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

use crate::error::AppError;

pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub branch: String,
    pub container_id: Option<String>,
    pub port: Option<u16>,
    pub status: String,
    pub verified: Option<String>,
    pub environment: String,
    pub url: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub name: String,
    pub record_type: String,
    pub value: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployPlan {
    pub id: String,
    pub repo: String,
    pub branch: String,
    pub name: String,
    pub environment: String,
    pub url: String,
    pub ttl: Option<String>,
    pub actions: String, // JSON array of planned actions
    pub status: String,  // "pending", "applied", "cancelled"
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomDomain {
    pub id: String,
    pub domain: String,
    pub deployment_name: String,
    pub verified: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub id: String,
    pub action: String,
    pub resource_type: String,
    pub resource_name: String,
    pub actor: String,
    pub details: String,
    pub created_at: String,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, AppError> {
        let conn = Connection::open(path).map_err(|e| AppError::Internal(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn conn_lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AppError> {
        self.conn.lock().map_err(|e| AppError::Internal(format!("db lock poisoned: {e}")))
    }

    pub fn migrate(&self) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS deployments (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                repo TEXT NOT NULL,
                branch TEXT NOT NULL,
                container_id TEXT,
                port INTEGER,
                status TEXT NOT NULL DEFAULT 'building',
                verified TEXT,
                environment TEXT NOT NULL DEFAULT 'preview',
                url TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT
            );
            CREATE TABLE IF NOT EXISTS dns_records (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                record_type TEXT NOT NULL DEFAULT 'A',
                value TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS deploy_plans (
                id TEXT PRIMARY KEY,
                repo TEXT NOT NULL,
                branch TEXT NOT NULL,
                name TEXT NOT NULL,
                environment TEXT NOT NULL DEFAULT 'preview',
                url TEXT NOT NULL,
                ttl TEXT,
                actions TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                action TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_name TEXT NOT NULL,
                actor TEXT NOT NULL DEFAULT 'api',
                details TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS custom_domains (
                id TEXT PRIMARY KEY,
                domain TEXT UNIQUE NOT NULL,
                deployment_name TEXT NOT NULL,
                verified INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY (deployment_name) REFERENCES deployments(name)
            );
            CREATE INDEX IF NOT EXISTS idx_audit_created ON audit_log(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_deployments_status ON deployments(status);
            CREATE INDEX IF NOT EXISTS idx_plans_status ON deploy_plans(status);
            CREATE INDEX IF NOT EXISTS idx_custom_domains_domain ON custom_domains(domain);
            CREATE INDEX IF NOT EXISTS idx_custom_domains_deployment ON custom_domains(deployment_name);
            CREATE TABLE IF NOT EXISTS managed_domains (
                domain TEXT PRIMARY KEY,
                server_ip TEXT NOT NULL,
                created_at TEXT NOT NULL
            );"
        )?;
        Ok(())
    }

    // --- Deployments ---

    pub fn insert_deployment(&self, d: &Deployment) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO deployments (id, name, repo, branch, container_id, port, status, verified, environment, url, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![d.id, d.name, d.repo, d.branch, d.container_id, d.port, d.status, d.verified, d.environment, d.url, d.created_at, d.expires_at],
        )?;
        Ok(())
    }

    pub fn update_deployment_status(&self, name: &str, status: &str, container_id: Option<&str>, port: Option<u16>) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "UPDATE deployments SET status = ?1, container_id = ?2, port = ?3 WHERE name = ?4",
            params![status, container_id, port, name],
        )?;
        Ok(())
    }

    pub fn update_deployment_verified(&self, name: &str, verified: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "UPDATE deployments SET verified = ?1 WHERE name = ?2",
            params![verified, name],
        )?;
        Ok(())
    }

    pub fn clear_deployment_expiry(&self, name: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute("UPDATE deployments SET expires_at = NULL WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn update_deployment_environment(&self, name: &str, environment: &str, new_url: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "UPDATE deployments SET environment = ?1, url = ?2 WHERE name = ?3",
            params![environment, new_url, name],
        )?;
        Ok(())
    }

    fn row_to_deployment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Deployment> {
        Ok(Deployment {
            id: row.get(0)?,
            name: row.get(1)?,
            repo: row.get(2)?,
            branch: row.get(3)?,
            container_id: row.get(4)?,
            port: row.get(5)?,
            status: row.get(6)?,
            verified: row.get(7)?,
            environment: row.get(8)?,
            url: row.get(9)?,
            created_at: row.get(10)?,
            expires_at: row.get(11)?,
        })
    }

    pub fn get_deployment(&self, name: &str) -> Result<Option<Deployment>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, verified, environment, url, created_at, expires_at FROM deployments WHERE name = ?1"
        )?;
        let mut rows = stmt.query_map(params![name], Self::row_to_deployment)?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_deployments(&self) -> Result<Vec<Deployment>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, verified, environment, url, created_at, expires_at FROM deployments ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], Self::row_to_deployment)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn delete_deployment(&self, name: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute("DELETE FROM deployments WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn count_active_deployments(&self) -> Result<usize, AppError> {
        let conn = self.conn_lock()?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM deployments WHERE status NOT IN ('stopped', 'failed')",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn is_port_in_use(&self, port: u16) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM deployments WHERE port = ?1 AND status NOT IN ('stopped', 'failed')",
            params![port],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn get_expired_deployments(&self, now: &str) -> Result<Vec<Deployment>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, verified, environment, url, created_at, expires_at
             FROM deployments WHERE expires_at IS NOT NULL AND expires_at < ?1 AND status NOT IN ('stopped', 'failed')"
        )?;
        let rows = stmt.query_map(params![now], Self::row_to_deployment)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    // --- DNS Records ---

    pub fn insert_dns_record(&self, r: &DnsRecord) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO dns_records (id, name, record_type, value, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![r.id, r.name, r.record_type, r.value, r.created_at],
        )?;
        Ok(())
    }

    pub fn list_dns_records(&self) -> Result<Vec<DnsRecord>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare("SELECT id, name, record_type, value, created_at FROM dns_records ORDER BY name")?;
        let rows = stmt.query_map([], |row| {
            Ok(DnsRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                record_type: row.get(2)?,
                value: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn delete_dns_record(&self, name: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute("DELETE FROM dns_records WHERE name = ?1", params![name])?;
        Ok(())
    }

    // --- Deploy Plans ---

    pub fn insert_plan(&self, p: &DeployPlan) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO deploy_plans (id, repo, branch, name, environment, url, ttl, actions, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![p.id, p.repo, p.branch, p.name, p.environment, p.url, p.ttl, p.actions, p.status, p.created_at],
        )?;
        Ok(())
    }

    pub fn get_plan(&self, id: &str) -> Result<Option<DeployPlan>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, repo, branch, name, environment, url, ttl, actions, status, created_at FROM deploy_plans WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(DeployPlan {
                id: row.get(0)?,
                repo: row.get(1)?,
                branch: row.get(2)?,
                name: row.get(3)?,
                environment: row.get(4)?,
                url: row.get(5)?,
                ttl: row.get(6)?,
                actions: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn update_plan_status(&self, id: &str, status: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute("UPDATE deploy_plans SET status = ?1 WHERE id = ?2", params![status, id])?;
        Ok(())
    }

    pub fn list_plans(&self) -> Result<Vec<DeployPlan>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, repo, branch, name, environment, url, ttl, actions, status, created_at FROM deploy_plans ORDER BY created_at DESC LIMIT 50"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DeployPlan {
                id: row.get(0)?,
                repo: row.get(1)?,
                branch: row.get(2)?,
                name: row.get(3)?,
                environment: row.get(4)?,
                url: row.get(5)?,
                ttl: row.get(6)?,
                actions: row.get(7)?,
                status: row.get(8)?,
                created_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    // --- Custom Domains ---

    pub fn insert_custom_domain(&self, d: &CustomDomain) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO custom_domains (id, domain, deployment_name, verified, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![d.id, d.domain, d.deployment_name, d.verified, d.created_at],
        )?;
        Ok(())
    }

    pub fn get_custom_domain(&self, domain: &str) -> Result<Option<CustomDomain>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, domain, deployment_name, verified, created_at FROM custom_domains WHERE domain = ?1"
        )?;
        let mut rows = stmt.query_map(params![domain], |row| {
            Ok(CustomDomain {
                id: row.get(0)?,
                domain: row.get(1)?,
                deployment_name: row.get(2)?,
                verified: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_custom_domains(&self) -> Result<Vec<CustomDomain>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, domain, deployment_name, verified, created_at FROM custom_domains ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(CustomDomain {
                id: row.get(0)?,
                domain: row.get(1)?,
                deployment_name: row.get(2)?,
                verified: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn list_custom_domains_for_deployment(&self, deployment_name: &str) -> Result<Vec<CustomDomain>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, domain, deployment_name, verified, created_at FROM custom_domains WHERE deployment_name = ?1"
        )?;
        let rows = stmt.query_map(params![deployment_name], |row| {
            Ok(CustomDomain {
                id: row.get(0)?,
                domain: row.get(1)?,
                deployment_name: row.get(2)?,
                verified: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn delete_custom_domain(&self, domain: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute("DELETE FROM custom_domains WHERE domain = ?1", params![domain])?;
        Ok(())
    }

    pub fn is_custom_domain(&self, domain: &str) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM custom_domains WHERE domain = ?1",
            params![domain],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // --- Managed Domains ---

    pub fn insert_managed_domain(&self, domain: &str, server_ip: &str) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO managed_domains (domain, server_ip, created_at) VALUES (?1, ?2, ?3)",
            params![domain, server_ip, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn list_managed_domains(&self) -> Result<Vec<(String, String, String)>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare("SELECT domain, server_ip, created_at FROM managed_domains ORDER BY created_at")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn delete_managed_domain(&self, domain: &str) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        let changed = conn.execute("DELETE FROM managed_domains WHERE domain = ?1", params![domain])?;
        Ok(changed > 0)
    }

    pub fn is_managed_domain_in_db(&self, domain: &str) -> Result<bool, AppError> {
        let conn = self.conn_lock()?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM managed_domains WHERE domain = ?1",
            params![domain],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // --- Audit Log ---

    pub fn insert_audit(&self, event: &AuditEvent) -> Result<(), AppError> {
        let conn = self.conn_lock()?;
        conn.execute(
            "INSERT INTO audit_log (id, action, resource_type, resource_name, actor, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event.id, event.action, event.resource_type, event.resource_name, event.actor, event.details, event.created_at],
        )?;
        Ok(())
    }

    pub fn list_audit(&self, limit: usize) -> Result<Vec<AuditEvent>, AppError> {
        let conn = self.conn_lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, action, resource_type, resource_name, actor, details, created_at FROM audit_log ORDER BY created_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(AuditEvent {
                id: row.get(0)?,
                action: row.get(1)?,
                resource_type: row.get(2)?,
                resource_name: row.get(3)?,
                actor: row.get(4)?,
                details: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }
}
