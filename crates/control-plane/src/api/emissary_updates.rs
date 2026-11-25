//! [INPUT]: 依赖租户会话、产品统一版本与升级事务。
//! [OUTPUT]: 提供固定版本状态、手动升级、签名目录和并发策略。
//! [POS]: 升级 HTTP 边界；目标由 Artifactory 产品统一版本确定。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::auth::TenantState;
pub use crate::store::Target;
use crate::store::dto;
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    ota,
};
use axum::{Json, extract::Path};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

pub async fn version(TenantState(state): TenantState, auth: Auth) -> Result<Json<Value>> {
    auth.session_role("viewer")?;
    let releases = state.store.current_emissary_releases().await?;
    let release = (releases.len() == 1).then(|| &releases[0]);
    Ok(Json(json!({
        "product_version":crate::VERSION,
        "release_id":release.map(|r|r.id),
        "release_version":release.map(|r|&r.version),
        "release_status":match releases.len() { 0=>"missing",1=>"ready",_=>"ambiguous" },
    })))
}

pub async fn releases(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::EmissaryRelease>>> {
    auth.session_role("viewer")?;
    Ok(Json(state.store.emissary_releases().await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    tag: String,
}
pub async fn import(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<Import>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    let id = ota::import_tag(&state, &input.tag).await.map_err(|e| {
        tracing::warn!(error=%e,"Emissary release import rejected");
        ApiError::BadRequest(format!("Emissary release import rejected: {e}"))
    })?;
    state
        .audit(
            auth.user_id,
            "emissary_release_imported",
            id,
            json!({"tag":input.tag}),
        )
        .await?;
    Ok(Json(json!({"id":id})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateConfirmation {
    product_version: String,
    release_id: Uuid,
}

pub async fn request_update(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateConfirmation>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    let update = state
        .store
        .request_emissary_update(id, auth.user_id, &input.product_version, input.release_id)
        .await?;
    state.coordination.commands.notify_one();
    Ok(Json(json!({"id":update})))
}

pub async fn node_target(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Target>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    state
        .store
        .set_node_target(id, &input, auth.user_id)
        .await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn pool_target(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Target>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    if input.concurrency.is_some_and(|n| !(1..=32).contains(&n)) {
        return Err(ApiError::BadRequest("concurrency must be 1 to 32".into()));
    }
    state
        .store
        .set_pool_target(id, &input, auth.user_id)
        .await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn updates(
    TenantState(state): TenantState,
    auth: Auth,
) -> Result<Json<Vec<dto::EmissaryUpdate>>> {
    auth.session_role("viewer")?;
    Ok(Json(state.store.emissary_updates().await?))
}
pub async fn retry(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    let new_id = state.store.retry_update(id, auth.user_id).await?;
    Ok(Json(json!({"id":new_id})))
}

pub async fn cancel(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    state.store.cancel_update(id, auth.user_id).await?;
    Ok(Json(json!({"ok":true})))
}
