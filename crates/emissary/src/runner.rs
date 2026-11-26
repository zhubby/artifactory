//! [INPUT]: 依赖 mTLS 会话、固定主机诊断、遥测缓存和持久化事件队列。
//! [OUTPUT]: 提供任务领取、租约心跳、OTA保存故障重试与独立Worker监督。
//! [POS]: 节点主进程；向 Worker 仅交付被授权的本机方案。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    client,
    config::{Config, atomic_json, private_dir},
    doctor, ota, protocol as pb, source,
    worker::{Event, Input, Lease},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

mod command_gate;
mod operations;
mod ota_cache;
mod state;
mod worker;
use worker::worker_task;

struct Task {
    token: String,
    assignment: pb::Assignment,
    lease: Lease,
    started: bool,
    control: Option<mpsc::Sender<Input>>,
    pending: Vec<pb::EmissaryEvent>,
    sequence: u64,
    sent_sequence: u64,
    replay_at: Option<Instant>,
    last_accept_at: Option<Instant>,
    log_sequence: u64,
    finished_at: Option<Instant>,
    buffer_error: bool,
    logs_abandoned: bool,
}
impl Task {
    fn new(token: String, assignment: pb::Assignment, seconds: u32, now: Instant) -> Self {
        Self {
            token,
            assignment,
            lease: Lease::new(now, seconds),
            started: false,
            control: None,
            pending: vec![],
            sequence: 0,
            sent_sequence: 0,
            replay_at: None,
            last_accept_at: None,
            log_sequence: 0,
            finished_at: None,
            buffer_error: false,
            logs_abandoned: false,
        }
    }
}
#[derive(Serialize, Deserialize)]
struct LogRecord {
    sequence: u64,
    step: String,
    data: Vec<u8>,
}
struct Spool {
    root: PathBuf,
    bytes: u64,
    records: u64,
    limit: u64,
}
impl Spool {
    fn new(root: PathBuf, limit: u64) -> Result<Self> {
        private_dir(&root.join("spool"))?;
        let bytes = walkdir::WalkDir::new(root.join("spool"))
            .into_iter()
            .try_fold(0, |sum, entry| {
                let entry = entry?;
                Ok::<_, anyhow::Error>(
                    sum + if entry.file_type().is_file() {
                        entry.metadata()?.len()
                    } else {
                        0
                    },
                )
            })?;
        let records = walkdir::WalkDir::new(root.join("spool"))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count() as u64;
        Ok(Self {
            root,
            bytes,
            records,
            limit,
        })
    }
    fn append(&mut self, id: &str, sequence: &mut u64, step: &str, data: &[u8]) -> Result<()> {
        let record = LogRecord {
            sequence: *sequence + 1,
            step: step.into(),
            data: data.into(),
        };
        let bytes = serde_json::to_vec(&record)?;
        ensure!(
            self.bytes + bytes.len() as u64 <= self.limit,
            "log buffer exhausted"
        );
        crate::config::atomic_bytes(
            &self
                .root
                .join("spool")
                .join(id)
                .join(format!("{:020}.json", record.sequence)),
            &bytes,
            false,
        )?;
        *sequence = record.sequence;
        self.bytes += bytes.len() as u64;
        self.records = self.records.saturating_add(1);
        Ok(())
    }
    fn batch(&self, id: &str) -> Result<Vec<LogRecord>> {
        let dir = self.root.join("spool").join(id);
        if !dir.try_exists()? {
            return Ok(vec![]);
        }
        let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        entries
            .into_iter()
            .take(32)
            .map(|e| Ok(serde_json::from_slice(&fs::read(e.path())?)?))
            .collect()
    }
    fn empty(&self, id: &str) -> bool {
        fs::read_dir(self.root.join("spool").join(id))
            .map(|mut e| e.next().is_none())
            .unwrap_or(false)
            || !self.root.join("spool").join(id).exists()
    }
    fn ack(&mut self, id: &str, through: u64) -> Result<()> {
        let dir = self.root.join("spool").join(id);
        if !dir.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".json")
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|seq| seq <= through)
            {
                let size = entry.metadata()?.len();
                fs::remove_file(entry.path())?;
                self.bytes = self.bytes.saturating_sub(size);
                self.records = self.records.saturating_sub(1);
            }
        }
        Ok(())
    }
}
struct State {
    cfg: Config,
    tasks: HashMap<String, Task>,
    anchors: HashMap<String, Instant>,
    spool: Spool,
    diagnostics: Value,
    capabilities: Vec<String>,
    workers: usize,
    stopping: bool,
    ota_directory: Option<PathBuf>,
    ota_session: String,
    health_at: Option<Instant>,
    drain_request: String,
    drain_update: String,
    pending_ota: Option<(ota::Request, Instant)>,
    telemetry: Option<crate::telemetry::Handle>,
    startup: Option<artifactory_emissary_protocol::nodes::MetadataSnapshot>,
    metadata_acked: bool,
    terminal_workers: usize,
    command_gate: command_gate::Gate,
    operation_active: std::sync::Arc<std::sync::atomic::AtomicBool>,
    operation_expired: std::sync::Arc<std::sync::atomic::AtomicU64>,
    ota_cached: ota_cache::Snapshot,
}
enum Action {
    Start(String, u32),
    Control(String, Box<Input>),
}

enum Message {
    Connected(String, mpsc::Sender<pb::EmissaryEvent>),
    Disconnected(String),
    Command(String, Box<pb::Command>),
    Event(String, Event),
    Exit(String),
    Logs(String, Option<u64>),
    OtaRefresh,
    MaintenanceReady(uuid::Uuid),
}
async fn connect(
    cfg: Config,
    session: String,
    hello: pb::EmissaryEvent,
    send: mpsc::Sender<Message>,
    cancel: CancellationToken,
) {
    let result = async {
        let mut control = client::control(&cfg).await?;
        let (tx, rx) = mpsc::channel(256);
        tx.send(hello).await?;
        let mut stream = control.connect(ReceiverStream::new(rx)).await?.into_inner();
        send.send(Message::Connected(session.clone(), tx)).await?;
        while let Some(command) = stream.message().await? {
            send.send(Message::Command(session.clone(), Box::new(command)))
                .await?;
        }
        Ok::<_, anyhow::Error>(())
    };
    tokio::select! {
        _=cancel.cancelled()=>{},
        outcome=result=>if let Err(error)=outcome {
            eprintln!("control stream ended: {error}");
        },
    }
    let _ = send.send(Message::Disconnected(session)).await;
}
pub async fn run(cfg: Config, cancel: CancellationToken) -> Result<()> {
    ensure!(
        source::valid_id(&cfg.emissary_id),
        "Emissary must be registered first"
    );
    private_dir(&cfg.root)?;
    let _runner_lock = source::lock(
        &cfg.root.join("runner.lock"),
        &cancel,
        Duration::from_secs(1),
    )
    .await?;
    source::sweep(&cfg).await?;
    crate::terminal::sweep(&cfg).await?;
    crate::signing::sweep(&cfg).await?;
    let (diagnostics, mut capabilities) = doctor::doctor(&cfg).await;
    capabilities.push(artifactory_emissary_protocol::commands::CAPABILITY.into());
    capabilities.push(artifactory_emissary_protocol::commands::NOTIFICATION_CAPABILITY.into());
    capabilities.push(artifactory_emissary_protocol::operations::CAPABILITY.into());
    if std::env::var_os("ARTIFACTORY_OTA_DIRECTORY").is_some() {
        capabilities.push("emissary-ota-v1".into());
        capabilities.push(artifactory_emissary_protocol::ota::CAPABILITY.into());
    }
    let terminal_capabilities = crate::terminal::capabilities(&cfg.terminal).await?;
    if terminal_capabilities.enabled {
        capabilities.push(artifactory_emissary_protocol::terminal::CAPABILITY.into());
    }
    let mut state = State::new(cfg.clone(), diagnostics, capabilities)?;
    let startup_id = uuid::Uuid::new_v4();
    let (mut snapshot, inventory) = crate::telemetry::startup(
        &cfg,
        startup_id,
        &state.diagnostics,
        state.capabilities.clone(),
    )
    .await?;
    if terminal_capabilities.enabled {
        snapshot.metadata.insert(
            "emissary.terminal".into(),
            serde_json::to_value(&terminal_capabilities)?,
        );
        snapshot.validate()?;
    }
    let mut terminals =
        crate::terminal::Supervisor::new(cfg.clone(), startup_id, terminal_capabilities.account);
    state.startup = Some(snapshot);
    if !cfg.telemetry_endpoint.is_empty() {
        state.telemetry = Some(crate::telemetry::Handle::start(
            cfg.clone(),
            startup_id,
            inventory,
        ));
    }
    let operations = operations::start(
        cfg.clone(),
        startup_id,
        state.telemetry.as_ref().map(|h| h.inventory_sink()),
        state.operation_active.clone(),
        state.operation_expired.clone(),
    );
    let (send, mut recv) = mpsc::channel(256);
    let mut jobs = JoinSet::new();
    let transport = CancellationToken::new();
    let mut outbound: Option<mpsc::Sender<pb::EmissaryEvent>> = None;
    let mut connecting = false;
    let mut session = String::new();
    let mut retry = Instant::now();
    let mut backoff = 1;
    let mut log_busy = HashSet::new();
    let (ota_cache_input, mut ota_cache_output) =
        ota_cache::start(state.ota_directory.clone(), &mut jobs);
    if state.ota_directory.is_some() {
        // 首连前等待一次后台读取，避免idle启动器被未知缓存暂时拒收。
        let _ = tokio::time::timeout(Duration::from_secs(2), ota_cache_output.changed()).await;
        state.ota_cached = ota_cache_output.borrow().clone();
    }
    let mut maintenance_ack = None;
    let mut maintenance_pending = None;
    let mut ota_refresh_busy = false;
    let mut ota_refresh_at = Instant::now();
    // Tonic reconnects this independent HTTP/2 channel. Share cheap client
    // clones across upload batches instead of rebuilding TLS for every batch.
    let log_transport = std::sync::Arc::new(tokio::sync::OnceCell::new());
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut heartbeat = Instant::now();
    let mut shutdown: Option<Instant> = None;
    loop {
        if !connecting && outbound.is_none() && Instant::now() >= retry && shutdown.is_none() {
            session = uuid::Uuid::new_v4().to_string();
            if state.reset_ota_session(&session) {
                connecting = true;
                state.metadata_acked = false;
                state.command_gate.reset(Instant::now());
                let hello = state.heartbeat("hello");
                let cfg = cfg.clone();
                let send = send.clone();
                let session = session.clone();
                let token = transport.clone();
                jobs.spawn(connect(cfg, session, hello, send, token));
            } else {
                retry = Instant::now() + Duration::from_secs(1);
            }
        }
        tokio::select! {
            _=cancel.cancelled(),if shutdown.is_none()=>{state.stop();terminals.drain();let _=operations.try_send(operations::Input::Stop);shutdown=Some(Instant::now()+Duration::from_secs(5));},
            _=tick.tick()=>{
                crate::signing::ready(&cfg)?;
                state.ota_cached=ota_cache_output.borrow().clone();
                state.retry_pending_ota(Instant::now());
                let maintenance=state.ota_cached.maintenance || !state.ota_cached.fresh() && state.ota_directory.is_some();
                operations.maintenance(maintenance);
                if !maintenance {maintenance_ack=None;maintenance_pending=None;}
                if let Some(nonce)=state.ota_cached.maintenance_nonce && maintenance_ack!=Some(nonce) && maintenance_pending!=Some(nonce) {
                    let (ready,wait)=tokio::sync::oneshot::channel();
                    if operations.try_send(operations::Input::Fence(ready)).is_ok() {
                        maintenance_pending=Some(nonce);let send=send.clone();
                        jobs.spawn(async move {if wait.await.is_ok() {let _=send.send(Message::MaintenanceReady(nonce)).await;}});
                    }
                }
                terminals.tick();
                // 维护仅关闭准入；已有操作不因维护被取消。
                let ota_drain=state.stopping || state.ota_cached.state.as_ref().is_some_and(|state|state.active());
                let _=operations.try_send(operations::Input::Drain(ota_drain));
                state.terminal_workers=terminals.len();
                terminals.set_draining(state.terminal_draining() || source::ready(&cfg).is_err());
                let now=Instant::now();
                ota_cache_input.send_replace((state.drain_status(),state.command_gate.ota_observability,maintenance_ack));
                if state.command_gate.ota_observability && outbound.is_some() && !ota_refresh_busy && now>=ota_refresh_at && let Some(directory)=state.ota_directory.clone() && let Some(control_session)=state.command_gate.control_session {
                    ota_refresh_busy=true;let cfg=cfg.clone();let send=send.clone();
                    jobs.spawn(async move {let _=tokio::time::timeout(Duration::from_secs(8),ota::forward_url_refresh(&cfg,&directory,&control_session.to_string())).await;let _=send.send(Message::OtaRefresh).await;});
                }
                let expired=state.tasks.iter().filter(|(_,task)|task.lease.expired(now)).map(|(id,_)|id.clone()).collect::<Vec<_>>();
                for id in expired {state.drop_task(&id,"execution lease expired")?;}
                for task in state.tasks.values_mut() {
                    if task.finished_at.is_some_and(|at|now.saturating_duration_since(at)>Duration::from_secs(120)) && !task.logs_abandoned {
                        task.logs_abandoned=true;
                        for event in &mut task.pending {if event.kind=="finished" {event.payload_json=json!({"status":"failed","error":"log delivery timed out; unacknowledged chunks retained on Emissary"}).to_string();}}
                    }
                }
                for (id,task) in &state.tasks {
                    if !log_busy.contains(id)
                        && let Ok(records)=state.spool.batch(id)&& !records.is_empty() {
                            log_busy.insert(id.clone());let cfg=cfg.clone();let id=id.clone();let token=task.token.clone();let send=send.clone();let log_transport=log_transport.clone();
                            jobs.spawn(async move {
                                let operation=async {
                                    let mut control=log_transport.get_or_try_init(||client::control(&cfg)).await?.clone();
                                    let max=records.last().map(|r|r.sequence).unwrap_or(0);
                                    let chunks=records.into_iter().map(|r|pb::LogChunk{attempt_id:id.clone(),lease_token:token.clone(),sequence:r.sequence,step:r.step,data:r.data}).collect::<Vec<_>>();
                                    let ack=control.upload_logs(tokio_stream::iter(chunks)).await?.into_inner();
                                    Ok::<_,anyhow::Error>(ack.through_sequence.min(max))
                                };
                                let through=tokio::time::timeout(Duration::from_secs(20),operation).await.ok().and_then(Result::ok);
                                let _=send.send(Message::Logs(id,through)).await;
                            });
                        }
                }
                if now>=heartbeat {if let Some(tx)=&outbound {let _=tx.try_send(state.heartbeat("heartbeat")); if !state.metadata_acked && let Some(snapshot)=&state.startup { let _=tx.try_send(pb::EmissaryEvent { emissary_id:cfg.emissary_id.clone(), kind:"metadata".into(),protocol_major:1,payload_json:serde_json::to_string(snapshot)?,..Default::default() }); }}heartbeat=now+Duration::from_secs(10);}
                if let Some(deadline)=shutdown&& (now>=deadline || (state.workers==0 && state.tasks.values().all(|t|!t.started) && state.spool.bytes==0)) {break;}
            },
            Some(message)=recv.recv()=>match message {
                Message::Connected(id,tx) if id==session=>{
                    outbound=Some(tx);connecting=false;backoff=1;
                    for task in state.tasks.values_mut() {
                        task.sent_sequence=0;task.replay_at=None;task.last_accept_at=None;
                    }
                },
                Message::Disconnected(id) if id==session=>{let _=operations.try_send(operations::Input::Disconnected);terminals.disconnect();outbound=None;connecting=false;retry=Instant::now()+client::retry_delay(backoff);backoff=(backoff*2).min(30);if shutdown.is_none(){eprintln!("control connection interrupted; reconnecting");}},
                Message::Command(id,command) if id==session=>{
                    match state.command_gate.admit(&command, &cfg.emissary_id) {
                        command_gate::Admission::Proceed => {},
                        command_gate::Admission::Ignored => continue,
                        command_gate::Admission::Consumed => {
                            if let (Some(tx),Some(current),Some(server_time_ms),Some(started),Some(received))=(&outbound,state.command_gate.control_session,state.command_gate.server_time_ms,state.command_gate.started,state.command_gate.received) {
                                let _=operations.try_send(operations::Input::Connected{sender:tx.clone(),session:current,server_time_ms,started,received});
                            }
                            continue;
                        },
                        command_gate::Admission::Rejected(event) => {
                            if let (Some(event),Some(tx))=(event,&outbound) {let _=tx.try_send(event);}
                            continue;
                        },
                    }
                    if command.kind.starts_with("node_operation") {let _=operations.try_send(operations::Input::Command(command));continue;}
                    if command.kind == "metadata_ack" && command.request_id == startup_id.to_string()
                        && let Ok(control_session)=uuid::Uuid::parse_str(&command.signing_session_id) {
                        terminals.control_session(control_session);
                    }
                    if command.kind.starts_with("terminal_") {
                        terminals.set_draining(state.terminal_draining());
                        if terminals.command(&command).is_err() { eprintln!("device terminal command rejected"); }
                        state.terminal_workers=terminals.len();
                        continue;
                    }
                    let actions=state.command(*command,Instant::now())?;
                    for action in actions {match action {
                        Action::Control(id,input)=>if let Some(task)=state.tasks.get(&id)&& let Some(tx)=&task.control {let _=tx.try_send(*input);},
                        Action::Start(id,seconds)=>{
                            let task=state.tasks.get_mut(&id).context("missing started task")?;
                            let (tx,rx)=mpsc::channel(32);task.control=Some(tx);state.workers+=1;
                            let cfg=cfg.clone();let token=task.token.clone();let assignment=task.assignment.clone();let send=send.clone();
                            let registry=state.telemetry.as_ref().map(|h|h.registry.clone()).unwrap_or_default();
                            jobs.spawn(async move {
                                if worker_task(cfg,id.clone(),token,assignment,seconds,(rx,registry),send.clone()).await.is_err() {
                                    let _=send.send(Message::Event(id.clone(),Event{kind:"finished".into(),payload:json!({"status":"failed","error":"isolated worker exited without a terminal result","failure_kind":"emissary"}),..Default::default()})).await;
                                }
                                let _=send.send(Message::Exit(id)).await;
                            });
                        },
                    }}
                },
                Message::Event(id,event)=>state.event(&id,event)?,
                Message::Exit(id)=>{
                    state.workers=state.workers.saturating_sub(1);
                    if state.tasks.get(&id).is_some_and(|t|t.finished_at.is_none()) {state.event(&id,Event{kind:"finished".into(),payload:json!({"status":"failed","error":"worker exited without a terminal result","failure_kind":"emissary"}),..Default::default()})?;}
                },
                Message::MaintenanceReady(nonce)=>{if state.ota_cached.maintenance_nonce==Some(nonce) {maintenance_ack=Some(nonce);}maintenance_pending=None;},
                Message::OtaRefresh=>{ota_refresh_busy=false;ota_refresh_at=Instant::now()+Duration::from_secs(2);},
                Message::Logs(id,through)=>{log_busy.remove(&id);if let Some(through)=through {state.spool.ack(&id,through)?;}},
                _=>{},
            },
            Some(_)=jobs.join_next(),if !jobs.is_empty()=>{},
        }
        if let Some(tx) = &outbound {
            for event in state.pending() {
                let sequence = event.sequence;
                let id = event.attempt_id.clone();
                if tx.try_send(event).is_err() {
                    break;
                }
                if let Some(task) = state.tasks.get_mut(&id) {
                    task.sent_sequence = task.sent_sequence.max(sequence);
                }
            }
        }
    }
    drop(ota_cache_input);
    drop(ota_cache_output);
    let _ = operations.send(operations::Input::Stop).await;
    transport.cancel();
    state.stop();
    terminals.shutdown().await;
    // Give EOF-triggered worker cleanup a chance even when the control plane is down.
    let _ = tokio::time::timeout(Duration::from_secs(3), async {
        while jobs.join_next().await.is_some() {}
    })
    .await;
    jobs.shutdown().await;
    operations.shutdown().await?;
    if let Some(telemetry) = state.telemetry.take() {
        telemetry.shutdown().await?;
    }
    Ok(())
}
#[cfg(test)]
mod tests;
