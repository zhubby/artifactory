//! [INPUT]: 依赖领域拒绝和 HTTP 响应。
//! [OUTPUT]: 输出身份拒绝和瞬态故障的 HTTP 状态。
//! [POS]: 服务接口的错误边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
pub enum ApiError {
    Unauthorized,
    Forbidden,
    NotFound,
    Unavailable,
    Internal(anyhow::Error),
    Conflict(String),
    BadRequest(String),
}
pub type Result<T> = std::result::Result<T, ApiError>;
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self::Internal(error.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "authorization expired".into()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "permission denied".into()),
            Self::NotFound => (StatusCode::NOT_FOUND, "resource not found".into()),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "service temporarily unavailable".into(),
            ),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "service operation failed".into(),
            ),
        };
        (status, Json(json!({"error":message}))).into_response()
    }
}
