//! [INPUT]: 依赖当前租户管理员 Session 与固定操作输入。
//! [OUTPUT]: 提供节点操作创建、分页、结果读取和请求取消。
//! [POS]: 只读诊断 HTTP 边界；CSRF 与 Origin 由统一认证中间件校验。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    state::AppState,
};
use artifactory_emissary_protocol::operations::OperationCreate;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/nodes/{id}/operations", get(list).post(create))
        .route("/api/v1/node-operations/{id}", get(detail))
        .route("/api/v1/node-operations/{id}/cancel", post(cancel))
}
async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(node): Path<Uuid>,
    Json(input): Json<OperationCreate>,
) -> Result<impl IntoResponse> {
    if state.coordination.is_draining() {
        return Err(ApiError::Conflict("control plane is draining".into()));
    }
    let operation = state
        .store
        .create_node_operation(&auth, node, &input)
        .await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(operation)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    cursor: Option<Uuid>,
    limit: Option<u32>,
}
async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Path(node): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<impl IntoResponse> {
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            state
                .store
                .list_node_operations(&auth, node, page.cursor, page.limit.unwrap_or(20))
                .await?,
        ),
    ))
}
async fn detail(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.store.node_operation(&auth, id).await?),
    ))
}
async fn cancel(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(state.store.cancel_node_operation(&auth, id).await?),
    ))
}
