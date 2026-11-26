//! [INPUT]: 依赖共享有限操作契约、认证身份与本地耐久日志。
//! [OUTPUT]: 提供操作准入、执行、取消、结果确认与有界清理等待。
//! [POS]: 独立于构建租约的只读操作边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod executor;
mod journal;
#[cfg(test)]
mod tests;
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::operations::*;
use futures::future::BoxFuture;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type InventorySink = Arc<
    dyn Fn(
            artifactory_emissary_protocol::telemetry::Inventory,
        ) -> BoxFuture<'static, Result<InventoryRefresh>>
        + Send
        + Sync,
>;
#[derive(Clone, Debug)]
pub struct Identity {
    pub tenant_id: Uuid,
    pub node_id: Uuid,
    pub startup_id: Uuid,
    pub control_session: Uuid,
}
pub struct ServerClock {
    upper_at_receive_ms: i64,
    received: Instant,
}
impl ServerClock {
    pub fn new(server_time_ms: i64, started: Instant, received: Instant) -> Result<Self> {
        let rtt = received
            .checked_duration_since(started)
            .context("invalid handshake clock")?;
        ensure!(
            server_time_ms > 0 && rtt <= Duration::from_secs(10),
            "handshake clock uncertainty exceeds budget"
        );
        Ok(Self {
            upper_at_receive_ms: server_time_ms
                .checked_add(i64::try_from(rtt.as_millis())?)
                .context("invalid server clock")?,
            received,
        })
    }
    fn valid(&self, envelope: &OperationEnvelope) -> bool {
        let elapsed = self.received.elapsed().as_millis();
        let Ok(elapsed) = i64::try_from(elapsed) else {
            return false;
        };
        self.upper_at_receive_ms
            .checked_add(elapsed)
            .is_some_and(|now| {
                envelope.expires_at_ms > now
                    && envelope.issued_at_ms <= now
                    && envelope.expires_at_ms - now <= RECEIVE_WINDOW_MS
            })
    }
}
struct Inner {
    journal: journal::Journal,
    active: Option<(Uuid, CancellationToken)>,
    accepted_deadline: Option<(Uuid, tokio::time::Instant)>,
    draining: bool,
    cleanup_failed: bool,
    shutting_down: bool,
}
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Mutex<Inner>>,
    startup_id: Uuid,
    worker: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}
impl Runtime {
    pub fn open(root: PathBuf, startup_id: Uuid) -> Result<Self> {
        ensure!(!startup_id.is_nil(), "operation startup missing");
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                journal: journal::Journal::open(root)?,
                active: None,
                accepted_deadline: None,
                draining: false,
                cleanup_failed: false,
                shutting_down: false,
            })),
            startup_id,
            worker: Default::default(),
        })
    }
    pub async fn accept(
        &self,
        envelope: OperationEnvelope,
        identity: &Identity,
        clock: Option<&ServerClock>,
    ) -> std::result::Result<OperationUpdate, OperationReason> {
        envelope
            .validate()
            .map_err(|_| OperationReason::InvalidInput)?;
        if self.startup_id != identity.startup_id
            || envelope.tenant_id != identity.tenant_id
            || envelope.node_id != identity.node_id
            || envelope.startup_id != identity.startup_id
            || envelope.control_session != identity.control_session
        {
            return Err(OperationReason::IdentityMismatch);
        }
        let mut inner = self.inner.lock().await;
        // 已接受投递只重放状态；新会话须由 Control 再授权信封。
        if inner.journal.get(envelope.operation_id).is_some() {
            return inner.journal.accept(envelope);
        }
        if inner.draining {
            return Err(OperationReason::Busy);
        }
        let clock = clock.ok_or(OperationReason::ClockUnsynchronized)?;
        if !clock.valid(&envelope) {
            return Err(OperationReason::ReceiveExpired);
        }
        let id = envelope.operation_id;
        let update = inner.journal.accept(envelope)?;
        inner.accepted_deadline = Some((
            id,
            tokio::time::Instant::now() + Duration::from_secs(EXECUTION_SECONDS),
        ));
        Ok(update)
    }
    /// 接受状态发出后调用；日志失败时不启动探测。
    pub async fn start(
        &self,
        id: Uuid,
        cfg: crate::config::Config,
        sink: Option<InventorySink>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().await;
        ensure!(
            !inner.draining && inner.active.is_none(),
            "operations are draining or busy"
        );
        let envelope = inner
            .journal
            .get(id)
            .context("operation missing")?
            .envelope
            .clone();
        let deadline = inner
            .accepted_deadline
            .filter(|(current, _)| *current == id)
            .context("accepted execution deadline missing")?
            .1;
        if deadline <= tokio::time::Instant::now() {
            let result = OperationResult {
                skipped: envelope
                    .validate()?
                    .probes()
                    .into_iter()
                    .map(|id| (id, SkippedReason::BudgetExhausted))
                    .collect(),
                ..Default::default()
            };
            inner.journal.finish(
                id,
                OperationState::Failed,
                Some(OperationReason::ExecutionBudgetExhausted),
                result,
            )?;
            anyhow::bail!("operation execution deadline expired");
        }
        inner.journal.running(id)?;
        let token = CancellationToken::new();
        inner.active = Some((id, token.clone()));
        let runtime = self.clone();
        let worker = tokio::spawn(async move {
            let (state, reason, result) =
                executor::execute(&cfg, &envelope, token.clone(), sink, deadline).await;
            let mut inner = runtime.inner.lock().await;
            let cleanup_failed = result
                .probes
                .values()
                .any(|report| report.error_code == Some(ProbeErrorCode::CleanupFailed));
            if cleanup_failed {
                inner.draining = true;
                inner.cleanup_failed = true;
            }
            // 写盘暂时失败时保留操作锁与结果，重试持久化；不得重新探测。
            loop {
                let canceled = inner
                    .active
                    .as_ref()
                    .is_some_and(|(current, cancel)| *current == id && cancel.is_cancelled());
                let (state, reason) = if cleanup_failed {
                    (OperationState::Failed, Some(OperationReason::CleanupFailed))
                } else if canceled {
                    (OperationState::Canceled, Some(OperationReason::Canceled))
                } else {
                    (state, reason)
                };
                if inner
                    .journal
                    .finish(id, state, reason, result.clone())
                    .is_ok()
                {
                    inner.active = None;
                    break;
                }
                drop(inner);
                tokio::time::sleep(Duration::from_secs(1)).await;
                inner = runtime.inner.lock().await;
            }
        });
        *self.worker.lock().await = Some(worker);
        Ok(())
    }
    pub async fn reauthorize(
        &self,
        resume: &OperationResume,
        identity: &Identity,
    ) -> Result<OperationUpdate> {
        resume.validate()?;
        ensure!(
            resume.node_id == identity.node_id
                && resume.tenant_id == identity.tenant_id
                && resume.current_startup_id == identity.startup_id
                && resume.control_session == identity.control_session
                && identity.startup_id == self.startup_id,
            "operation resume authority mismatch"
        );
        self.inner.lock().await.journal.reauthorize(resume)
    }
    pub async fn status(&self, id: Uuid) -> Option<OperationUpdate> {
        self.inner
            .lock()
            .await
            .journal
            .get(id)
            .map(|entry| entry.update.clone())
    }
    pub async fn updates(&self) -> Vec<OperationUpdate> {
        self.inner
            .lock()
            .await
            .journal
            .entries()
            .filter(|e| !e.acknowledged)
            .map(|e| e.update.clone())
            .collect()
    }
    pub async fn ack(&self, ack: &OperationAck) -> Result<bool> {
        self.inner.lock().await.journal.ack(ack)
    }
    pub async fn active(&self) -> bool {
        let inner = self.inner.lock().await;
        inner.cleanup_failed || inner.journal.active()
    }
    pub async fn cancel(
        &self,
        cancel: &OperationCancel,
        identity: &Identity,
    ) -> Result<OperationUpdate> {
        ensure!(
            cancel.startup_id == identity.startup_id
                && cancel.control_session == identity.control_session
                && identity.startup_id == self.startup_id,
            "operation cancel identity mismatch"
        );
        let mut inner = self.inner.lock().await;
        let entry = inner
            .journal
            .get(cancel.operation_id)
            .context("operation missing")?;
        ensure!(
            entry.envelope.tenant_id == identity.tenant_id
                && entry.envelope.node_id == identity.node_id
                && entry.envelope.startup_id == identity.startup_id
                && entry.envelope.control_session == identity.control_session,
            "operation cancel authority mismatch"
        );
        if entry.update.state.terminal() {
            return Ok(entry.update.clone());
        }
        if let Some((id, token)) = &inner.active
            && *id == cancel.operation_id
        {
            token.cancel();
            return Ok(inner.journal.get(*id).unwrap().update.clone());
        }
        inner.journal.finish(
            cancel.operation_id,
            OperationState::Canceled,
            Some(OperationReason::Canceled),
            OperationResult::default(),
        )
    }
    pub async fn drain(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.draining = true;
        if let Some((_, token)) = &inner.active {
            token.cancel();
        }
        let pending: Vec<_> = inner
            .journal
            .entries()
            .filter(|e| e.update.state == OperationState::Accepted)
            .map(|e| e.envelope.operation_id)
            .collect();
        for id in pending {
            inner.journal.finish(
                id,
                OperationState::Canceled,
                Some(OperationReason::Canceled),
                OperationResult::default(),
            )?;
        }
        Ok(())
    }
    pub async fn resume(&self) {
        let mut inner = self.inner.lock().await;
        if !inner.cleanup_failed && !inner.shutting_down {
            inner.draining = false;
        }
    }
    /// 先围栏再取消；返回成功前等待子进程清理与终态落盘。
    pub async fn shutdown(&self, budget: Duration) -> Result<()> {
        let wait = async {
            {
                let mut inner = self.inner.lock().await;
                inner.shutting_down = true;
                inner.draining = true;
            }
            self.drain().await?;
            let mut worker = self.worker.lock().await.take();
            if let Some(task) = &mut worker {
                match tokio::time::timeout(budget, task).await {
                    Ok(result) => result.context("operation cleanup task failed")?,
                    Err(_) => {
                        *self.worker.lock().await = worker;
                        anyhow::bail!(
                            "operation cleanup or terminal persistence deadline exceeded"
                        );
                    }
                }
            }
            let inner = self.inner.lock().await;
            ensure!(!inner.cleanup_failed, "operation child cleanup failed");
            ensure!(
                inner.active.is_none() && !inner.journal.active(),
                "operation terminal is not durable"
            );
            Ok(())
        };
        tokio::time::timeout(budget + Duration::from_secs(1), wait)
            .await
            .context("operation shutdown could not acquire its journal")?
    }
    pub async fn maintenance(&self) -> Result<u64> {
        self.inner.lock().await.journal.maintenance()
    }
}
