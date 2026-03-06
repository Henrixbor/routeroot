use axum::{Json, extract::{Path, State}};
use serde::Deserialize;
use std::sync::Arc;

use crate::{AppState, db::{AuditEvent, DnsRecord}, error::AppError};

const PROTECTED_RECORD_TYPES: &[&str] = &["NS", "SOA", "CAA"];
const PROTECTED_RECORD_NAMES: &[&str] = &["@", "ns1", "ns2"];

#[derive(Deserialize)]
pub struct CreateRecordRequest {
    pub name: String,
    pub record_type: Option<String>,
    pub value: String,
}

pub async fn create_record(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRecordRequest>,
) -> Result<Json<DnsRecord>, AppError> {
    let record_type = req.record_type.unwrap_or_else(|| "A".into());

    // Block protected record types
    if PROTECTED_RECORD_TYPES.iter().any(|t| t.eq_ignore_ascii_case(&record_type)) {
        return Err(AppError::BadRequest(format!(
            "cannot create {record_type} records — NS, SOA, and CAA records are protected"
        )));
    }

    // Block protected names
    if PROTECTED_RECORD_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&req.name)) {
        return Err(AppError::BadRequest(format!(
            "cannot modify record '{}' — this name is protected", req.name
        )));
    }

    // Validate record name format
    if !req.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '*') {
        return Err(AppError::BadRequest(
            "invalid record name: only alphanumeric, hyphens, dots, underscores, and * allowed".into()
        ));
    }
    if req.name.len() > 253 {
        return Err(AppError::BadRequest("record name too long (max 253 chars)".into()));
    }

    // Validate record value — no zone file injection characters
    if req.value.contains('\n') || req.value.contains('\r') || req.value.contains(';')
        || req.value.contains('(') || req.value.contains(')') {
        return Err(AppError::BadRequest(
            "invalid record value: contains forbidden characters".into()
        ));
    }
    if req.value.len() > 4096 {
        return Err(AppError::BadRequest("record value too long (max 4096 chars)".into()));
    }

    // Validate record type
    let valid_types = ["A", "AAAA", "CNAME", "MX", "TXT", "SRV", "PTR"];
    if !valid_types.iter().any(|t| t.eq_ignore_ascii_case(&record_type)) {
        return Err(AppError::BadRequest(format!(
            "unsupported record type '{record_type}'. Allowed: {}", valid_types.join(", ")
        )));
    }

    let record = DnsRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        record_type,
        value: req.value,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state.db.insert_dns_record(&record)?;
    regenerate_zone(&state)?;

    audit(&state, "record_created", "dns_record", &record.name, &serde_json::json!({
        "type": record.record_type, "value": record.value
    }));

    Ok(Json(record))
}

pub async fn list_records(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DnsRecord>>, AppError> {
    Ok(Json(state.db.list_dns_records()?))
}

pub async fn delete_record(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Block protected names from deletion
    if PROTECTED_RECORD_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
        return Err(AppError::BadRequest(format!(
            "cannot delete record '{}' — this name is protected", name
        )));
    }

    state.db.delete_dns_record(&name)?;
    regenerate_zone(&state)?;
    audit(&state, "record_deleted", "dns_record", &name, &serde_json::json!({}));

    Ok(Json(serde_json::json!({ "deleted": name })))
}

fn regenerate_zone(state: &AppState) -> Result<(), AppError> {
    let records = state.db.list_dns_records()?;
    let tuples: Vec<(String, String, String)> = records
        .into_iter()
        .map(|r| (r.name, r.record_type, r.value))
        .collect();
    state.dns.write_zone(&tuples)
}

fn audit(state: &AppState, action: &str, resource_type: &str, resource_name: &str, details: &serde_json::Value) {
    let event = AuditEvent {
        id: uuid::Uuid::new_v4().to_string(),
        action: action.to_string(),
        resource_type: resource_type.to_string(),
        resource_name: resource_name.to_string(),
        actor: "api".to_string(),
        details: details.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.insert_audit(&event).ok();
}
