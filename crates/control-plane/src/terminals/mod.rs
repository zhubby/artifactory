//! [INPUT]: 依赖持久会话、设备连接、内部认证流与有界内存通道。
//! [OUTPUT]: 提供终端运行时、浏览器和设备流路由及后台清扫。
//! [POS]: Control 只认证和转发设备终端，不启动本地进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub(crate) mod access;
mod broker;
mod relay;
pub mod rpc;

use crate::{
    protocol::TerminalFrame,
    state::AppState,
    store::terminals::{TerminalInstance, TerminalSession},
};
use ed25519_dalek::SigningKey;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, mpsc};
use tonic::Status;
use uuid::Uuid;

pub type Frames = mpsc::Receiver<Result<TerminalFrame, Status>>;
pub type FrameSender = mpsc::Sender<Result<TerminalFrame, Status>>;

pub struct Runtime {
    instance: TerminalInstance,
    signing: SigningKey,
    brokers: Mutex<HashMap<Uuid, mpsc::Sender<broker::Event>>>,
    nonces: Mutex<HashMap<Uuid, std::time::Instant>>,
    pub(crate) incoming: Arc<tokio::sync::Semaphore>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalRuntime")
            .field("id", &self.instance.id)
            .finish_non_exhaustive()
    }
}

impl Runtime {
    pub async fn get(state: &AppState) -> anyhow::Result<Arc<Self>> {
        state
            .coordination
            .terminals
            .get_or_try_init(|| relay::start(state.clone()))
            .await
            .cloned()
    }

    async fn broker(
        self: &Arc<Self>,
        state: &AppState,
        session: TerminalSession,
    ) -> Result<mpsc::Sender<broker::Event>, Status> {
        let mut brokers = self.brokers.lock().await;
        if let Some(broker) = brokers.get(&session.id) {
            return Ok(broker.clone());
        }
        if brokers.len() >= 64 {
            return Err(Status::resource_exhausted(
                "terminal owner capacity reached",
            ));
        }
        let (tx, rx) = mpsc::channel(64);
        brokers.insert(session.id, tx.clone());
        let runtime = self.clone();
        let state = state.clone();
        tokio::spawn(async move {
            let id = session.id;
            broker::run(state, session, rx).await;
            runtime.brokers.lock().await.remove(&id);
        });
        Ok(tx)
    }
}

pub async fn start(state: AppState) -> anyhow::Result<()> {
    let _ = Runtime::get(&state).await?;
    Ok(())
}

pub(crate) fn frame(id: Uuid, kind: &str) -> TerminalFrame {
    TerminalFrame {
        terminal_id: id.to_string(),
        kind: kind.into(),
        ..Default::default()
    }
}

pub(crate) async fn authorized(state: &AppState, id: Uuid) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(1), state.store.terminal_authorized(id)).await,
        Ok(Ok(true))
    )
}

pub(crate) async fn browser(
    state: &AppState,
    id: Uuid,
    user: Uuid,
    credential: String,
    first: serde_json::Value,
    input: Frames,
) -> Result<Frames, Status> {
    let runtime = Runtime::get(state).await.map_err(crate::grpc::internal)?;
    let session = state
        .store
        .terminal_session(id)
        .await
        .map_err(|_| Status::permission_denied("terminal unavailable"))?;
    if session.owner != state.coordination.id() {
        return relay::forward(
            state,
            &runtime,
            session.owner,
            id,
            "browser",
            serde_json::json!({"user_id":user,"credential":credential,"authenticate":first}),
            input,
        )
        .await;
    }
    browser_local(state, &runtime, session, user, credential, first, input).await
}

async fn browser_local(
    state: &AppState,
    runtime: &Arc<Runtime>,
    session: TerminalSession,
    user: Uuid,
    credential: String,
    first: serde_json::Value,
    mut input: Frames,
) -> Result<Frames, Status> {
    let renderer = first["renderer_id"]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or_else(|| Status::invalid_argument("invalid renderer"))?;
    let ticket = first["ticket"]
        .as_str()
        .ok_or_else(|| Status::unauthenticated("terminal ticket required"))?;
    let offset = first["offset"].as_u64().unwrap_or(0);
    let session = state
        .store
        .consume_terminal_ticket(session.id, user, &credential, renderer, ticket)
        .await
        .map_err(|_| Status::permission_denied("terminal ticket rejected"))?;
    let id = session.id;
    let generation = session.generation;
    let broker = runtime.broker(state, session).await?;
    let (tx, rx) = mpsc::channel(32);
    broker
        .send(broker::Event::Browser {
            sender: tx,
            generation,
            offset,
        })
        .await
        .map_err(|_| Status::unavailable("terminal closed"))?;
    tokio::spawn(async move {
        while let Some(item) = input.recv().await {
            match item {
                Ok(frame) => {
                    if broker
                        .send(broker::Event::BrowserFrame { frame, generation })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = broker.send(broker::Event::Detached { generation }).await;
        let _ = id;
    });
    Ok(rx)
}

pub(crate) async fn device(
    state: &AppState,
    id: Uuid,
    node: Uuid,
    hello: crate::protocol::terminal::TerminalHello,
    input: Frames,
) -> Result<Frames, Status> {
    let runtime = Runtime::get(state).await.map_err(crate::grpc::internal)?;
    let session = state
        .store
        .terminal_session(id)
        .await
        .map_err(|_| Status::permission_denied("terminal unavailable"))?;
    if session.node_id != node
        || hello.node_id != node
        || hello.node_session != session.node_session
        || hello.startup_id != session.startup_id
        || !authorized(state, id).await
    {
        return Err(Status::permission_denied("terminal identity rejected"));
    }
    if session.owner != state.coordination.id() {
        return relay::forward(
            state,
            &runtime,
            session.owner,
            id,
            "device",
            serde_json::to_value(hello).map_err(crate::grpc::internal)?,
            input,
        )
        .await;
    }
    device_local(state, &runtime, session, input).await
}

async fn device_local(
    state: &AppState,
    runtime: &Arc<Runtime>,
    session: TerminalSession,
    mut input: Frames,
) -> Result<Frames, Status> {
    let broker = runtime
        .brokers
        .lock()
        .await
        .get(&session.id)
        .cloned()
        .ok_or_else(|| Status::failed_precondition("browser must attach first"))?;
    let (tx, rx) = mpsc::channel(32);
    let (accepted, received) = tokio::sync::oneshot::channel();
    broker
        .send(broker::Event::Device {
            sender: tx,
            accepted,
        })
        .await
        .map_err(|_| Status::unavailable("terminal closed"))?;
    received
        .await
        .map_err(|_| Status::unavailable("terminal closed"))??;
    let id = session.id;
    tokio::spawn(async move {
        while let Some(item) = input.recv().await {
            match item {
                Ok(frame) => {
                    if broker
                        .send(broker::Event::DeviceFrame(frame))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = broker.send(broker::Event::DeviceDetached).await;
        let _ = id;
    });
    let _ = state;
    Ok(rx)
}
