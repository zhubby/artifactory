//! [INPUT]: 依赖租户鉴权、Release 聚合和发布调度器。
//! [OUTPUT]: 提供版本元数据、独立构建创建、发布和冻结交付下载。
//! [POS]: Release HTTP 边界；版本创建后固定，说明独立更新，源码由构建接收。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{Auth, TenantState},
    error::{ApiError, Result},
    scheduler,
    store::{ReleaseBuildInput, ReleaseInput, ReleaseUpdate},
};
use axum::{
    Json,
    extract::Path,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Path(project): Path<Uuid>,
    Json(input): Json<ReleaseInput>,
) -> Result<Json<Value>> {
    auth.project(project, "release")?;
    let id = state.store.create_release(project, &input).await?;
    state
        .audit(
            auth.user_id,
            "release_created",
            id,
            json!({"project_id":project,"version":input.version}),
        )
        .await?;
    Ok(Json(json!(state.store.release(id).await?)))
}
pub async fn project_list(
    TenantState(state): TenantState,
    auth: Auth,
    Path(project): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(project, "read")?;
    Ok(Json(json!(
        state
            .store
            .releases(Some(project), auth.visible_projects())
            .await?
    )))
}
pub async fn update(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<ReleaseUpdate>,
) -> Result<Json<Value>> {
    auth.project(state.store.release_project(id).await?, "release")?;
    state.store.update_release(id, &input).await?;
    state
        .audit(
            auth.user_id,
            "release_updated",
            id,
            json!({"notes":input.notes}),
        )
        .await?;
    Ok(Json(json!(state.store.release(id).await?)))
}
pub async fn builds(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(state.store.release_project(id).await?, "read")?;
    Ok(Json(json!(state.store.release_builds(id).await?)))
}
pub async fn create_build(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<ReleaseBuildInput>,
) -> Result<Json<Value>> {
    auth.project(state.store.release_project(id).await?, "build")?;
    let build = state
        .store
        .create_release_build(id, auth.user_id, &input)
        .await?;
    state
        .audit(
            auth.user_id,
            "build_created",
            build,
            json!({"release_id":id}),
        )
        .await?;
    Ok(Json(state.store.build_route(build).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishInput {
    build_id: Uuid,
}
pub async fn publish(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<PublishInput>,
) -> Result<Json<Value>> {
    auth.project(state.store.release_project(id).await?, "release")?;
    let builds = state.store.release_builds(id).await?;
    if !builds.iter().any(|b| b.id == input.build_id) {
        return Err(ApiError::BadRequest(
            "build does not belong to this Release".into(),
        ));
    }
    scheduler::publish_build(&state, input.build_id)
        .await
        .map_err(|e| ApiError::Conflict(e.to_string()))?;
    state
        .audit(
            auth.user_id,
            "release_publish_requested",
            id,
            json!({"build_id":input.build_id}),
        )
        .await?;
    Ok(Json(json!(state.store.release(id).await?)))
}
async fn frozen(
    state: &crate::state::AppState,
    auth: &Auth,
    id: Uuid,
) -> Result<crate::store::dto::Release> {
    let r = state.store.release(id).await?;
    auth.project(r.project_id, "read")?;
    if r.status != "published" {
        return Err(ApiError::Conflict("Release must be published".into()));
    }
    Ok(r)
}
fn attachment(body: String, name: String, content_type: &str) -> Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        content_type.parse().map_err(anyhow::Error::from)?,
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{name}\"")
            .parse()
            .map_err(anyhow::Error::from)?,
    );
    Ok((headers, body).into_response())
}
pub async fn manifest(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let r = frozen(&state, &auth, id).await?;
    let body = serde_json::to_string_pretty(&r.delivery_manifest).map_err(anyhow::Error::from)?;
    attachment(
        body,
        format!("{}-{}-release.json", r.project_slug, r.version),
        "application/json",
    )
}
pub async fn checksums(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let r = frozen(&state, &auth, id).await?;
    let manifest = r.delivery_manifest.as_ref().ok_or(ApiError::NotFound)?;
    let files = manifest["files"].as_array().ok_or(ApiError::NotFound)?;
    let body = files
        .iter()
        .map(|f| {
            format!(
                "{}  {}\n",
                f["sha256"].as_str().unwrap_or(""),
                f["delivery_filename"].as_str().unwrap_or("")
            )
        })
        .collect();
    attachment(
        body,
        format!("{}-{}-SHA256SUMS", r.project_slug, r.version),
        "text/plain; charset=utf-8",
    )
}
