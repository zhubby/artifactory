//! [INPUT]: 依赖共享进程原语、独立Worker及Darwin真实出生时间。
//! [OUTPUT]: 导出进程入口，按出生时间登记 Worker 和 PTY 会话并回收。
//! [POS]: 节点进程身份边界；源码删除与终端清扫共享出生时间围栏。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
pub use artifactory_execution_support::process::{output, probe, read_log, spawn, wait};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::time::Duration;
use tokio::process::Command;

pub fn spawn_worker(command: Command) -> std::io::Result<Box<dyn ChildWrapper>> {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessSession);
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    command.spawn()
}
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Session {
    pid: u32,
    start_time: u64,
}
impl Session {
    pub fn capture(pid: u32) -> Result<Self> {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let process = system
            .process(sysinfo::Pid::from_u32(pid))
            .context("worker exited before session registration")?;
        #[cfg(unix)]
        ensure!(
            process.session_id() == Some(sysinfo::Pid::from_u32(pid)),
            "worker must own an isolated session"
        );
        Ok(Self {
            pid,
            start_time: process.start_time(),
        })
    }
    /// PTY 库已校验独立 SID；父确认前持有未 wait 的 Child，PID 不能复用。
    #[cfg(unix)]
    pub(crate) async fn capture_terminal(pid: u32) -> Result<Self> {
        if let Ok(session) = Self::capture(pid)
            && session.start_time != 0
        {
            return Ok(session);
        }
        // Darwin 的退出过渡态和 Zombie 不提供完整 SID；未 wait Child
        // 保留 PID，ps 的内核查询仍返回真实出生时间。
        #[cfg(target_os = "macos")]
        if let Some(start_time) = macos_process_start(pid).await? {
            return Ok(Self { pid, start_time });
        }
        anyhow::bail!("terminal process identity unavailable before registration")
    }
    pub async fn terminate(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut system = sysinfo::System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let leader = sysinfo::Pid::from_u32(self.pid);
            if let Some(process) = system.process(leader) {
                let start_time = process.start_time();
                #[cfg(target_os = "macos")]
                let start_time = if start_time == 0 {
                    macos_process_start(self.pid)
                        .await?
                        .context("cannot verify terminal process identity")?
                } else {
                    start_time
                };
                ensure!(
                    start_time == self.start_time,
                    "worker PID was reused; source cleanup requires inspection"
                );
            }
            #[cfg(unix)]
            let alive = system
                .processes()
                .values()
                .filter(|p| {
                    p.session_id() == Some(leader) && p.status() != sysinfo::ProcessStatus::Zombie
                })
                .collect::<Vec<_>>();
            #[cfg(not(unix))]
            let alive = system.process(leader).into_iter().collect::<Vec<_>>();
            if alive.is_empty() {
                return Ok(());
            }
            for process in alive {
                let _ = process.kill();
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "worker descendants remain; source cleanup is blocked"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

#[cfg(target_os = "macos")]
async fn macos_process_start(pid: u32) -> Result<Option<u64>> {
    let mut command = Command::new("/bin/ps");
    command
        .args(["-p", &pid.to_string(), "-o", "stat=", "-o", "lstart="])
        .env_clear()
        .env("LC_ALL", "C")
        .env("TZ", "UTC");
    let (status, bytes) = probe(command, Duration::from_secs(2)).await?;
    if !status.success() {
        return Ok(None);
    }
    Ok(Some(macos_start_time(&bytes)?))
}

#[cfg(target_os = "macos")]
fn macos_start_time(bytes: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(bytes)?.trim();
    let (_status, started) = text
        .split_once(char::is_whitespace)
        .context("invalid terminal process identity")?;
    let started = chrono::NaiveDateTime::parse_from_str(started.trim(), "%a %b %e %T %Y")?;
    let start_time = started.and_utc().timestamp().try_into()?;
    ensure!(
        start_time != 0,
        "terminal process birth time is unavailable"
    );
    Ok(start_time)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::macos_start_time;

    #[test]
    fn darwin_exit_transition_preserves_real_birth_time() {
        // 真实探针的退出过渡态为 ?Es；PID 由未回收 Child 保留。
        let transition = macos_start_time(b"?Es  Wed Nov 25 09:44:05 2026    \n").unwrap();
        let zombie = macos_start_time(b"Zs  Wed Nov 25 09:44:05 2026    \n").unwrap();
        assert_eq!(transition, 1_795_599_845);
        assert_eq!(transition, zombie);
        assert!(macos_start_time(b"Z Thu Jan 1 00:00:00 1970\n").is_err());
        assert!(macos_start_time(b"?Es\n").is_err());
    }
}
