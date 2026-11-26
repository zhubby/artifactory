//! [INPUT]: 依赖固定启动快照、构建任务、当前会话命令与OTA邮箱。
//! [OUTPUT]: 提供心跳、租约、OTA排空投影及保存失败后的安全重试。
//! [POS]: Runner的内存状态机；进程执行交由独立Worker。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
impl State {
    pub(super) fn new(cfg: Config, diagnostics: Value, capabilities: Vec<String>) -> Result<Self> {
        let spool = Spool::new(cfg.root.clone(), cfg.log_buffer_bytes)?;
        Ok(Self {
            cfg,
            tasks: HashMap::new(),
            anchors: HashMap::new(),
            spool,
            diagnostics,
            capabilities,
            workers: 0,
            stopping: false,
            ota_directory: std::env::var_os("ARTIFACTORY_OTA_DIRECTORY").map(PathBuf::from),
            ota_session: uuid::Uuid::new_v4().to_string(),
            health_at: None,
            drain_request: String::new(),
            drain_update: String::new(),
            pending_ota: None,
            telemetry: None,
            startup: None,
            metadata_acked: false,
            terminal_workers: 0,
            command_gate: command_gate::Gate::default(),
            operation_active: Default::default(),
            operation_expired: Default::default(),
            ota_cached: Default::default(),
        })
    }
    pub(super) fn heartbeat(&mut self, kind: &str) -> pb::EmissaryEvent {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Instant::now();
        let mut ota = self
            .ota_snapshot(&id)
            .and_then(|state| serde_json::to_value(state).ok());
        if self.command_gate.ota_observability
            && let Some(value) = &mut ota
            && let Some(observation) = &self.ota_cached.observation
        {
            value["observability"] =
                serde_json::to_value(observation).expect("typed OTA observation");
        }
        self.anchors
            .retain(|_, at| now.saturating_duration_since(*at) < Duration::from_secs(120));
        self.anchors.insert(id.clone(), now);
        let mut metrics = json!({});
        let mut network_interfaces = None;
        if let Some(snapshot) = self.telemetry.as_ref().and_then(|h| h.snapshot()) {
            metrics = snapshot.metrics;
            network_interfaces = snapshot.network_interfaces;
        }
        if let Some(value) = metrics.get_mut("telemetry") {
            *value = artifactory_emissary_protocol::nodes::delivery_diagnostics(value);
        }
        let mut diagnostics = json!({"emissary_version": self.diagnostics["emissary_version"],"hostname": self.diagnostics["hostname"]});
        if let Some(network_interfaces) = network_interfaces {
            diagnostics["network_interfaces"] = network_interfaces;
        }
        diagnostics["metrics"] = metrics;
        diagnostics["buffered_log_bytes"] = json!(self.spool.bytes);
        diagnostics["command_notifications_ignored"] =
            json!(self.command_gate.ignored_notifications);
        diagnostics["operation_result_expired"] = json!(
            self.operation_expired
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        pb::EmissaryEvent{emissary_id:self.cfg.emissary_id.clone(),kind:kind.into(),command_id:id,protocol_major:1,payload_json:json!({"active_attempts":self.tasks.iter().map(|(id,t)|json!({"attempt_id":id,"lease_token":t.token})).collect::<Vec<_>>(),"startup":if kind=="hello" {self.startup.as_ref()} else {None},"diagnostics":diagnostics,"ota":ota}).to_string(),..Default::default()}
    }
    pub(super) fn drain_status(&self) -> ota::Drain {
        let ids = self
            .tasks
            .keys()
            .filter_map(|id| id.parse::<uuid::Uuid>().ok())
            .collect::<Vec<_>>();
        let count = ids.len();
        ota::Drain {
            active_workers: self.workers as u32,
            pending_log_records: self.spool.records,
            pending_log_bytes: self.spool.bytes,
            pending_terminal_records: self
                .tasks
                .values()
                .map(|task| task.pending.len() as u32)
                .sum(),
            terminals: self.terminal_workers as u32,
            operations: u32::from(
                self.operation_active
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            waiting_ids: ids.into_iter().take(8).collect(),
            remaining_objects: count.saturating_sub(8) as u32,
        }
    }
    pub(super) fn terminal_draining(&self) -> bool {
        self.stopping
            || self.ota_directory.as_ref().is_some_and(|directory| {
                let _ = directory;
                !self.ota_cached.fresh()
                    || self.ota_cached.maintenance
                    || self
                        .ota_cached
                        .state
                        .as_ref()
                        .is_none_or(|state| state.active())
            })
    }
    pub(super) fn ota_snapshot(&mut self, request: &str) -> Option<ota::State> {
        self.ota_directory.as_ref()?;
        if !self.ota_cached.fresh() {
            return None;
        }
        let mut state = self.ota_cached.state.clone()?;
        if state.phase == "draining"
            && self.tasks.is_empty()
            && self.workers == 0
            && self.spool.bytes == 0
            && self.terminal_workers == 0
            && !self
                .operation_active
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            state.phase = "activating".into();
            state.activation_pending = true;
            self.drain_request = request.into();
            self.drain_update = state.update_id.clone();
        }
        Some(state)
    }
    pub(super) fn command(&mut self, command: pb::Command, now: Instant) -> Result<Vec<Action>> {
        self.stopping |= source::ready(&self.cfg).is_err();
        if command.kind == "metadata_ack" {
            self.metadata_acked = self
                .startup
                .as_ref()
                .is_some_and(|s| command.request_id == s.startup_id.to_string());
            return Ok(vec![]);
        }
        if command.kind == "emissary_health" && self.ota_directory.is_none() {
            return Ok(vec![]);
        }
        if let Some(directory) = &self.ota_directory {
            match command.kind.as_str() {
                "emissary_update" => {
                    let request: ota::Request = serde_json::from_str(&command.payload_json)?;
                    if self.pending_ota.as_ref().is_none_or(|(pending, _)| {
                        request.generation > pending.generation
                            || (request.generation == pending.generation
                                && request.id == pending.id
                                && pending.action != "cancel")
                    }) {
                        self.pending_ota = Some((request, now));
                    }
                    self.retry_pending_ota(now);
                    return Ok(vec![]);
                }
                "emissary_health" => {
                    if let Some(anchor) =
                        self.anchors.get(&command.request_id).copied().filter(|at| {
                            now.saturating_duration_since(*at) < Duration::from_secs(30)
                                && self.health_at.is_none_or(|last| *at > last)
                        })
                    {
                        if command.request_id == self.drain_request {
                            let Ok(state) = ota::snapshot(directory) else {
                                return Ok(vec![]);
                            };
                            if state.phase == "draining"
                                && state.update_id == self.drain_update
                                && atomic_json(
                                    &directory.join("drained.json"),
                                    &json!({"id":state.update_id}),
                                )
                                .is_err()
                            {
                                return Ok(vec![]);
                            }
                        }
                        if ota::confirm(directory, &self.ota_session, &command.request_id).is_ok() {
                            self.health_at = Some(anchor);
                        }
                    }
                    return Ok(vec![]);
                }
                _ => {}
            }
        }
        let id = command.attempt_id;
        if command.kind == "assign" {
            ensure!(source::valid_id(&id), "invalid attempt ID");
            if !self.stopping
                && !self.terminal_draining()
                && self.tasks.len() < 32
                && !self.tasks.contains_key(&id)
                && command.lease_seconds > 0
                && let Some(assignment) = command.assignment
            {
                self.tasks.insert(
                    id,
                    Task::new(command.lease_token, assignment, command.lease_seconds, now),
                );
            }
            return Ok(vec![]);
        }
        let Some(task) = self.tasks.get_mut(&id) else {
            return Ok(vec![]);
        };
        if command.kind == "event_ack" {
            let terminal = task
                .pending
                .iter()
                .any(|e| e.kind == "finished" && e.sequence <= command.ack_sequence);
            task.pending.retain(|e| e.sequence > command.ack_sequence);
            if terminal {
                self.tasks.remove(&id);
            }
            return Ok(vec![]);
        }
        ensure!(command.lease_token == task.token, "lease token mismatch");
        match command.kind.as_str() {
            "start" | "renew" => {
                if self.stopping && !task.started {
                    return Ok(vec![]);
                }
                let Some(anchor) = self.anchors.get(&command.request_id) else {
                    return Ok(vec![]);
                };
                if !task.lease.renew(now, *anchor, command.lease_seconds) {
                    self.drop_task(&id, "execution lease expired")?;
                    return Ok(vec![]);
                }
                let seconds = task
                    .lease
                    .deadline()
                    .saturating_duration_since(now)
                    .as_secs() as u32;
                if seconds == 0 {
                    self.drop_task(&id, "execution lease expired")?;
                    return Ok(vec![]);
                }
                if command.kind == "start" && !task.started {
                    task.started = true;
                    {
                        let assignment = &mut task.assignment;
                        if !assignment.managed_signing_json.is_empty() {
                            let mut signing: Value =
                                serde_json::from_str(&assignment.managed_signing_json)?;
                            signing["session_id"] = json!(command.signing_session_id);
                            assignment.managed_signing_json = signing.to_string();
                        }
                    }
                    Ok(vec![Action::Start(id, seconds)])
                } else {
                    Ok(vec![Action::Control(
                        id,
                        Box::new(Input {
                            kind: "renew".into(),
                            lease_seconds: seconds,
                            signing_session_id: command.signing_session_id.clone(),
                            ..Default::default()
                        }),
                    )])
                }
            }
            "cancel" => {
                if command.message == "lease_invalid" {
                    self.drop_task(&id, "execution lease expired or revoked")?;
                    Ok(vec![])
                } else {
                    Ok(vec![Action::Control(
                        id,
                        Box::new(Input {
                            kind: "cancel".into(),
                            reason: "cancel requested".into(),
                            ..Default::default()
                        }),
                    )])
                }
            }
            _ => Ok(vec![]),
        }
    }
    pub(super) fn reset_ota_session(&mut self, session: &str) -> bool {
        if let Some(directory) = &self.ota_directory
            && ota::confirm(directory, session, "").is_err()
        {
            // 保存会话围栏前等待；不能带旧健康确认进入新连接。
            return false;
        }
        self.ota_session = session.into();
        self.health_at = None;
        true
    }
    pub(super) fn retry_pending_ota(&mut self, now: Instant) {
        if self.ota_cached.maintenance || !self.ota_cached.fresh() {
            return;
        }
        let Some(directory) = &self.ota_directory else {
            return;
        };
        let Some((request, retry_at)) = &mut self.pending_ota else {
            return;
        };
        if now < *retry_at {
            return;
        }
        if ota::submit(directory, request).is_err() {
            // 仅保留一个最新请求；锁或磁盘故障不终止当前runtime。
            *retry_at = now + Duration::from_secs(1);
            return;
        }
        self.pending_ota = None;
        self.ota_cached = Default::default();
    }
    pub(super) fn drop_task(&mut self, id: &str, reason: &str) -> Result<()> {
        if let Some(mut task) = self.tasks.remove(id)
            && let Some(control) = task.control.take()
        {
            let _ = control.try_send(Input {
                kind: "cancel".into(),
                reason: reason.into(),
                ..Default::default()
            });
        }
        atomic_json(
            &self.cfg.root.join("lost").join(format!("{id}.json")),
            &json!({"attempt_id":id,"reason":reason,"time":chrono::Utc::now()}),
        )
    }
    pub(super) fn event(&mut self, id: &str, mut event: Event) -> Result<()> {
        let Some(task) = self.tasks.get_mut(id) else {
            return Ok(());
        };
        if task.finished_at.is_some() {
            return Ok(());
        }
        if event.kind == "log" {
            if self
                .spool
                .append(id, &mut task.log_sequence, &event.step, &event.data)
                .is_err()
            {
                task.buffer_error = true;
                if let Some(control) = &task.control {
                    let _ = control.try_send(Input {
                        kind: "cancel".into(),
                        reason: "log buffer exhausted or unavailable".into(),
                        ..Default::default()
                    });
                }
            }
            return Ok(());
        }
        if event.kind == "finished" {
            task.finished_at = Some(Instant::now());
            if task.buffer_error {
                event.payload = json!({"status":"failed","error":"log buffer exhausted or unavailable","failure_kind":"emissary"});
            }
        }
        task.sequence += 1;
        task.pending.push(pb::EmissaryEvent {
            emissary_id: self.cfg.emissary_id.clone(),
            kind: event.kind,
            attempt_id: id.into(),
            lease_token: task.token.clone(),
            sequence: task.sequence,
            payload_json: event.payload.to_string(),
            ..Default::default()
        });
        Ok(())
    }
    pub(super) fn pending(&mut self) -> Vec<pb::EmissaryEvent> {
        let mut events = Vec::new();
        let now = Instant::now();
        for (id, task) in &mut self.tasks {
            if !task.started {
                if self.stopping
                    || task.last_accept_at.is_some_and(|at| {
                        now.saturating_duration_since(at) < Duration::from_secs(1)
                    })
                {
                    continue;
                }
                task.last_accept_at = Some(now);
                let request = uuid::Uuid::new_v4().to_string();
                self.anchors.insert(request.clone(), now);
                events.push(pb::EmissaryEvent {
                    emissary_id: self.cfg.emissary_id.clone(),
                    kind: "accept".into(),
                    attempt_id: id.clone(),
                    lease_token: task.token.clone(),
                    command_id: request,
                    ..Default::default()
                });
            } else {
                // Replay unacknowledged events on a timer. Replaying on every
                // acknowledgement amplifies a slow peer's queue and can starve
                // later step/terminal events and heartbeat processing.
                if task.replay_at.is_none_or(|at| now >= at) {
                    task.sent_sequence = 0;
                    task.replay_at = Some(now + Duration::from_secs(5));
                }
                for event in &task.pending {
                    if event.sequence <= task.sent_sequence {
                        continue;
                    }
                    if event.kind == "finished" && !task.logs_abandoned && !self.spool.empty(id) {
                        break;
                    }
                    events.push(event.clone());
                }
            }
        }
        events
    }
    pub(super) fn stop(&mut self) {
        self.stopping = true;
        for task in self.tasks.values_mut() {
            if let Some(control) = task.control.take() {
                let _ = control.try_send(Input {
                    kind: "cancel".into(),
                    reason: "Emissary stopping".into(),
                    ..Default::default()
                });
            }
        }
    }
}
