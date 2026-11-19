//! [INPUT]: 依赖网关服务令牌和绑定方法、对象、交付文件名的加密票据。
//! [OUTPUT]: 返回网关内部使用的 S3 签名 URL 与固定文件名。
//! [POS]: 私有对象 URL 只经过认证的服务接口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    error::{ApiError, Result},
    state::AppState,
};
use axum::{Json, extract::State, http::HeaderMap};
use serde::Deserialize;
use serde_json::Value;
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
    authorize_gateway(&headers)?;
    let resolved = state
        .storage
        .resolve_object_ticket(&input.ticket, &input.method)
        .map_err(|_| ApiError::Forbidden)?;
    Ok(Json(
        serde_json::to_value(resolved).map_err(anyhow::Error::from)?,
    ))
}

pub(super) fn authorize_gateway(headers: &HeaderMap) -> Result<()> {
    let configured = std::env::var("ARTIFACTORY_GATEWAY_TOKEN").map_err(|_| ApiError::Forbidden)?;
    let supplied = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    if configured.len() < 32 || crate::crypto::hash(&configured) != crate::crypto::hash(supplied) {
        return Err(ApiError::Unauthorized);
    }
    Ok(())
}
