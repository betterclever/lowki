//! Append-only audit log. Callers pass only non-secret metadata (ids, kinds, reasons).

use serde_json::Value;

use crate::{AppState, db::now};

pub async fn record(state: &AppState, user_id: Option<&str>, event: &str, detail: Value) {
    let res = sqlx::query("INSERT INTO audit_log (id, at, user_id, event, detail) VALUES ($1, $2, $3, $4, $5)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(now())
        .bind(user_id)
        .bind(event)
        .bind(detail.to_string())
        .execute(&state.db)
        .await;
    if let Err(e) = res {
        tracing::error!(error = %e, event, "failed to write audit log");
    }
    tracing::info!(target: "lowki::audit", event, user = user_id.unwrap_or("-"), "audit");
}
