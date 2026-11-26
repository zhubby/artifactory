//! [INPUT]: 实际父启动器能力、持久进度与运行时真实排空计数。
//! [OUTPUT]: 构造有限频率的观察信封，并转交冻结资产续签。
//! [POS]: runtime侧OTA观测与认证RPC适配，不决定激活。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::{
    State, complete_url_refresh, launcher_snapshot, progress_snapshot, url_refresh_request,
};
use crate::{config::Config, protocol};
use anyhow::{Result, ensure};
use artifactory_emissary_protocol::ota::Progress;
pub use artifactory_emissary_protocol::ota::{CAPABILITY, Drain, Observability};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Reporter {
    last: Option<Observability>,
    sent: Option<Instant>,
    changed: Option<chrono::DateTime<chrono::Utc>>,
    update: String,
}
#[derive(Serialize, Deserialize)]
struct Checkpoint {
    update_id: String,
    generation: i64,
    observation: Observability,
}
impl Reporter {
    /// 返回None表示尚未到发送周期；调用者保留既有基础OTA心跳。
    pub fn observe(
        &mut self,
        directory: &Path,
        state: &State,
        drain: Drain,
        now: Instant,
    ) -> Result<Option<Observability>> {
        if self.last.is_none()
            && let Ok(checkpoint) =
                crate::config::read_json::<Checkpoint>(&directory.join("runtime-report.json"))
            && checkpoint.update_id == state.update_id
            && checkpoint.generation == state.generation
            && checkpoint.observation.validate().is_ok()
        {
            self.changed = Some(checkpoint.observation.last_progress_at);
            self.update = checkpoint.update_id;
            self.last = Some(checkpoint.observation);
        }
        let launcher = launcher_snapshot(directory)?;
        let progress = progress_snapshot(directory)?
            .filter(|_| launcher.capabilities.iter().any(|v| v == "ota-progress-v1"))
            .filter(|p| p.update_id == state.update_id && p.generation == state.generation)
            .map(|p| {
                Ok::<_, anyhow::Error>(Progress {
                    update_id: p.update_id.parse()?,
                    generation: p.generation,
                    asset_sha256: p.asset_sha256,
                    sequence: p.sequence,
                    subphase: p.subphase,
                    bytes_done: p.bytes_done,
                    bytes_total: p.bytes_total,
                    download_attempt: p.download_attempt,
                    last_progress_at: p.last_progress_at,
                    updated_at: p.updated_at,
                    error_code: p.error_code,
                })
            })
            .transpose()?;
        let draining =
            matches!(state.phase.as_str(), "draining" | "activating") && state.activation_pending;
        let drain = (state.phase == "draining" || draining).then_some(drain);
        let changed = self.last.as_ref().is_none_or(|old| {
            old.progress != progress
                || old.drain != drain
                || old.launcher_capabilities != launcher.capabilities
        }) || self.update != state.update_id;
        if changed {
            self.changed = Some(chrono::Utc::now());
        }
        let last_progress_at = self.changed.unwrap_or_else(chrono::Utc::now);
        let immediate = self.last.as_ref().is_none_or(|old| {
            old.progress.as_ref().map(|p| (&p.subphase, &p.error_code))
                != progress.as_ref().map(|p| (&p.subphase, &p.error_code))
        }) || self.update != state.update_id;
        let elapsed = self
            .sent
            .map(|sent| now.saturating_duration_since(sent))
            .unwrap_or(Duration::MAX);
        if !immediate
            && elapsed
                < if changed {
                    Duration::from_secs(2)
                } else {
                    Duration::from_secs(30)
                }
        {
            return Ok(None);
        }
        let sequence = if self.update != state.update_id {
            1
        } else {
            self.last.as_ref().map_or(Ok(1), |old| {
                old.sequence
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("OTA observation sequence exhausted"))
            })?
        };
        let value = Observability {
            schema_version: 1,
            sequence,
            launcher_version: if launcher.launcher_version.is_empty() {
                state.launcher_version.clone()
            } else {
                launcher.launcher_version
            },
            launcher_capabilities: launcher.capabilities,
            progress,
            drain,
            last_progress_at,
            stalled: chrono::Utc::now()
                .signed_duration_since(last_progress_at)
                .num_seconds()
                >= 90,
        };
        value.validate()?;
        crate::config::atomic_json(
            &directory.join("runtime-report.json"),
            &Checkpoint {
                update_id: state.update_id.clone(),
                generation: state.generation,
                observation: value.clone(),
            },
        )?;
        self.update = state.update_id.clone();
        self.last = Some(value.clone());
        self.sent = Some(now);
        Ok(Some(value))
    }
}

/// 仅在当前Control能力确认后调用；错误不包含URL或凭据。
pub async fn forward_url_refresh(cfg: &Config, directory: &Path, session: &str) -> Result<bool> {
    let Some(frozen) = url_refresh_request(directory)? else {
        return Ok(false);
    };
    let mut client = crate::client::control(cfg)
        .await
        .map_err(|_| anyhow::anyhow!("OTA renewal connection unavailable"))?;
    let mut request = tonic::Request::new(protocol::EmissaryAssetRenewRequest {
        session_id: session.into(),
        request_id: frozen.request_id.clone(),
        update_id: frozen.update_id.clone(),
        generation: frozen.generation,
        manifest_sha256: frozen.manifest_sha256.clone(),
        asset_sha256: frozen.asset_sha256.clone(),
        asset_size: frozen.asset_size,
        os: frozen.os.clone(),
        arch: frozen.arch.clone(),
    });
    request.set_timeout(Duration::from_secs(5));
    let reply = client
        .renew_emissary_asset(request)
        .await
        .map_err(|_| anyhow::anyhow!("OTA renewal rejected"))?
        .into_inner();
    ensure!(
        reply.request_id == frozen.request_id
            && reply.update_id == frozen.update_id
            && reply.generation == frozen.generation,
        "OTA renewal reply mismatch"
    );
    complete_url_refresh(directory, frozen, Some(reply.url))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_launcher_reports_unknown_progress_and_durable_monotonic_sequence() {
        let root = tempfile::tempdir().unwrap();
        let state = State {
            update_id: uuid::Uuid::new_v4().to_string(),
            generation: 5,
            phase: "draining".into(),
            launcher_version: "0.2.3".into(),
            ..Default::default()
        };
        let now = Instant::now();
        let mut reporter = Reporter::default();
        let first = reporter
            .observe(root.path(), &state, Drain::default(), now)
            .unwrap()
            .unwrap();
        assert!(first.launcher_capabilities.is_empty());
        assert!(first.progress.is_none());
        assert_eq!(first.launcher_version, "0.2.3");
        assert!(
            reporter
                .observe(
                    root.path(),
                    &state,
                    Drain::default(),
                    now + Duration::from_secs(1)
                )
                .unwrap()
                .is_none()
        );
        let mut restarted = Reporter::default();
        let next = restarted
            .observe(
                root.path(),
                &state,
                Drain::default(),
                now + Duration::from_secs(2),
            )
            .unwrap()
            .unwrap();
        assert!(next.sequence > first.sequence);
        assert_eq!(next.last_progress_at, first.last_progress_at);
    }
}
