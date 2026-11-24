//! [INPUT]: 依赖共享进程原语、独立 Worker 与未回收 PTY 进程身份。
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
        // Darwin 的 Zombie 不再提供 getsid/PROC_PIDTBSDINFO。ps 的内核查询
        // 仍返回真实出生时间，禁止将 sysinfo 的缺省零值写入身份记录。
        #[cfg(target_os = "macos")]
        if let Some(start_time) = macos_zombie_start(pid).await? {
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
                    macos_zombie_start(self.pid)
                        .await?
                        .context("cannot verify terminal zombie identity")?
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
async fn macos_zombie_start(pid: u32) -> Result<Option<u64>> {
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
    let text = std::str::from_utf8(&bytes)?.trim();
    let (status, started) = text
        .split_once(char::is_whitespace)
        .context("invalid terminal process identity")?;
    if !status.starts_with('Z') {
        return Ok(None);
    }
    let started = chrono::NaiveDateTime::parse_from_str(started.trim(), "%a %b %e %T %Y")?;
    Ok(Some(started.and_utc().timestamp().try_into()?))
}
