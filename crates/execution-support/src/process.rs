//! [INPUT]: 依赖受控进程组、Unix进程快照、取消令牌与输出流。
//! [OUTPUT]: 提供进程组回收、输出读取与失败读取任务的截止。
//! [POS]: 执行器共用的进程边界，不读取节点配置。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::events::{Emitter, Event};
use anyhow::{Result, bail};
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

pub async fn wait(
    child: &mut dyn ChildWrapper,
    cancel: &CancellationToken,
    timeout: Duration,
) -> Result<ExitStatus> {
    #[cfg(unix)]
    let reason = tokio::select! {
        result = observe_exit(child) => { result?; None },
        _ = cancel.cancelled() => Some("execution canceled"),
        _ = tokio::time::sleep(timeout) => Some("execution timed out"),
    };
    #[cfg(not(unix))]
    let reason = tokio::select! {
        result = child.wait() => return Ok(result?),
        _ = cancel.cancelled() => Some("execution canceled"),
        _ = tokio::time::sleep(timeout) => Some("execution timed out"),
    };
    if let Some(reason) = reason {
        terminate(child).await?;
        bail!(reason);
    }
    Ok(child.wait().await?)
}

#[cfg(unix)]
async fn observe_exit(child: &dyn ChildWrapper) -> Result<()> {
    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("process leader is unavailable"))?;
    loop {
        if exited_without_reaping(pid)? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
fn exited_without_reaping(pid: u32) -> Result<bool> {
    // SAFETY: siginfo_t零初始化有效；waitid写入独占本地内存，WNOWAIT保留子进程身份。
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: PID来自受借用Child，WNOWAIT不回收该PID。
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error.into());
    }
    // SAFETY: waitid成功后si_pid包含内核写入的已退出子进程ID或零。
    Ok(unsafe { info.si_pid() } != 0)
}

async fn terminate(child: &mut dyn ChildWrapper) -> Result<()> {
    #[cfg(unix)]
    let group = {
        // 未wait的leader固定PID/PGID；身份失效时禁止发送组信号。
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("process group leader was already reaped"))?;
        let group = libc::pid_t::try_from(pid)?;
        anyhow::ensure!(group > 1, "invalid process group leader");
        group
    };
    let result = async {
        #[cfg(unix)]
        {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let mut empty = false;
            loop {
                if let Err(error) = child.start_kill()
                    && error.raw_os_error() != Some(libc::ESRCH)
                {
                    return Err(error.into());
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
                let snapshot = tokio::task::spawn_blocking(move || group_has_live_members(group));
                let alive = tokio::time::timeout_at(deadline, snapshot)
                    .await
                    .map_err(|_| {
                        anyhow::anyhow!("process snapshot exceeded cleanup deadline")
                    })???;
                if !alive && empty {
                    break;
                }
                empty = !alive;
                anyhow::ensure!(
                    tokio::time::Instant::now() < deadline,
                    "process group descendants remain after cancellation"
                );
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        let _ = child.start_kill();
    }
    #[cfg(not(unix))]
    let _ = child.start_kill();
    let reaped = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    result?;
    reaped.map_err(|_| anyhow::anyhow!("process leader did not exit after termination"))??;
    Ok(())
}

#[cfg(unix)]
fn group_has_live_members(group: libc::pid_t) -> Result<bool> {
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    anyhow::ensure!(
        system
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .is_some(),
        "process snapshot is unavailable"
    );
    Ok(system.processes().values().any(|process| {
        if process.status() == sysinfo::ProcessStatus::Zombie {
            return false;
        }
        let Ok(pid) = libc::pid_t::try_from(process.pid().as_u32()) else {
            return false;
        };
        // SAFETY: getpgid仅读内核进程表；group来自尚未wait的Child。
        unsafe { libc::getpgid(pid) == group }
    }))
}

/// 执行失败后读取任务必须有界；正常退出仍完整读取输出。
pub async fn finish_reader<T: Send + 'static>(
    mut task: tokio::task::JoinHandle<T>,
    failed: bool,
) -> Result<T> {
    if !failed {
        return Ok(task.await?);
    }
    match tokio::time::timeout(Duration::from_secs(2), &mut task).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            task.abort();
            let _ = task.await;
            bail!("process output remained open after execution failed");
        }
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
    let bytes = finish_reader(reader, result.is_err()).await??;
    Ok((result?, bytes))
}

pub async fn read_log(mut reader: impl AsyncRead + Unpin, step: &str, emit: Emitter) -> Result<()> {
    let mut buffer = vec![0; 32 * 1024];
    loop {
        let size = reader.read(&mut buffer).await?;
        if size == 0 {
            return Ok(());
        }
        emit.send(Event {
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
    let failed = status.is_err();
    let out_result = finish_reader(out, failed).await;
    let err_result = finish_reader(err, failed).await;
    let mut bytes = out_result??;
    bytes.extend(err_result??);
    Ok((status?, bytes))
}
