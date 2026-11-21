//! [INPUT]: 依赖租户鉴权、项目存储与源码引用查询。
//! [OUTPUT]: 提供项目查询、配置写入、原生平台和仓库引用接口。
//! [POS]: 项目 HTTP 权限边界；签名默认方案由 signing 接口写入。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::auth::TenantState;
pub use crate::store::ProjectInput;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    source,
    state::AppState,
};
use axum::{Json, extract::Path};
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn list(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::ProjectSummary>>> {
    if auth.token_projects.is_some() && !auth.token_scopes.iter().any(|scope| scope == "read") {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(state.store.projects(auth.visible_projects()).await?))
}

pub async fn detail(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<dto::Project>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.project(id).await?))
}

pub async fn resolve(
    TenantState(state): TenantState,
    auth: Auth,
    Path(slug): Path<String>,
) -> Result<Json<dto::Project>> {
    let id = state.store.project_by_slug(&slug).await?;
    auth.project(id, "read")?;
    Ok(Json(state.store.project(id).await?))
}

pub async fn refs(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(id, "read")?;
    let lease = state
        .store
        .begin_workspace_task("source_refs", Some(id))
        .await?;
    let result = crate::source_tasks::refs(&state, id).await;
    state.store.end_workspace_task(lease).await?;
    Ok(Json(result.map_err(|_| {
        ApiError::BadGateway("repository reference query failed; check access and retry".into())
    })?))
}

pub async fn build_platforms(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.project_build_platforms(id).await?))
}

async fn validate(_state: &AppState, input: &ProjectInput) -> Result<()> {
    if input.name.is_empty()
        || input.name.len() > 100
        || input.signing_profiles.len() > 32
        || input.build_context.len() > 16000
        || input.source_credentials.len() > 128
    {
        return Err(ApiError::BadRequest("invalid project settings".into()));
    }
    source::validate_repository(&input.repository_url)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    for binding in &input.source_credentials {
        source::validate_repository(&binding.repository_url)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    Ok(())
}

pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    validate(&state, &input).await?;
    let id = Uuid::new_v4();
    state.store.create_project(id, &input).await?;
    let slug = state.store.project(id).await?.slug;
    state
        .audit(auth.user_id, "project_created", id, json!({}))
        .await?;
    Ok(Json(json!({"id":id,"slug":slug})))
}

pub async fn update(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<ProjectInput>,
) -> Result<Json<Value>> {
    auth.project(id, "admin")?;
    validate(&state, &input).await?;
    state.store.update_project(id, &input).await?;
    state
        .audit(auth.user_id, "project_updated", id, json!({}))
        .await?;
    Ok(Json(json!({"id":id})))
}
