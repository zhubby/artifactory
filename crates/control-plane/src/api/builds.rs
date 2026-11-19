//! [INPUT]: 依赖租户鉴权、构建存储与私有制品存储。
//! [OUTPUT]: 提供构建、规划、发布和制品下载接口。
//! [POS]: HTTP 输入边界，权限检查后调用业务与存储层。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md

use crate::auth::TenantState;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, Query},
    http::HeaderMap,
    response::{IntoResponse, Redirect},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize, Default)]
pub struct Filter {
    pub project_id: Option<Uuid>,
}

fn read_scope(auth: &Auth) -> Result<()> {
    if auth.token_projects.is_some() && !auth.token_scopes.iter().any(|scope| scope == "read") {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

pub async fn list(
    TenantState(state): TenantState,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<dto::BuildSummary>>> {
    read_scope(&auth)?;
    if let Some(project) = filter.project_id {
        auth.project(project, "read")?;
    }
    Ok(Json(
        state
            .store
            .builds(filter.project_id, auth.visible_projects())
            .await?,
    ))
}

pub async fn project_list(
    TenantState(state): TenantState,
    auth: Auth,
    Path(project): Path<Uuid>,
) -> Result<Json<Vec<dto::BuildSummary>>> {
    auth.project(project, "read")?;
    Ok(Json(
        state
            .store
            .builds(Some(project), auth.visible_projects())
            .await?,
    ))
}

pub async fn build_detail(state: &AppState, id: Uuid) -> Result<Value> {
    let mut build =
        serde_json::to_value(state.store.build_detail(id).await?).map_err(anyhow::Error::from)?;
    build["source_manifest"] = if let Some(snapshot) = build["source_manifest_id"].as_str() {
        let id = Uuid::parse_str(snapshot).map_err(anyhow::Error::from)?;
        serde_json::to_value(state.store.source_manifest(id).await?).map_err(anyhow::Error::from)?
    } else {
        Value::Null
    };
    Ok(build)
}

pub async fn detail(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(state.store.build_project(id).await?, "read")?;
    Ok(Json(build_detail(&state, id).await?))
}

pub async fn resolve(
    TenantState(state): TenantState,
    auth: Auth,
    Path((slug, number)): Path<(String, i64)>,
) -> Result<Json<Value>> {
    let project = state.store.project_by_slug(&slug).await?;
    auth.project(project, "read")?;
    if !(1..=9_007_199_254_740_991).contains(&number) {
        return Err(ApiError::NotFound);
    }
    let id = state.store.build_by_number(project, number).await?;
    Ok(Json(build_detail(&state, id).await?))
}

pub async fn cancel(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(state.store.build_project(id).await?, "build")?;
    state.store.cancel_build(id).await?;
    state
        .audit(auth.user_id, "build_canceled", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn retry(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    state.store.retry_job(id, &auth).await?;
    state
        .audit(auth.user_id, "job_retry_requested", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn artifacts(
    TenantState(state): TenantState,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<dto::ArtifactSummary>>> {
    read_scope(&auth)?;
    if let Some(project) = filter.project_id {
        auth.project(project, "read")?;
    }
    Ok(Json(
        state
            .store
            .artifacts(filter.project_id, auth.visible_projects())
            .await?,
    ))
}

pub async fn download(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<axum::response::Response> {
    let artifact = state.store.artifact_download(id).await?;
    auth.project(artifact.project_id, "read")?;
    state
        .audit(auth.user_id, "artifact_downloaded", id, json!({}))
        .await?;
    let url = match artifact.delivery_filename {
        Some(name) => state
            .storage
            .delivery_download_url(&artifact.object_key, &name)?,
        None => state.storage.download_url(&artifact.object_key)?,
    };
    if headers
        .get("accept")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|s| s.contains("application/json"))
    {
        return Ok(Json(json!({"url":url})).into_response());
    }
    Ok(Redirect::temporary(&url).into_response())
}

pub async fn releases(
    TenantState(state): TenantState,
    auth: Auth,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<dto::ReleaseSummary>>> {
    read_scope(&auth)?;
    if let Some(project) = filter.project_id {
        auth.project(project, "read")?;
    }
    Ok(Json(
        state
            .store
            .releases(filter.project_id, auth.visible_projects())
            .await?,
    ))
}

pub async fn release_detail(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut release =
        serde_json::to_value(state.store.release(id).await?).map_err(anyhow::Error::from)?;
    auth.project(
        Uuid::parse_str(release["project_id"].as_str().unwrap()).unwrap(),
        "read",
    )?;
    release["artifacts"] = json!(state.store.release_artifacts(id).await?);
    release["images"] = json!(state.store.release_images(id).await?);
    release["image_registry"] = json!(std::env::var("ARTIFACTORY_REGISTRY_URL").ok());
    Ok(Json(release))
}
