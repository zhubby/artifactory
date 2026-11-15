use crate::{
    auth::{AccountAuth, Auth, TenantState},
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTenant {
    name: String,
    idempotency_key: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    username: String,
    role: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    role: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    user_id: Uuid,
}

fn tenant_role(role: &str) -> Result<()> {
    if !matches!(role, "admin" | "member") {
        return Err(ApiError::BadRequest("role must be admin or member".into()));
    }
    Ok(())
}
fn project_role(role: &str) -> Result<()> {
    if !matches!(role, "admin" | "operator" | "viewer") {
        return Err(ApiError::BadRequest("invalid project role".into()));
    }
    Ok(())
}

pub async fn list(State(s): State<AppState>, account: AccountAuth) -> Result<Json<Vec<Value>>> {
    Ok(Json(s.store.tenants(account.user_id).await?))
}
pub async fn create(
    State(s): State<AppState>,
    account: AccountAuth,
    Json(input): Json<NewTenant>,
) -> Result<Json<Value>> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(ApiError::BadRequest("invalid tenant name".into()));
    }
    let id = s
        .store
        .create_tenant(account.user_id, name, input.idempotency_key)
        .await?;
    Ok(Json(json!({"id":id})))
}
pub async fn context(TenantState(s): TenantState, auth: Auth) -> Result<Json<Value>> {
    let tenant = s
        .store
        .tenants(auth.user_id)
        .await?
        .into_iter()
        .find(|t| t["id"] == auth.tenant_id.to_string())
        .ok_or(ApiError::NotFound)?;
    Ok(Json(
        json!({"tenant":tenant,"project_roles":auth.project_roles}),
    ))
}
pub async fn members(TenantState(s): TenantState, auth: Auth) -> Result<Json<Vec<Value>>> {
    auth.session_role("admin")?;
    Ok(Json(s.store.tenant_members().await?))
}
pub async fn add_member(
    TenantState(s): TenantState,
    auth: Auth,
    Json(input): Json<Member>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    tenant_role(&input.role)?;
    if input.username.is_empty() || input.username.len() > 100 {
        return Err(ApiError::BadRequest("invalid username".into()));
    }
    let id = s.store.add_member(&input.username, &input.role).await?;
    s.audit(
        auth.user_id,
        "tenant_member_added",
        id,
        json!({"role":input.role}),
    )
    .await?;
    Ok(Json(json!({"id":id})))
}
pub async fn change_member(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Role>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    tenant_role(&input.role)?;
    s.store.change_member(id, Some(&input.role)).await?;
    s.audit(
        auth.user_id,
        "tenant_member_role_changed",
        id,
        json!({"role":input.role}),
    )
    .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn remove_member(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    s.store.change_member(id, None).await?;
    s.audit(auth.user_id, "tenant_member_removed", id, json!({}))
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn transfer(
    TenantState(s): TenantState,
    auth: Auth,
    Json(input): Json<Owner>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    s.store.transfer_owner(input.user_id).await?;
    s.audit(
        auth.user_id,
        "tenant_owner_transferred",
        input.user_id,
        json!({}),
    )
    .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn invitations(TenantState(s): TenantState, auth: Auth) -> Result<Json<Vec<Value>>> {
    auth.session_role("admin")?;
    Ok(Json(s.store.invitations().await?))
}
pub async fn invite(
    TenantState(s): TenantState,
    auth: Auth,
    Json(input): Json<Role>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    tenant_role(&input.role)?;
    Ok(Json(s.store.invite(&input.role).await?))
}
pub async fn revoke_invite(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    s.store.revoke_invitation(id).await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn preview(State(s): State<AppState>, Path(token): Path<String>) -> Result<Json<Value>> {
    if token.len() > 256 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(s.store.invitation_preview(&token).await?))
}
pub async fn accept(
    State(s): State<AppState>,
    account: AccountAuth,
    Path(token): Path<String>,
) -> Result<Json<Value>> {
    if token.len() > 256 {
        return Err(ApiError::NotFound);
    }
    Ok(Json(
        json!({"tenant_id":s.store.accept_invitation(&token,account.user_id).await?}),
    ))
}
pub async fn project_members(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>> {
    auth.project(id, "admin")?;
    Ok(Json(s.store.project_members(id).await?))
}
pub async fn member_options(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>> {
    auth.project(id, "admin")?;
    Ok(Json(s.store.tenant_members().await?))
}
pub async fn set_project_member(
    TenantState(s): TenantState,
    auth: Auth,
    Path((id, user)): Path<(Uuid, Uuid)>,
    Json(input): Json<Role>,
) -> Result<Json<Value>> {
    auth.project(id, "admin")?;
    project_role(&input.role)?;
    s.store
        .set_project_member(id, user, Some(&input.role))
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn remove_project_member(
    TenantState(s): TenantState,
    auth: Auth,
    Path((id, user)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    auth.project(id, "admin")?;
    s.store.set_project_member(id, user, None).await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn node_access(
    TenantState(s): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<crate::store::NodeAccess>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    s.store.set_node_access(id, &input).await?;
    s.audit(
        auth.user_id,
        "project_node_access_changed",
        id,
        serde_json::to_value(input).map_err(anyhow::Error::from)?,
    )
    .await?;
    Ok(Json(json!({"ok":true})))
}
