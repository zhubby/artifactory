//! [INPUT]: 依赖受控进程组、独立 Worker 会话和取消令牌。
//! [OUTPUT]: 提供进程树启动、会话回收和有界日志读取。
//! [POS]: 以进程身份验证源码删除前的清理边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, bail, ensure};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

pub fn spawn(command: Command) -> std::io::Result<Box<dyn ChildWrapper>> {
    let mut command = CommandWrap::from(command);
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(process_wrap::tokio::JobObject);
    command.spawn()
}
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
pub async fn wait(
    child: &mut dyn ChildWrapper,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<ExitStatus> {
    tokio::select! {
        result = child.wait() => Ok(result?),
        _ = cancel.cancelled() => { let _ = child.start_kill(); let _ = child.wait().await; bail!("execution canceled"); },
        _ = tokio::time::sleep(timeout) => { let _ = child.start_kill(); let _ = child.wait().await; bail!("execution timed out"); }
    }
}
pub async fn output(
    mut command: Command,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = spawn(command)?;
    let stdout = child.stdout().take();
    let reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(stdout) = stdout {
            stdout.take(4 << 20).read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    });
    let result = wait(&mut *child, cancel, timeout).await;
    let bytes = reader.await??;
    Ok((result?, bytes))
}
pub async fn read_log(
    mut reader: impl AsyncRead + Unpin,
    step: &str,
    emit: crate::worker::Emitter,
) -> Result<()> {
    let mut buffer = vec![0; 32 * 1024];
    loop {
        let size = reader.read(&mut buffer).await?;
        if size == 0 {
            return Ok(());
        }
        emit.send(crate::worker::Event {
            kind: "log".into(),
            step: step.into(),
            data: buffer[..size].to_vec(),
            ..Default::default()
        })
        .await?;
    }
}
pub async fn probe(mut command: Command, timeout: Duration) -> Result<(ExitStatus, Vec<u8>)> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn(command)?;
    let stdout = child.stdout().take();
    let stderr = child.stderr().take();
    let read = |pipe: Option<tokio::process::ChildStdout>| async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            pipe.take(65536).read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    };
    let out = tokio::spawn(read(stdout));
    let err = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr {
            pipe.take(65536).read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    });
    let status = wait(&mut *child, &CancellationToken::new(), timeout).await;
    let mut bytes = out.await??;
    bytes.extend(err.await??);
    Ok((status?, bytes))
}
