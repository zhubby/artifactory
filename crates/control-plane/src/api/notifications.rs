//! [INPUT]: 依赖账户会话、收件箱存储与项目读取授权。
//! [OUTPUT]: 提供跨租户个人通知、回执和失效事件流。
//! [POS]: 账户级 HTTP 边界，写操作沿用会话 CSRF 校验。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{auth::AccountAuth, error::Result, state::AppState, store::notifications::*};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde::Deserialize;
use std::{convert::Infallible, time::Duration};
use uuid::Uuid;
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/notifications", get(list))
        .route("/notifications/summary", get(summary))
        .route("/notifications/scopes", get(scopes))
        .route("/notifications/read-all", post(read_all))
        .route("/notifications/events", get(events))
        .route("/notifications/{id}", get(detail))
        .route("/notifications/{id}/read", post(read))
        .route(
            "/projects/{id}/notification-subscription",
            get(subscription).put(follow),
        )
        .layer(axum::middleware::map_response(no_cache))
}
async fn no_cache(mut response: axum::response::Response) -> axum::response::Response {
    response
        .headers_mut()
        .entry(axum::http::header::CACHE_CONTROL)
        .or_insert(axum::http::HeaderValue::from_static("no-store"));
    response
}
async fn list(
    State(s): State<AppState>,
    a: AccountAuth,
    Query(f): Query<NotificationFilter>,
) -> Result<Json<NotificationPage>> {
    Ok(Json(s.store.list_notifications(a.user_id, f).await?))
}
async fn summary(State(s): State<AppState>, a: AccountAuth) -> Result<Json<NotificationSummary>> {
    Ok(Json(s.store.notification_summary(a.user_id).await?))
}
async fn scopes(State(s): State<AppState>, a: AccountAuth) -> Result<Json<NotificationScopes>> {
    Ok(Json(s.store.notification_scopes(a.user_id).await?))
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct TimelineQuery {
    cursor: Option<String>,
}
async fn detail(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
    Query(f): Query<TimelineQuery>,
) -> Result<Json<NotificationDetail>> {
    Ok(Json(
        s.store.notification_detail(a.user_id, id, f.cursor).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadInput {
    attention_revision: i64,
}
async fn read(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
    Json(f): Json<ReadInput>,
) -> Result<Json<NotificationSummary>> {
    Ok(Json(
        s.store
            .read_notification(a.user_id, id, f.attention_revision)
            .await?,
    ))
}
async fn read_all(
    State(s): State<AppState>,
    a: AccountAuth,
    Json(f): Json<ReadAllInput>,
) -> Result<Json<NotificationSummary>> {
    Ok(Json(s.store.read_all_notifications(a.user_id, f).await?))
}
async fn subscription(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectSubscription>> {
    Ok(Json(
        s.store.notification_subscription(a.user_id, id).await?,
    ))
}
async fn follow(
    State(s): State<AppState>,
    a: AccountAuth,
    Path(id): Path<Uuid>,
    Json(f): Json<ProjectSubscription>,
) -> Result<Json<ProjectSubscription>> {
    Ok(Json(
        s.store
            .set_notification_subscription(a.user_id, id, f.following)
            .await?,
    ))
}
async fn events(
    State(s): State<AppState>,
    a: AccountAuth,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let hash = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|p| p.trim().strip_prefix("artifactory_session="))
        })
        .map(crate::crypto::hash)
        .unwrap_or_default();
    let stream = async_stream::stream! {
     let started=tokio::time::Instant::now();let mut last=None;
     loop {
      if s.store.authenticate_session(&hash).await.ok().flatten().is_none_or(|u|u.user_id!=a.user_id){break}
      let Ok(current)=s.store.notification_summary(a.user_id).await else {break};
      if last.as_ref()!=Some(&current) {
       // The stream contains invalidations only. The client fetches current ACL-filtered data.
       yield Ok::<Event,Infallible>(Event::default().event("invalidate").data("{}"));last=Some(current);
      }
      if started.elapsed()>Duration::from_secs(50){break}
      tokio::time::sleep(Duration::from_secs(3)).await;
     }
    };
    Ok((
        [
            ("cache-control", "no-cache, no-store, no-transform"),
            ("x-accel-buffering", "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    ))
}
