//! [INPUT]: 依赖共享进程原语、独立 Worker 会话和进程身份。
//! [OUTPUT]: 导出通用进程入口，提供节点会话登记与回收。
//! [POS]: 以进程身份验证源码删除前的清理边界。
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
    pub async fn terminate(&self) -> Result<()> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let mut system = sysinfo::System::new();
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            let leader = sysinfo::Pid::from_u32(self.pid);
            if let Some(process) = system.process(leader) {
                ensure!(
                    process.start_time() == self.start_time,
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
