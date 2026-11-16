//! [INPUT]: 依赖租户 Session、签名密钥环与签名存储。
//! [OUTPUT]: 提供签名凭据、版本、方案和项目默认方案 HTTP 接口。
//! [POS]: 控制台签名管理入口；所有响应均不返回秘密材料。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{Auth, TenantState},
    error::{ApiError, Result},
    signing::{self, Material},
};
use axum::{Json, extract::Path};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::Zeroizing;

pub async fn credentials(TenantState(state): TenantState, auth: Auth) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    Ok(Json(
        json!({"configured":state.settings.signing_keys.is_some(),"credentials":state.store.signing_credentials().await?}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCredential {
    name: String,
    material: Material,
}
pub async fn create(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<NewCredential>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    if input.name.trim().is_empty() || input.name.len() > 100 {
        return Err(ApiError::BadRequest(
            "invalid signing credential name".into(),
        ));
    }
    save(
        &state,
        auth.tenant_id,
        Uuid::new_v4(),
        Some(input.name),
        input.material,
    )
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewVersion {
    material: Material,
}
pub async fn version(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<NewVersion>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    save(&state, auth.tenant_id, id, None, input.material).await
}
async fn save(
    state: &crate::state::AppState,
    tenant: Uuid,
    id: Uuid,
    name: Option<String>,
    material: Material,
) -> Result<Json<Value>> {
    let keys = state
        .settings
        .signing_keys
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("signing key ring is not configured".into()))?;
    let material=tokio::task::spawn_blocking(move||signing::inspect(&material).map(|metadata|(material,metadata))).await.map_err(anyhow::Error::from)?
        .map_err(|_|ApiError::BadRequest("unable to unlock signing identity; check file, password, alias, certificate validity and private key".into()))?;
    let (material, metadata) = material;
    let version = Uuid::new_v4();
    let plaintext = Zeroizing::new(serde_json::to_vec(&material).map_err(anyhow::Error::from)?);
    let envelope = keys.seal(tenant, id, version, &plaintext)?;
    state
        .store
        .save_signing_version(
            id,
            version,
            name.as_deref(),
            material.platform(),
            &metadata,
            &envelope,
        )
        .await?;
    Ok(Json(
        json!({"id":id,"version_id":version,"metadata":metadata}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enabled {
    enabled: bool,
}
pub async fn enabled(
    TenantState(state): TenantState,
    auth: Auth,
    Path((resource, id)): Path<(String, String)>,
    Json(input): Json<Enabled>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    state
        .store
        .set_signing_enabled(&resource, &id, input.enabled)
        .await?;
    Ok(Json(json!({"ok":true})))
}
pub async fn profiles(TenantState(state): TenantState, auth: Auth) -> Result<Json<Vec<Value>>> {
    auth.session_role("admin")?;
    Ok(Json(state.store.signing_profiles().await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    name: String,
    credential_id: Uuid,
    #[serde(default)]
    notarize: bool,
}
pub async fn create_profile(
    TenantState(state): TenantState,
    auth: Auth,
    Json(input): Json<Profile>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    if input.name.is_empty()
        || input.name.len() > 80
        || !input
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(ApiError::BadRequest(
            "signing profile name requires 1..80 letters, digits, dot, underscore or hyphen".into(),
        ));
    }
    let name = format!("managed:{}", input.name);
    state
        .store
        .create_signing_profile(&name, input.credential_id, input.notarize)
        .await?;
    Ok(Json(json!({"name":name})))
}
pub async fn defaults(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.project_signing_defaults(id).await?))
}
pub async fn set_defaults(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<BTreeMap<String, String>>,
) -> Result<Json<Value>> {
    auth.session_role("admin")?;
    auth.project(id, "admin")?;
    if input
        .keys()
        .any(|k| !["macos", "android"].contains(&k.as_str()))
    {
        return Err(ApiError::BadRequest(
            "unsupported signing default platform".into(),
        ));
    }
    state.store.set_project_signing_defaults(id, &input).await?;
    Ok(Json(json!({"ok":true})))
}

pub async fn preview(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>> {
    auth.project(id, "read")?;
    Ok(Json(state.store.project_signing_preview(id).await?))
}
