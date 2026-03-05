use rusqlite::{Connection, params};
use serde::Serialize;
use std::sync::Mutex;

use crate::error::AppError;

pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Deployment {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub branch: String,
    pub container_id: Option<String>,
    pub port: Option<u16>,
    pub status: String,
    pub url: String,
    pub created_at: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DnsRecord {
    pub id: String,
    pub name: String,
    pub record_type: String,
    pub value: String,
    pub created_at: String,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, AppError> {
        let conn = Connection::open(path).map_err(|e| AppError::Internal(e.to_string()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn migrate(&self) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS deployments (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                repo TEXT NOT NULL,
                branch TEXT NOT NULL,
                container_id TEXT,
                port INTEGER,
                status TEXT NOT NULL DEFAULT 'building',
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
            );"
        )?;
        Ok(())
    }

    pub fn insert_deployment(&self, d: &Deployment) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO deployments (id, name, repo, branch, container_id, port, status, url, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![d.id, d.name, d.repo, d.branch, d.container_id, d.port, d.status, d.url, d.created_at, d.expires_at],
        )?;
        Ok(())
    }

    pub fn update_deployment_status(&self, name: &str, status: &str, container_id: Option<&str>, port: Option<u16>) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE deployments SET status = ?1, container_id = ?2, port = ?3 WHERE name = ?4",
            params![status, container_id, port, name],
        )?;
        Ok(())
    }

    pub fn get_deployment(&self, name: &str) -> Result<Option<Deployment>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, url, created_at, expires_at FROM deployments WHERE name = ?1"
        )?;
        let mut rows = stmt.query_map(params![name], |row| {
            Ok(Deployment {
                id: row.get(0)?,
                name: row.get(1)?,
                repo: row.get(2)?,
                branch: row.get(3)?,
                container_id: row.get(4)?,
                port: row.get(5)?,
                status: row.get(6)?,
                url: row.get(7)?,
                created_at: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn list_deployments(&self) -> Result<Vec<Deployment>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, url, created_at, expires_at FROM deployments ORDER BY created_at DESC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Deployment {
                id: row.get(0)?,
                name: row.get(1)?,
                repo: row.get(2)?,
                branch: row.get(3)?,
                container_id: row.get(4)?,
                port: row.get(5)?,
                status: row.get(6)?,
                url: row.get(7)?,
                created_at: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn delete_deployment(&self, name: &str) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM deployments WHERE name = ?1", params![name])?;
        Ok(())
    }

    pub fn count_active_deployments(&self) -> Result<usize, AppError> {
        let conn = self.conn.lock().unwrap();
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM deployments WHERE status != 'stopped'",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn get_expired_deployments(&self, now: &str) -> Result<Vec<Deployment>, AppError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, repo, branch, container_id, port, status, url, created_at, expires_at
             FROM deployments WHERE expires_at IS NOT NULL AND expires_at < ?1 AND status != 'stopped'"
        )?;
        let rows = stmt.query_map(params![now], |row| {
            Ok(Deployment {
                id: row.get(0)?,
                name: row.get(1)?,
                repo: row.get(2)?,
                branch: row.get(3)?,
                container_id: row.get(4)?,
                port: row.get(5)?,
                status: row.get(6)?,
                url: row.get(7)?,
                created_at: row.get(8)?,
                expires_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| AppError::Internal(e.to_string()))
    }

    pub fn insert_dns_record(&self, r: &DnsRecord) -> Result<(), AppError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO dns_records (id, name, record_type, value, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![r.id, r.name, r.record_type, r.value, r.created_at],
        )?;
        Ok(())
    }

    pub fn list_dns_records(&self) -> Result<Vec<DnsRecord>, AppError> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM dns_records WHERE name = ?1", params![name])?;
        Ok(())
    }
}
