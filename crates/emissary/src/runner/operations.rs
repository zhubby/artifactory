//! [INPUT]: 依赖当前认证流、单调服务器时钟和有限操作耐久运行时。
//! [OUTPUT]: 隔离状态授权与执行授权，关闭时等待子进程清理和耐久终态。
//! [POS]: Runner 与操作日志之间的旁路；文件和工具I/O不占租约循环。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    config::Config,
    operations::{Identity, InventorySink, Runtime, ServerClock},
    protocol as pb,
};
use artifactory_emissary_protocol::operations::*;
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) enum Input {
    Connected {
        sender: mpsc::Sender<pb::EmissaryEvent>,
        session: Uuid,
        server_time_ms: i64,
        started: Instant,
        received: Instant,
    },
    Disconnected,
    Command(Box<pb::Command>),
    Drain(bool),
    Fence(tokio::sync::oneshot::Sender<bool>),
    Stop,
}
pub(super) struct Handle {
    sender: mpsc::Sender<Input>,
    stop: CancellationToken,
    task: Option<tokio::task::JoinHandle<anyhow::Result<()>>>,
    maintenance: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl Handle {
    pub fn try_send(&self, input: Input) -> Result<(), mpsc::error::TrySendError<Input>> {
        self.sender.try_send(input)
    }
    pub async fn send(&self, input: Input) -> Result<(), mpsc::error::SendError<Input>> {
        self.sender.send(input).await
    }
    pub fn maintenance(&self, closed: bool) {
        self.maintenance
            .store(closed, std::sync::atomic::Ordering::SeqCst);
    }
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        self.stop.cancel();
        let mut task = self.task.take().expect("operation bridge task");
        tokio::time::timeout(Duration::from_secs(12), &mut task)
            .await
            .map_err(|_| anyhow::anyhow!("operation shutdown deadline exceeded"))?
            .map_err(|_| anyhow::anyhow!("operation bridge failed during shutdown"))?
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
pub(super) fn start(
    cfg: Config,
    startup: Uuid,
    sink: Option<InventorySink>,
    status: std::sync::Arc<std::sync::atomic::AtomicBool>,
    expired: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> Handle {
    let (send, mut receive) = mpsc::channel(32);
    let stop = CancellationToken::new();
    let stopping = stop.clone();
    let maintenance_gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let maintenance = maintenance_gate.clone();
    let task = tokio::spawn(async move {
        let root = cfg.root.join("operations");
        let runtime = match tokio::task::spawn_blocking(move || Runtime::open(root, startup)).await
        {
            Ok(Ok(runtime)) => runtime,
            _ => {
                anyhow::bail!("node operation journal unavailable");
            }
        };
        let mut outbound = None::<mpsc::Sender<pb::EmissaryEvent>>;
        let mut session = None;
        let mut clock = None;
        let mut identities = HashMap::<Uuid, Identity>::new();
        let mut authorized = HashSet::<Uuid>::new();
        let mut executable = HashSet::<Uuid>::new();
        let mut pending_cancels = HashMap::<Uuid, OperationCancel>::new();
        let mut cancel_overflow = false;
        let mut maintenance = Instant::now();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _=stopping.cancelled()=>break,
                _=tick.tick()=>{},
                message=receive.recv()=>match message {
                    Some(Input::Connected{sender,session:current,server_time_ms,started,received})=>{
                        authorized.clear();executable.clear();identities.clear();pending_cancels.clear();cancel_overflow=false;outbound=Some(sender);session=Some(current);clock=ServerClock::new(server_time_ms,started,received).ok();
                    },
                    Some(Input::Disconnected)=>{outbound=None;session=None;clock=None;authorized.clear();executable.clear();identities.clear();pending_cancels.clear();cancel_overflow=false;},
                    Some(Input::Fence(reply))=>{
                        let active=runtime.active().await;
                        status.store(active,std::sync::atomic::Ordering::SeqCst);
                        let _=reply.send(active);
                    },
                    Some(Input::Drain(true))=>{let _=runtime.drain().await;},
                    Some(Input::Drain(false))=>runtime.resume().await,
                    Some(Input::Command(command))=>{
                        let Some(current)=session else {continue;};
                        match command.kind.as_str() {
                            "node_operation_resume"=>{
                                let Ok(resume)=serde_json::from_str::<OperationResume>(&command.payload_json) else {continue;};
                                let Ok(node)=cfg.emissary_id.parse() else {continue;};
                                let identity=Identity{tenant_id:resume.tenant_id,node_id:node,startup_id:startup,control_session:current};
                                if let Ok(update)=runtime.reauthorize(&resume,&identity).await {
                                    authorized.insert(update.operation_id);identities.insert(update.operation_id,identity);
                                    if let Some(tx)=&outbound {let _=tx.try_send(event(&cfg,&update));}
                                }
                            },
                            "node_operation"=>{
                                let Ok(envelope)=serde_json::from_str::<OperationEnvelope>(&command.payload_json) else {continue;};
                                // 租户来自已认证Control的冻结信封；设备身份和会话必须逐项匹配。
                                let Ok(node)=cfg.emissary_id.parse() else {continue;};
                                let identity=Identity{tenant_id:envelope.tenant_id,node_id:node,startup_id:startup,control_session:current};
                                let id=envelope.operation_id;
                                let acceptance=if maintenance_gate.load(std::sync::atomic::Ordering::SeqCst) || cancel_overflow {Err(OperationReason::Busy)} else {runtime.accept(envelope,&identity,clock.as_ref()).await};
                                let mut update=match acceptance {
                                    Ok(update)=>{authorized.insert(id);executable.insert(id);identities.insert(id,identity.clone());update},
                                    Err(reason)=>match runtime.status(id).await {
                                        Some(update) if authorized.contains(&id)=>update,
                                        // 既有状态不受拒收覆盖；新连接须先完成精确再授权。
                                        Some(_)=>continue,
                                        None=>OperationUpdate{operation_id:id,startup_id:startup,sequence:1,state:OperationState::Rejected,reason:Some(reason),result:Some(OperationResult::default()),result_digest:OperationResult::default().digest().ok()},
                                    },
                                };
                                if let Some(cancel)=pending_cancels.get(&id).cloned() && authorized.contains(&id) {
                                    executable.remove(&id);
                                    if let Ok(canceled)=runtime.cancel(&cancel,&identity).await {update=canceled;pending_cancels.remove(&id);}
                                }
                                if let Some(tx)=&outbound {let _=tx.try_send(event(&cfg,&update));}
                            },
                            "node_operation_cancel"=>{
                                if let Ok(cancel)=serde_json::from_str::<OperationCancel>(&command.payload_json) && cancel.control_session==current && cancel.startup_id==startup {
                                    executable.remove(&cancel.operation_id);
                                    if pending_cancels.len()<64 || pending_cancels.contains_key(&cancel.operation_id) {pending_cancels.insert(cancel.operation_id,cancel.clone());}
                                    else {cancel_overflow=true;}
                                    if let Some(identity)=identities.get(&cancel.operation_id) && runtime.cancel(&cancel,identity).await.is_ok() {pending_cancels.remove(&cancel.operation_id);}
                                }
                            },
                            "node_operation_result_ack"=>{if let Ok(ack)=serde_json::from_str::<OperationAck>(&command.payload_json) && authorized.contains(&ack.operation_id) {let _=runtime.ack(&ack).await;}},
                            _=>{},
                        }
                    },
                    Some(Input::Stop)|None=>break,
                }
            }
            if maintenance.elapsed() >= Duration::from_secs(60) {
                if let Ok(count) = runtime.maintenance().await {
                    expired.store(count, std::sync::atomic::Ordering::Relaxed);
                }
                maintenance = Instant::now();
            }
            status.store(runtime.active().await, std::sync::atomic::Ordering::Relaxed);
            if let Some(tx) = &outbound {
                for update in runtime
                    .updates()
                    .await
                    .into_iter()
                    .filter(|u| authorized.contains(&u.operation_id))
                {
                    if tx.try_send(event(&cfg, &update)).is_err() {
                        break;
                    }
                    if update.state == OperationState::Accepted
                        && executable.contains(&update.operation_id)
                        && !maintenance_gate.load(std::sync::atomic::Ordering::SeqCst)
                        && runtime
                            .start(update.operation_id, cfg.clone(), sink.clone())
                            .await
                            .is_ok()
                    {
                        executable.remove(&update.operation_id);
                    }
                }
            }
        }
        let result = runtime.shutdown(Duration::from_secs(10)).await;
        status.store(runtime.active().await, std::sync::atomic::Ordering::Relaxed);
        result
    });
    Handle {
        sender: send,
        stop,
        task: Some(task),
        maintenance,
    }
}
fn event(cfg: &Config, update: &OperationUpdate) -> pb::EmissaryEvent {
    pb::EmissaryEvent {
        emissary_id: cfg.emissary_id.clone(),
        kind: "node_operation_update".into(),
        protocol_major: 1,
        payload_json: serde_json::to_string(update).expect("typed update"),
        ..Default::default()
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
