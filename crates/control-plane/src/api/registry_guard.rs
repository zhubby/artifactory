//! [INPUT]: 依赖网关服务身份、仓库签名 JWT 与冻结发布租约。
//! [OUTPUT]: 授权单次正式 Manifest 写入，不返回仓库凭据。
//! [POS]: Gateway 在缓冲并计算正文摘要后调用的内部围栏。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    error::{ApiError, Result},
    state::AppState,
};
use axum::{Json, extract::State, http::HeaderMap};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorize {
    authorization: String,
    repository: String,
    reference: String,
    digest: String,
}

pub async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Authorize>,
) -> Result<Json<Value>> {
    super::artifact_gateway::authorize_gateway(&headers)?;
    if input.authorization.len() > 16384
        || input.repository.len() > 255
        || input.reference.len() > 128
    {
        return Err(ApiError::Forbidden);
    }
    let token = input
        .authorization
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;
    let (release, generation) = crate::images::registry::verify_release_registry_token(token)?
        .ok_or(ApiError::Forbidden)?;
    if !state
        .store
        .authorize_release_manifest(
            release,
            generation,
            &input.repository,
            &input.reference,
            &input.digest,
        )
        .await?
    {
        return Err(ApiError::Conflict(
            "manifest is outside the active frozen release delivery".into(),
        ));
    }
    Ok(Json(json!({"authorized":true})))
}
