//! [INPUT]: 依赖网关服务令牌和绑定方法、对象的加密票据。
//! [OUTPUT]: 返回网关内部使用的 S3 签名 URL。
//! [POS]: 私有对象 URL 只经过认证的服务接口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    crypto,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{Json, extract::State, http::HeaderMap};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolve {
    ticket: String,
    method: String,
}
pub async fn resolve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Resolve>,
) -> Result<Json<Value>> {
    let configured = std::env::var("ARTIFACTORY_GATEWAY_TOKEN").map_err(|_| ApiError::Forbidden)?;
    let supplied = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    if configured.len() < 32 || crypto::hash(&configured) != crypto::hash(supplied) {
        return Err(ApiError::Unauthorized);
    }
    let url = state
        .storage
        .resolve_ticket(&input.ticket, &input.method)
        .map_err(|_| ApiError::Forbidden)?;
    Ok(Json(json!({"url":url})))
}
