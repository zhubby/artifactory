//! [INPUT]: 依赖个人会话、租户项目权限和通知渠道服务。
//! [OUTPUT]: 提供个人邮件设置和管理员 Webhook 路由。
//! [POS]: 外部渠道 HTTP 边界；个人设置不要求当前租户。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{AccountAuth, Auth},
    error::Result,
    notifications::channels::{email, webhook},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post, put},
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Address {
    address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Token {
    token: String,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/notifications/preferences",
            get(preferences).put(set_preferences),
        )
        .route("/notifications/email", put(bind).delete(retire))
        .route("/notifications/email/resend", post(resend))
        .route("/notifications/email/verify", post(verify))
        .route("/notifications/email/unsubscribe", post(unsubscribe))
        .route("/notifications/email/one-click", post(one_click))
        .route(
            "/tenant/notification-webhooks",
            get(tenant_list).post(tenant_create),
        )
        .route(
            "/tenant/notification-webhooks/{endpoint}",
            patch(tenant_patch).delete(tenant_delete),
        )
        .route(
            "/tenant/notification-webhooks/{endpoint}/rotate",
            post(tenant_rotate),
        )
        .route(
            "/tenant/notification-webhooks/{endpoint}/test",
            post(tenant_test),
        )
        .route(
            "/tenant/notification-webhooks/{endpoint}/deliveries",
            get(tenant_deliveries),
        )
        .route(
            "/tenant/notification-webhooks/{endpoint}/deliveries/{delivery}/retry",
            post(tenant_retry),
        )
        .route(
            "/projects/{id}/notification-webhooks",
            get(project_list).post(project_create),
        )
        .route(
            "/projects/{id}/notification-webhooks/{endpoint}",
            patch(project_patch).delete(project_delete),
        )
        .route(
            "/projects/{id}/notification-webhooks/{endpoint}/rotate",
            post(project_rotate),
        )
        .route(
            "/projects/{id}/notification-webhooks/{endpoint}/test",
            post(project_test),
        )
        .route(
            "/projects/{id}/notification-webhooks/{endpoint}/deliveries",
            get(project_deliveries),
        )
        .route(
            "/projects/{id}/notification-webhooks/{endpoint}/deliveries/{delivery}/retry",
            post(project_retry),
        )
}
async fn preferences(State(s): State<AppState>, a: AccountAuth) -> Result<Json<Value>> {
    Ok(Json(email::preferences(&s, a.user_id).await?))
}
async fn set_preferences(
    State(s): State<AppState>,
    a: AccountAuth,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(email::put_preferences(&s, a.user_id, v).await?))
}
async fn bind(
    State(s): State<AppState>,
    a: AccountAuth,
    Json(v): Json<Address>,
) -> Result<Json<Value>> {
    Ok(Json(email::bind(&s, a.user_id, Some(&v.address)).await?))
}
async fn resend(State(s): State<AppState>, a: AccountAuth) -> Result<Json<Value>> {
    Ok(Json(email::bind(&s, a.user_id, None).await?))
}
async fn verify(
    State(s): State<AppState>,
    a: AccountAuth,
    Json(v): Json<Token>,
) -> Result<Json<Value>> {
    Ok(Json(email::verify(&s, a.user_id, &v.token).await?))
}
async fn retire(State(s): State<AppState>, a: AccountAuth) -> Result<Json<Value>> {
    Ok(Json(email::retire(&s, a.user_id).await?))
}
async fn unsubscribe(State(s): State<AppState>, Json(v): Json<Token>) -> Result<Json<Value>> {
    Ok(Json(email::unsubscribe(&s, &v.token).await?))
}
async fn tenant_list(State(s): State<AppState>, a: Auth) -> Result<Json<Value>> {
    Ok(Json(webhook::list(&s, &a, None).await?))
}
async fn tenant_create(
    State(s): State<AppState>,
    a: Auth,
    Json(v): Json<webhook::Input>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::write(&s, &a, None, None, v).await?))
}
async fn tenant_patch(
    State(s): State<AppState>,
    a: Auth,
    Path(e): Path<Uuid>,
    Json(v): Json<webhook::Input>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::write(&s, &a, None, Some(e), v).await?))
}
async fn tenant_delete(
    State(s): State<AppState>,
    a: Auth,
    Path(e): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, None, e, "delete", None).await?,
    ))
}
async fn tenant_rotate(
    State(s): State<AppState>,
    a: Auth,
    Path(e): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, None, e, "rotate", None).await?,
    ))
}
async fn tenant_test(
    State(s): State<AppState>,
    a: Auth,
    Path(e): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::mutate(&s, &a, None, e, "test", None).await?))
}
async fn tenant_deliveries(
    State(s): State<AppState>,
    a: Auth,
    Path(e): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::deliveries(&s, &a, None, e).await?))
}
async fn tenant_retry(
    State(s): State<AppState>,
    a: Auth,
    Path((e, d)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, None, e, "retry", Some(d)).await?,
    ))
}
async fn project_list(
    State(s): State<AppState>,
    a: Auth,
    Path(p): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::list(&s, &a, Some(p)).await?))
}
async fn project_create(
    State(s): State<AppState>,
    a: Auth,
    Path(p): Path<Uuid>,
    Json(v): Json<webhook::Input>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::write(&s, &a, Some(p), None, v).await?))
}
async fn project_patch(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e)): Path<(Uuid, Uuid)>,
    Json(v): Json<webhook::Input>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::write(&s, &a, Some(p), Some(e), v).await?))
}
async fn project_delete(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, Some(p), e, "delete", None).await?,
    ))
}
async fn project_rotate(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, Some(p), e, "rotate", None).await?,
    ))
}
async fn project_test(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, Some(p), e, "test", None).await?,
    ))
}
async fn project_deliveries(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(webhook::deliveries(&s, &a, Some(p), e).await?))
}
async fn project_retry(
    State(s): State<AppState>,
    a: Auth,
    Path((p, e, d)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        webhook::mutate(&s, &a, Some(p), e, "retry", Some(d)).await?,
    ))
}

async fn one_click(State(s): State<AppState>, Query(v): Query<Token>) -> Result<Json<Value>> {
    Ok(Json(email::unsubscribe(&s, &v.token).await?))
}
