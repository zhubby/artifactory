//! [INPUT]: 依赖登录 Session、租户设备授权、一次性票据和终端流。
//! [OUTPUT]: 提供严格同源终端入口；内部短断用可恢复关闭码。
//! [POS]: 公开终端入口；秘密仅在首帧出现，不放 URL 或事件日志。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    auth::{AccountAuth, Auth},
    crypto,
    error::{ApiError, Result},
    protocol::{
        TerminalFrame,
        terminal::{MAX_FRAME_BYTES, validate_frame},
    },
    state::AppState,
    store::terminals::TerminalCreate,
    terminals,
};
use axum::{
    Json, Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, header},
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/nodes/{id}/terminal-capabilities",
            get(capabilities),
        )
        .route("/api/v1/nodes/{id}/terminal-sessions", post(create))
        .route("/api/v1/terminal-sessions/{id}", get(detail))
        .route("/api/v1/terminal-sessions/{id}/attach", post(attach))
        .route("/api/v1/terminal-sessions/{id}/close", post(close))
        .route("/api/v1/terminal-sessions/{id}/stream", get(stream))
}

async fn capabilities(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(state.store.terminal_capabilities(&auth, id).await?))
}
async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(node): Path<Uuid>,
    Json(input): Json<TerminalCreate>,
) -> Result<impl IntoResponse> {
    if state.coordination.is_draining() {
        return Err(ApiError::Conflict("control plane is draining".into()));
    }
    let _ = terminals::Runtime::get(&state).await?;
    let terminal = state.store.create_terminal(&auth, node, &input).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(terminal)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attach {
    renderer_id: Uuid,
    #[serde(default)]
    offset: u64,
}
async fn attach(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Attach>,
) -> Result<impl IntoResponse> {
    let _ = input.offset;
    let ticket = state
        .store
        .terminal_ticket(&auth, id, input.renderer_id)
        .await?;
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(
            json!({"ticket":ticket,"stream_url":format!("/api/v1/terminal-sessions/{id}/stream"),"expires_at":chrono::Utc::now()+chrono::Duration::seconds(30)}),
        ),
    ))
}
fn credential(headers: &HeaderMap) -> Result<String> {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(';')
                .find_map(|s| s.trim().strip_prefix("artifactory_session="))
        })
        .ok_or(ApiError::Unauthorized)?;
    Ok(crypto::hash(cookie))
}
async fn owned(
    state: &AppState,
    account: &AccountAuth,
    headers: &HeaderMap,
    id: Uuid,
) -> Result<crate::store::terminals::TerminalSession> {
    let terminal = state.store.terminal_session(id).await?;
    if terminal.user_id != account.user_id || terminal.credential_hash != credential(headers)? {
        return Err(ApiError::Forbidden);
    }
    Ok(terminal)
}
async fn detail(
    State(state): State<AppState>,
    account: AccountAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse> {
    let terminal = owned(&state, &account, &headers, id).await?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(terminal)))
}
async fn close(
    State(state): State<AppState>,
    account: AccountAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let _ = owned(&state, &account, &headers, id).await?;
    state
        .store
        .close_terminal(id, "user_closed", None, None)
        .await?;
    Ok(Json(json!({"closed":true})))
}
async fn stream(
    State(state): State<AppState>,
    account: AccountAuth,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| url::Url::parse(v).ok())
        .ok_or(ApiError::Forbidden)?;
    let expected = url::Url::parse(&state.settings.public_url).map_err(anyhow::Error::from)?;
    if origin.origin() != expected.origin()
        || origin.path() != "/"
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err(ApiError::Forbidden);
    }
    let _ = owned(&state, &account, &headers, id).await?;
    let runtime = terminals::Runtime::get(&state).await?;
    let permit = runtime
        .incoming
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::TooManyRequests(5))?;
    let hash = credential(&headers)?;
    Ok(ws
        .max_message_size(32 * 1024)
        .max_frame_size(32 * 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            websocket(state, id, account.user_id, hash, socket).await;
        }))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BrowserMessage {
    Authenticate {
        ticket: String,
        renderer_id: Uuid,
        #[serde(default)]
        offset: u64,
    },
    Input {
        sequence: u64,
        data: String,
    },
    Resize {
        columns: u32,
        rows: u32,
    },
    Ack {
        offset: u64,
    },
    Close,
}

fn browser_frame(
    id: Uuid,
    value: BrowserMessage,
) -> std::result::Result<TerminalFrame, &'static str> {
    let mut frame = TerminalFrame {
        terminal_id: id.to_string(),
        ..Default::default()
    };
    match value {
        BrowserMessage::Input { sequence, data } => {
            if data.len() > MAX_FRAME_BYTES.div_ceil(3) * 4 {
                return Err("input too large");
            };
            frame.kind = "input".into();
            frame.offset = sequence;
            frame.data = STANDARD
                .decode(data)
                .map_err(|_| "invalid input encoding")?;
        }
        BrowserMessage::Resize { columns, rows } => {
            frame.kind = "resize".into();
            frame.columns = columns;
            frame.rows = rows;
        }
        BrowserMessage::Ack { offset } => {
            frame.kind = "ack".into();
            frame.offset = offset;
        }
        BrowserMessage::Close => frame.kind = "close".into(),
        BrowserMessage::Authenticate { .. } => return Err("already authenticated"),
    }
    validate_frame(&frame).map_err(|_| "invalid terminal frame")?;
    Ok(frame)
}

fn browser_output(frame: TerminalFrame) -> Value {
    match frame.kind.as_str() {
        "output" => {
            json!({"type":"output","offset":frame.offset,"data":STANDARD.encode(frame.data)})
        }
        "ready" => {
            let mut value: Value =
                serde_json::from_str(&frame.payload_json).unwrap_or_else(|_| json!({}));
            value["type"] = json!("status");
            value["state"] = json!("active");
            value
        }
        "exit" => {
            let mut value: Value = serde_json::from_str(&frame.payload_json)
                .unwrap_or_else(|_| json!({"reason":"session_closed"}));
            value["type"] = json!("exit");
            value
        }
        _ => json!({"type":"status","state":"opening"}),
    }
}

async fn websocket(state: AppState, id: Uuid, user: Uuid, hash: String, mut socket: WebSocket) {
    let first = tokio::time::timeout(Duration::from_secs(5), socket.recv()).await;
    let first = match first {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<BrowserMessage>(&text) {
            Ok(BrowserMessage::Authenticate {
                ticket,
                renderer_id,
                offset,
            }) => json!({"ticket":ticket,"renderer_id":renderer_id,"offset":offset}),
            _ => {
                let _ = socket.close().await;
                return;
            }
        },
        _ => {
            let _ = socket.close().await;
            return;
        }
    };
    let (input_tx, input_rx) = mpsc::channel(32);
    let mut output = match tokio::time::timeout(
        Duration::from_secs(5),
        terminals::browser(&state, id, user, hash, first, input_rx),
    )
    .await
    {
        Ok(Ok(output)) => output,
        result => {
            let code = match result {
                Err(_) => Some(1013),
                Ok(Err(error)) => match error.code() {
                    tonic::Code::Unavailable => Some(1012),
                    tonic::Code::DeadlineExceeded | tonic::Code::ResourceExhausted => Some(1013),
                    _ => None,
                },
                _ => None,
            };
            if let Some(code) = code {
                let _ = socket
                    .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                        code,
                        reason: "forwarding unavailable".into(),
                    })))
                    .await;
                return;
            }
            let _ = socket
                .send(Message::Text(
                    json!({"type":"exit","reason":"attachment_rejected"})
                        .to_string()
                        .into(),
                ))
                .await;
            let _ = socket.close().await;
            return;
        }
    };
    let mut access = match terminals::access::Access::new(state.clone(), id).await {
        Ok(access) => access,
        Err(_) => {
            let _ = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 1008,
                    reason: "authorization expired".into(),
                })))
                .await;
            return;
        }
    };
    let (mut sink, mut source) = socket.split();
    let mut close_code = 1000;
    loop {
        if !access.valid() {
            close_code = 1008;
            break;
        }
        let deadline = *access.deadline.borrow();
        tokio::select! {
            biased;
            _=access.cancelled.cancelled()=>{close_code=1008;break},
            changed=access.deadline.changed()=>{if changed.is_err(){close_code=1008;break}},
            _=tokio::time::sleep_until(deadline)=>{close_code=1008;break},
            incoming=source.next()=>match incoming {
                Some(Ok(Message::Text(text)))=>{
                    let Ok(message)=serde_json::from_str::<BrowserMessage>(&text)else{break};
                    let Ok(frame)=browser_frame(id,message)else{break};
                    match access.run(input_tx.send(Ok(frame)),Duration::from_secs(5)).await {
                        Ok(Ok(()))=>{},
                        Err(error)if error.code()==tonic::Code::PermissionDenied=>{close_code=1008;break},
                        Err(_)=>{close_code=1013;break},
                        _=>{close_code=1012;break},
                    }
                },
                Some(Ok(Message::Ping(_)|Message::Pong(_)))=>{},
                _=>break,
            },
            outgoing=output.recv()=>{
                let Some(outgoing)=outgoing else{close_code=1012;break};
                let value=match outgoing{Ok(frame)=>browser_output(frame),Err(error)=>{
                    if matches!(error.code(),tonic::Code::PermissionDenied|tonic::Code::Unauthenticated){close_code=1008;break}
                    if error.code()!=tonic::Code::Aborted{close_code=1012;break}
                    json!({"type":"exit","reason":"attachment_superseded"})
                }};
                let finished=value["type"]=="exit";
                let sent=match access.run(sink.send(Message::Text(value.to_string().into())),Duration::from_secs(5)).await{
                    Ok(Ok(()))=>true,
                    Err(error)if error.code()==tonic::Code::PermissionDenied=>{close_code=1008;false},
                    Err(_)=>{close_code=1013;false},
                    _=>{close_code=1012;false},
                };
                if !sent{break}
                if finished{break}
            },
        }
    }
    drop(input_tx);
    drop(output);
    if close_code != 1000 {
        let _ = tokio::time::timeout(
            Duration::from_millis(100),
            sink.send(Message::Close(Some(axum::extract::ws::CloseFrame {
                code: close_code,
                reason: match close_code {
                    1008 => "authorization expired",
                    1013 => "forwarding congested",
                    _ => "forwarding interrupted",
                }
                .into(),
            }))),
        )
        .await;
    } else {
        let _ = tokio::time::timeout(Duration::from_millis(100), sink.close()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_data_is_binary_safe_and_protocol_is_closed() {
        let id = Uuid::new_v4();
        let bytes = vec![0, 255, 195, 169];
        let frame = browser_frame(
            id,
            BrowserMessage::Input {
                sequence: 1,
                data: STANDARD.encode(&bytes),
            },
        )
        .unwrap();
        assert_eq!(frame.data, bytes);
        assert!(
            serde_json::from_value::<BrowserMessage>(
                json!({"type":"connect_tcp","host":"127.0.0.1"})
            )
            .is_err()
        );
        assert!(
            browser_frame(
                id,
                BrowserMessage::Resize {
                    columns: 0,
                    rows: 24
                }
            )
            .is_err()
        );
        assert!(
            browser_frame(
                id,
                BrowserMessage::Input {
                    sequence: 1,
                    data: STANDARD.encode(vec![0; MAX_FRAME_BYTES + 1])
                }
            )
            .is_err()
        );
    }
}
