//! [INPUT]: 依赖受认证的控制命令、设备启动身份和独立终端传输。
//! [OUTPUT]: 监督有界终端会话并在控制替换、OTA 与停机时取消。
//! [POS]: Runner 与设备终端流之间的准入与生命周期边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{config::Config, protocol::Command};
use anyhow::{Result, ensure};
use artifactory_emissary_protocol::terminal::{MAX_CONTROL_BYTES, OpenTerminal};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Active {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

pub(crate) struct Supervisor {
    cfg: Config,
    startup: Uuid,
    session: Option<Uuid>,
    account: String,
    active: HashMap<Uuid, Active>,
    seen: HashMap<Uuid, Instant>,
    draining: bool,
}

impl Supervisor {
    pub fn new(cfg: Config, startup: Uuid, account: String) -> Self {
        Self {
            cfg,
            startup,
            session: None,
            account,
            active: HashMap::new(),
            seen: HashMap::new(),
            draining: false,
        }
    }

    pub fn control_session(&mut self, session: Uuid) {
        if self.session != Some(session) {
            self.disconnect();
        }
        self.session = Some(session);
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    pub fn disconnect(&mut self) {
        self.session = None;
        for entry in self.active.values() {
            entry.cancel.cancel();
        }
    }

    pub fn drain(&mut self) {
        self.set_draining(true);
    }

    pub fn set_draining(&mut self, draining: bool) {
        self.draining = draining;
        if draining {
            for entry in self.active.values() {
                entry.cancel.cancel();
            }
        }
    }

    pub fn tick(&mut self) {
        self.active.retain(|_, entry| !entry.task.is_finished());
        self.seen.retain(|_, until| *until > Instant::now());
    }

    pub fn command(&mut self, command: &Command) -> Result<()> {
        ensure!(
            command.payload_json.len() <= MAX_CONTROL_BYTES,
            "oversized terminal command"
        );
        if command.kind == "terminal_close" {
            let value: serde_json::Value = serde_json::from_str(&command.payload_json)?;
            if let Some(id) = value
                .get("terminal_id")
                .and_then(|v| v.as_str())
                .and_then(|s| Uuid::parse_str(s).ok())
            {
                self.seen
                    .insert(id, Instant::now() + Duration::from_secs(60));
                if let Some(active) = self.active.get(&id) {
                    active.cancel.cancel();
                }
            }
            return Ok(());
        }
        ensure!(command.kind == "terminal_open", "unknown terminal command");
        let open: OpenTerminal = serde_json::from_str(&command.payload_json)?;
        open.validate(chrono::Utc::now().timestamp_millis())?;
        ensure!(
            self.cfg.terminal.enabled && !self.draining,
            "device terminal is disabled or draining"
        );
        ensure!(
            Some(open.node_session) == self.session && open.startup_id == self.startup,
            "stale terminal control identity"
        );
        if self.seen.contains_key(&open.terminal_id) || self.active.contains_key(&open.terminal_id)
        {
            return Ok(());
        }
        self.tick();
        ensure!(
            self.active.len() < self.cfg.terminal.max_sessions,
            "device terminal capacity exhausted"
        );
        ensure!(
            self.cfg.terminal.profiles.contains_key(&open.profile),
            "terminal profile is unavailable"
        );
        let id = open.terminal_id;
        let cfg = self.cfg.clone();
        let account = self.account.clone();
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = super::transport::run_session(cfg, open, account, stop).await {
                // 只记录分类结果；不得把控制描述或帧内容写入日志。
                eprintln!("device terminal ended: {error}");
            }
        });
        self.active.insert(id, Active { cancel, task });
        self.seen
            .insert(id, Instant::now() + Duration::from_secs(60));
        Ok(())
    }

    pub async fn shutdown(&mut self) {
        self.drain();
        let active = std::mem::take(&mut self.active);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        for (_, mut entry) in active {
            if tokio::time::timeout_at(deadline, &mut entry.task)
                .await
                .is_err()
            {
                entry.task.abort();
                let _ = entry.task.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ota_resume_keeps_control_identity_and_never_revives_closed_terminal() {
        let session = Uuid::new_v4();
        let startup = Uuid::new_v4();
        let mut cfg = Config::default();
        cfg.terminal.enabled = true;
        let mut supervisor = Supervisor::new(cfg, startup, "emissary".into());
        supervisor.control_session(session);
        let open = OpenTerminal {
            terminal_id: Uuid::new_v4(),
            node_session: session,
            startup_id: startup,
            expires_at_ms: chrono::Utc::now().timestamp_millis() + 15_000,
            columns: 80,
            rows: 24,
            term: "xterm-256color".into(),
            profile: "default".into(),
        };
        let command = |open: &OpenTerminal| Command {
            kind: "terminal_open".into(),
            payload_json: serde_json::to_string(open).unwrap(),
            ..Default::default()
        };
        supervisor.command(&command(&open)).unwrap();
        supervisor.set_draining(true);
        assert!(supervisor.active[&open.terminal_id].cancel.is_cancelled());
        let next = OpenTerminal {
            terminal_id: Uuid::new_v4(),
            ..open.clone()
        };
        assert!(supervisor.command(&command(&next)).is_err());
        supervisor.set_draining(false);
        supervisor.command(&command(&open)).unwrap();
        assert!(supervisor.active[&open.terminal_id].cancel.is_cancelled());
        supervisor.command(&command(&next)).unwrap();
        assert!(!supervisor.active[&next.terminal_id].cancel.is_cancelled());
        supervisor.shutdown().await;
    }
}
