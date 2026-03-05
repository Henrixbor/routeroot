use axum::{Json, extract::{Path, State}};
use serde::Deserialize;
use std::sync::Arc;

use crate::{AppState, db::DnsRecord, error::AppError};

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
    let record = DnsRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name,
        record_type: req.record_type.unwrap_or_else(|| "A".into()),
        value: req.value,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    state.db.insert_dns_record(&record)?;

    // Regenerate zone file with all records
    regenerate_zone(&state)?;

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
    state.db.delete_dns_record(&name)?;
    regenerate_zone(&state)?;
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
