//! [INPUT]: 依赖账户会话、租户项目授权和完整窗口构建聚合。
//! [OUTPUT]: 提供租户与单项目概览的只读统计接口。
//! [POS]: 查询参数先校验，再由受权存储计算自然日统计。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{Auth, TenantState},
    error::Result,
    store::dto,
};
use axum::{Json, extract::Query};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardQuery {
    #[serde(default = "default_days")]
    days: u32,
    #[serde(default = "default_timezone")]
    timezone: String,
    project_id: Option<Uuid>,
}

fn default_days() -> u32 {
    30
}

fn default_timezone() -> String {
    "UTC".into()
}

pub async fn dashboard(
    TenantState(state): TenantState,
    auth: Auth,
    Query(query): Query<DashboardQuery>,
) -> Result<Json<dto::Dashboard>> {
    auth.session_role("viewer")?;
    if let Some(project) = query.project_id {
        auth.project(project, "read")?;
    }
    Ok(Json(
        state
            .store
            .dashboard(query.project_id, query.days, &query.timezone)
            .await?,
    ))
}
