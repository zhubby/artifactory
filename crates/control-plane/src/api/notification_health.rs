//! [INPUT]: 依赖租户管理员会话和通知队列状态。
//! [OUTPUT]: 提供租户隔离的通知健康摘要和 Prometheus 指标。
//! [POS]: 通知运维读取边界；指标不包含业务身份标签或正文。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{auth::Auth, error::Result, state::AppState};
use axum::{Json, extract::State, http::header, response::IntoResponse};
use serde_json::Value;

async fn snapshot(state: &AppState, auth: &Auth) -> Result<Value> {
    auth.session_role("admin")?;
    Ok(state.store.notification_health(auth.tenant_id).await?)
}

pub(super) async fn health(State(state): State<AppState>, auth: Auth) -> Result<Json<Value>> {
    Ok(Json(snapshot(&state, &auth).await?))
}

pub(super) async fn metrics(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<impl IntoResponse> {
    let values = snapshot(&state, &auth).await?;
    let mut body = String::new();
    for key in [
        "pending_events",
        "quarantined_events",
        "oldest_event_age_seconds",
        "queued_email",
        "queued_webhook",
        "retrying_deliveries",
        "failed_deliveries",
        "paused_endpoints",
    ] {
        let value = values[key].as_f64().unwrap_or(0.0);
        body.push_str(&format!("# TYPE artifactory_notifications_{key} gauge\nartifactory_notifications_{key} {value}\n"));
    }
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    ))
}
