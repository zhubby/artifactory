use crate::{
    auth::{self, AccountAuth, Auth, TenantState},
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    action: String,
    idempotency_key: Uuid,
    slug: Option<String>,
    current_password: Option<String>,
}

async fn start(
    s: AppState,
    a: Auth,
    project: Option<Uuid>,
    input: Input,
) -> Result<(StatusCode, Json<Value>)> {
    a.session_role("viewer")?;
    let verified = if input.action == "delete" {
        Some(
            auth::verify_current_password(
                &s,
                a.user_id,
                &a.username,
                input
                    .current_password
                    .ok_or_else(|| ApiError::BadRequest("current password is required".into()))?,
            )
            .await?,
        )
    } else {
        None
    };
    let operation = s
        .store
        .start_lifecycle(
            &a,
            project,
            &input.action,
            input.idempotency_key,
            input.slug.as_deref(),
            verified.as_deref(),
        )
        .await?;
    Ok((StatusCode::ACCEPTED, Json(operation)))
}
pub async fn tenant(
    TenantState(s): TenantState,
    a: Auth,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>)> {
    start(s, a, None, input).await
}
pub async fn project(
    TenantState(s): TenantState,
    a: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Input>,
) -> Result<(StatusCode, Json<Value>)> {
    start(s, a, Some(id), input).await
}
pub async fn tenant_impact(TenantState(s): TenantState, a: Auth) -> Result<Json<Value>> {
    a.session_role("admin")?;
    Ok(Json(s.store.lifecycle_impact(a.tenant_id, None).await?))
}
pub async fn project_impact(
    TenantState(s): TenantState,
    a: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    a.project(id, "admin")?;
    Ok(Json(s.store.lifecycle_impact(a.tenant_id, Some(id)).await?))
}
pub async fn list(State(s): State<AppState>, a: AccountAuth) -> Result<Json<Vec<Value>>> {
    Ok(Json(s.store.lifecycle_operations(a.user_id).await?))
}
pub async fn detail(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.store.lifecycle_operation(a.user_id, id).await?))
}
pub async fn cancel(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.store.cancel_lifecycle(a.user_id, id).await?))
}
pub async fn retry(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(s.store.retry_lifecycle(a.user_id, id).await?))
}
