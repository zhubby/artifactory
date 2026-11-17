//! [INPUT]: 依赖租户鉴权、构建存储与私有制品存储。
//! [OUTPUT]: 提供构建、规划、发布和制品下载接口。
//! [POS]: HTTP 输入边界，权限检查后调用业务与存储层。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md

use crate::auth::TenantState;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    scheduler,
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

pub use crate::store::BuildInput;

pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Path(project): Path<Uuid>,
    Json(mut input): Json<BuildInput>,
) -> Result<Json<Value>> {
    auth.project(project, "build")?;
    if input.release_version.is_some() {
        auth.project(project, "release")?;
    }
    if input.reference.is_empty()
        || input.reference.len() > 255
        || input.reference.starts_with('-')
        || input.reference.contains(['\0', '\n', '\r'])
        || !input.parameters.is_object()
        || input.parameters.to_string().len() > 16384
        || input.targets.len() > 32
    {
        return Err(ApiError::BadRequest("invalid build request".into()));
    }
    if let Some(version) = &input.release_version {
        crate::build::validate_release_version(version)?;
    }
    let goal = input.goal.take().unwrap_or_else(|| {
        "Build and verify this project, then collect its distributable artifacts.".into()
    });
    if goal.trim().is_empty() || goal.len() > 16000 {
        return Err(ApiError::BadRequest(
            "a build goal of 1..16000 bytes is required".into(),
        ));
    }
    let requirements = input
        .target_requirements
        .as_array()
        .ok_or_else(|| ApiError::BadRequest("target_requirements must be an array".into()))?;
    if requirements.len() > 32
        || requirements.iter().any(|r| {
            !r.is_object()
                || r.as_object()
                    .unwrap()
                    .keys()
                    .any(|k| k != "os" && k != "arch")
                || !["linux", "darwin", "windows"].contains(&r["os"].as_str().unwrap_or(""))
                || !["amd64", "arm64"].contains(&r["arch"].as_str().unwrap_or(""))
        })
    {
        return Err(ApiError::BadRequest("invalid platform requirements".into()));
    }
    let id = state
        .store
        .create_build(project, auth.user_id, &input, goal)
        .await?;
    state
        .audit(
            auth.user_id,
            "build_created",
            id,
            json!({"project_id":project,"ref":input.reference}),
        )
        .await?;
    Ok(Json(state.store.build_route(id).await?))
}

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

pub async fn build_detail(state: &AppState, id: Uuid) -> Result<Value> {
    let mut build =
        serde_json::to_value(state.store.build_detail(id).await?).map_err(anyhow::Error::from)?;
    build["source_snapshot"] = if let Some(snapshot) = build["source_snapshot_id"].as_str() {
        let id = Uuid::parse_str(snapshot).map_err(anyhow::Error::from)?;
        serde_json::to_value(crate::source_cache::load(state, id).await?)
            .map_err(anyhow::Error::from)?
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishInput {
    version: Option<String>,
}

pub async fn publish(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<PublishInput>,
) -> Result<Json<Value>> {
    auth.project(state.store.build_project(id).await?, "release")?;
    state.store.request_publication(id, input.version).await?;
    let release = scheduler::publish_build(&state, id)
        .await
        .map_err(|error| ApiError::Conflict(error.to_string()))?;
    state
        .audit(
            auth.user_id,
            "publish_requested",
            id,
            json!({"release_id":release}),
        )
        .await?;
    Ok(Json(json!({"id":release})))
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
    let url = state.storage.download_url(&artifact.object_key)?;
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
