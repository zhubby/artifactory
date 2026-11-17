//! ```text
//! [INPUT]: 依赖项目鉴权、日志索引与私有对象存储。
//! [OUTPUT]: 提供静态日志分页和带 complete 事件的 SSE。
//! [POS]: 历史阅读与实时订阅共用日志正文契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use super::builds::build_detail;
use crate::{
    auth::{Auth, TenantState},
    error::{ApiError, Result},
    store::LogChunk,
};
use axum::{
    Json,
    extract::{Path, Query},
    http::HeaderMap,
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use uuid::Uuid;

#[derive(Deserialize, Default)]
pub struct LogQuery {
    attempt_id: Option<Uuid>,
    step: Option<String>,
    before: Option<i64>,
}

fn log_data(log: &LogChunk, text: String) -> Value {
    json!({"id":log.id,"attempt_id":log.attempt_id,"step":log.step,"text":text})
}

pub async fn history(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>> {
    auth.project(state.store.build_project(id).await?, "read")?;
    if query.before.is_some_and(|cursor| cursor <= 0)
        || query.step.as_ref().is_some_and(|step| step.len() > 255)
    {
        return Err(ApiError::BadRequest("invalid log cursor or step".into()));
    }
    let mut logs = state
        .store
        .historical_log_chunks(
            id,
            query.attempt_id,
            query.step.as_deref(),
            query.before.unwrap_or(i64::MAX),
        )
        .await?;
    let more = logs.len() > 100;
    logs.truncate(100);
    logs.reverse();
    let next_before = if more {
        logs.first().map(|log| log.id)
    } else {
        None
    };
    let logs = futures::stream::iter(logs)
        .map(|log| {
            let storage = &state.storage;
            async move {
                let bytes = storage.get_log(&log.object_key).await?;
                Ok::<_, anyhow::Error>(log_data(&log, String::from_utf8_lossy(&bytes).into_owned()))
            }
        })
        .buffered(8)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(Json(json!({"logs":logs,"next_before":next_before})))
}

#[derive(Deserialize, Default)]
pub struct EventQuery {
    #[serde(default)]
    after: i64,
}

pub async fn events(
    TenantState(state): TenantState,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let project = state.store.build_project(id).await?;
    auth.project(project, "read")?;
    let mut cursor = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(query.after)
        .max(0);
    let stream = async_stream::stream! {
        let started = tokio::time::Instant::now();
        loop {
            if !auth.refresh(&state).await.is_ok_and(|auth| auth.project(project, "read").is_ok()) { break; }
            let snapshot = build_detail(&state, id).await.ok();
            let terminal = snapshot.as_ref().is_some_and(|snapshot| matches!(snapshot["status"].as_str(), Some("succeeded" | "failed" | "canceled")));
            if let Some(snapshot) = &snapshot {
                yield Ok::<Event, Infallible>(Event::default().event("snapshot").data(snapshot.to_string()));
            }
            let mut drained = false;
            let mut backlog = false;
            if let Ok(logs) = state.store.log_chunks(id, cursor).await {
                backlog = logs.len() == 100;
                drained = !backlog;
                for log in logs {
                    let bytes = match state.storage.get_log(&log.object_key).await {
                        Ok(bytes) => bytes,
                        Err(_) => { drained = false; backlog = false; break; }
                    };
                    cursor = log.id;
                    yield Ok(Event::default().id(cursor.to_string()).event("log").data(log_data(&log, String::from_utf8_lossy(&bytes).into_owned()).to_string()));
                }
            }
            if terminal && drained {
                yield Ok(Event::default().event("complete").data(snapshot.unwrap().to_string()));
                break;
            }
            if started.elapsed() > Duration::from_secs(55) { break; }
            if !terminal || !backlog {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    };
    Ok((
        [
            ("cache-control", "no-cache, no-transform"),
            ("x-accel-buffering", "no"),
        ],
        Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))),
    ))
}
