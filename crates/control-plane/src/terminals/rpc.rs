//! [INPUT]: 依赖真实设备证书、单次内部委托与终端会话围栏。
//! [OUTPUT]: 独立设备 Terminal RPC 和私有 TerminalRelay RPC。
//! [POS]: 每条流在首帧认证；之后保持有界帧转发。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{Frames, Runtime};
use crate::{
    protocol::{self, TerminalFrame, terminal::TerminalHello},
    state::AppState,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

#[derive(Clone)]
pub struct Device(pub AppState);
#[derive(Clone)]
pub struct Relay(pub AppState);

fn id(first: &TerminalFrame) -> Result<Uuid, Status> {
    Uuid::parse_str(&first.terminal_id).map_err(|_| Status::invalid_argument("invalid terminal ID"))
}
async fn initial(stream: &mut Streaming<TerminalFrame>) -> Result<TerminalFrame, Status> {
    tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .map_err(|_| Status::deadline_exceeded("terminal authentication timeout"))??
        .ok_or_else(|| Status::unauthenticated("terminal hello required"))
}
fn input(mut stream: Streaming<TerminalFrame>) -> Frames {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        loop {
            match stream.message().await {
                Ok(Some(frame)) => {
                    if protocol::terminal::validate_frame(&frame).is_err() {
                        let _ = tx
                            .send(Err(Status::invalid_argument("invalid terminal frame")))
                            .await;
                        break;
                    }
                    if tx.send(Ok(frame)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    break;
                }
            }
        }
    });
    rx
}

#[tonic::async_trait]
impl protocol::terminal_server::Terminal for Device {
    type ConnectStream = super::access::AuthorizedFrames;
    async fn connect(
        &self,
        request: Request<Streaming<TerminalFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let node = tokio::time::timeout(
            Duration::from_secs(2),
            crate::grpc::identity(&self.0, &request),
        )
        .await
        .map_err(|_| Status::deadline_exceeded("device authentication timeout"))??;
        let mut stream = request.into_inner();
        let first = initial(&mut stream).await?;
        protocol::terminal::validate_frame(&first)
            .map_err(|_| Status::invalid_argument("invalid terminal hello"))?;
        if first.kind != "hello" {
            return Err(Status::unauthenticated("terminal hello required"));
        }
        let id = id(&first)?;
        let hello: TerminalHello = serde_json::from_str(&first.payload_json)
            .map_err(|_| Status::invalid_argument("invalid terminal hello"))?;
        let output = super::device(&self.0, id, node, hello, input(stream)).await?;
        let access = super::access::Access::new(self.0.clone(), id).await?;
        Ok(Response::new(access.stream(output)))
    }
}

#[tonic::async_trait]
impl protocol::terminal_relay_server::TerminalRelay for Relay {
    type ConnectStream = super::access::AuthorizedFrames;
    async fn connect(
        &self,
        request: Request<Streaming<TerminalFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let signature = request
            .metadata()
            .get("x-terminal-signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| Status::unauthenticated("relay signature required"))?
            .to_owned();
        let runtime = Runtime::get(&self.0).await.map_err(crate::grpc::internal)?;
        let permit = runtime
            .incoming
            .clone()
            .try_acquire_owned()
            .map_err(|_| Status::resource_exhausted("terminal forwarding capacity reached"))?;
        let mut stream = request.into_inner();
        let first = initial(&mut stream).await?;
        let id = id(&first)?;
        let delegation = super::relay::verify(&self.0, &runtime, &first, &signature).await?;
        let session = self
            .0
            .store
            .terminal_session(id)
            .await
            .map_err(|_| Status::permission_denied("terminal unavailable"))?;
        if session.owner != self.0.coordination.id() {
            return Err(Status::permission_denied("relay owner mismatch"));
        }
        let incoming = input(stream);
        let output = match delegation.role.as_str() {
            "browser" => {
                let context = delegation.context;
                let user = context["user_id"]
                    .as_str()
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .ok_or_else(|| Status::unauthenticated("invalid delegated user"))?;
                let credential = context["credential"]
                    .as_str()
                    .ok_or_else(|| Status::unauthenticated("delegated session required"))?
                    .to_owned();
                super::browser_local(
                    &self.0,
                    &runtime,
                    session,
                    user,
                    credential,
                    context["authenticate"].clone(),
                    incoming,
                )
                .await?
            }
            "device" => {
                let hello: TerminalHello = serde_json::from_value(delegation.context)
                    .map_err(|_| Status::invalid_argument("invalid delegated device"))?;
                if hello.node_id != session.node_id
                    || hello.node_session != session.node_session
                    || hello.startup_id != session.startup_id
                    || !super::authorized(&self.0, id).await
                {
                    return Err(Status::permission_denied("delegated device rejected"));
                }
                super::device_local(&self.0, &runtime, session, incoming).await?
            }
            _ => return Err(Status::permission_denied("unknown relay role")),
        };
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let _permit = permit;
            let mut output = output;
            while let Some(frame) = output.recv().await {
                if tx.send(frame).await.is_err() {
                    break;
                }
            }
        });
        let access = super::access::Access::new(self.0.clone(), id).await?;
        Ok(Response::new(access.stream(rx)))
    }
}
