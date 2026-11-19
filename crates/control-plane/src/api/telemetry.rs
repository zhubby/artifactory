//! [INPUT]: 租户权限、节点身份与私有遥测查询服务。
//! [OUTPUT]: 提供设备最新遥测、历史和任务观测查询。
//! [POS]: HTTP 权限边界；用服务令牌查询遥测后端。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::auth::TenantState;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    state::AppState,
};
use artifactory_service_transport::service;
use axum::{
    Json,
    extract::{Path, Query},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    #[serde(default = "minute")]
    resolution: String,
}
fn minute() -> String {
    "minute".into()
}
impl Range {
    fn pairs(&self) -> Result<Vec<(&'static str, String)>> {
        if !["raw", "minute", "hour"].contains(&self.resolution.as_str()) {
            return Err(ApiError::BadRequest("invalid telemetry resolution".into()));
        }
        Ok(vec![
            ("from", self.from.to_rfc3339()),
            ("to", self.to.to_rfc3339()),
            ("resolution", self.resolution.clone()),
        ])
    }
}
async fn fetch(state: &AppState, path: &str, range: Option<&Range>) -> Result<Json<Value>> {
    let cfg = state
        .settings
        .telemetry
        .as_ref()
        .ok_or_else(|| ApiError::BadGateway("telemetry service is not configured".into()))?;
    let ca = tokio::fs::read(state.settings.pki_dir.join("ca.pem"))
        .await
        .map_err(anyhow::Error::from)?;
    let client = reqwest::Client::builder()
        .add_root_certificate(reqwest::Certificate::from_pem(&ca).map_err(anyhow::Error::from)?)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(anyhow::Error::from)?;
    let mut request = client
        .get(format!("{}{path}", cfg.query_url.trim_end_matches('/')))
        .bearer_auth(&cfg.token)
        .timeout(std::time::Duration::from_secs(8));
    if let Some(range) = range {
        request = request.query(&range.pairs()?);
    }
    let response = request
        .send()
        .await
        .map_err(|_| ApiError::BadGateway("telemetry service unavailable".into()))?;
    if response.status() == reqwest::StatusCode::BAD_REQUEST {
        return Err(ApiError::BadRequest(
            "invalid or expired telemetry range; raw <=6h, minute <=2d, hour <=31d".into(),
        ));
    }
    if !response.status().is_success() {
        return Err(ApiError::BadGateway("telemetry service unavailable".into()));
    }
    Ok(Json(service::decode(response, 4 * 1024 * 1024).await?))
}
async fn node(state: &AppState, auth: &Auth, id: Uuid) -> Result<()> {
    auth.session_role("viewer")?;
    let exists = state.store.node_exists(id).await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    Ok(())
}
async fn attempt(state: &AppState, auth: &Auth, id: Uuid) -> Result<()> {
    let project = state.store.attempt_project(id).await?;
    auth.project(project, "read")?;
    Ok(())
}
pub async fn latest(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    node(&s, &auth, id).await?;
    fetch(&s, &format!("/v1/nodes/{id}/latest"), None).await
}
pub async fn inventory(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    node(&s, &auth, id).await?;
    fetch(&s, &format!("/v1/nodes/{id}/inventory"), None).await
}
pub async fn history(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(r): Query<Range>,
) -> Result<Json<Value>> {
    node(&s, &auth, id).await?;
    fetch(&s, &format!("/v1/nodes/{id}/history"), Some(&r)).await
}
pub async fn build_history(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(r): Query<Range>,
) -> Result<Json<Value>> {
    attempt(&s, &auth, id).await?;
    fetch(&s, &format!("/v1/attempts/{id}/history"), Some(&r)).await
}
pub async fn processes(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(r): Query<Range>,
) -> Result<Json<Value>> {
    attempt(&s, &auth, id).await?;
    fetch(&s, &format!("/v1/attempts/{id}/processes"), Some(&r)).await
}
