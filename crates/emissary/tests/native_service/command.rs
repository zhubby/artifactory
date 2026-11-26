//! [INPUT]: 测试命令、执行截止和共享进程组/Job回收原语。
//! [OUTPUT]: 隔离父进程数据库凭据，提供有界等待、超额拒收和完整输出，读回异步退出。
//! [POS]: 原生服务夹具的同步命令边界，不启动服务或提权。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_execution_support::process;
use process_wrap::tokio::ChildWrapper;
use std::{
    io,
    process::{Command, Output, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const RETAIN: usize = 1 << 20;
const CLEANUP: Duration = Duration::from_secs(8);
const WORKER_MARGIN: Duration = Duration::from_secs(10);

pub(super) fn run(command: Command, budget: Duration) -> io::Result<Output> {
    run_with_limit(command, budget, RETAIN)
}

pub(super) fn run_with_limit(
    mut command: Command,
    budget: Duration,
    limit: usize,
) -> io::Result<Output> {
    for name in [
        "DATABASE_URL",
        "ARTIFACTORY_TEST_DATABASE_URL",
        "ARTIFACTORY_BUSINESS_DATABASE_URL",
        "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL",
        "ARTIFACTORY_PLANNING_DATABASE_URL",
    ] {
        command.env_remove(name);
    }
    if limit > (129 << 20) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture limit exceeds fixture budget",
        ));
    }
    let label = std::path::Path::new(command.get_program())
        .file_name()
        .unwrap_or(command.get_program())
        .to_string_lossy()
        .into_owned();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("native-test-command".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runtime.block_on(execute(command, budget, token, limit))
                }))
                .unwrap_or_else(|_| {
                    Err(io::Error::other(
                        "command worker panicked; cleanup unconfirmed",
                    ))
                });
                // 共享回收器使用spawn_blocking采样；关闭runtime也不能无界等它。
                runtime.shutdown_timeout(Duration::from_millis(500));
                result
            }))
            .unwrap_or_else(|_| {
                Err(io::Error::other(
                    "command worker panicked; cleanup unconfirmed",
                ))
            });
            let _ = send.send(result);
        })?;
    let result = receive.recv_timeout(budget + WORKER_MARGIN);
    if result.is_err() {
        cancel.cancel();
    }
    // 只有线程已结束才join；权限失败或系统调用停滞不能卡住调用者。
    let until = Instant::now() + Duration::from_millis(100);
    while !worker.is_finished() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
    if worker.is_finished() {
        let _ = worker.join();
    }
    result
        .unwrap_or_else(|_| {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "command worker deadline exceeded; cleanup unconfirmed",
            ))
        })
        .map_err(|error| io::Error::new(error.kind(), format!("{label}: {error}")))
}

struct Tree {
    child: Box<dyn ChildWrapper>,
    armed: bool,
}
impl Drop for Tree {
    fn drop(&mut self) {
        if self.armed && self.child.id().is_some() {
            let _ = self.child.start_kill();
        }
    }
}
struct Readers {
    out: JoinHandle<io::Result<(Vec<u8>, bool)>>,
    err: JoinHandle<io::Result<(Vec<u8>, bool)>>,
}
impl Drop for Readers {
    fn drop(&mut self) {
        self.out.abort();
        self.err.abort();
    }
}

async fn drain(mut pipe: impl AsyncRead + Unpin, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut kept = Vec::new();
    let mut overflow = false;
    let mut buffer = [0; 32 * 1024];
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            return Ok((kept, overflow));
        }
        let retained = count.min(limit - kept.len());
        overflow |= count > retained;
        kept.extend_from_slice(&buffer[..retained]);
    }
}

async fn execute(
    command: Command,
    budget: Duration,
    cancel: CancellationToken,
    limit: usize,
) -> io::Result<Output> {
    let mut command = tokio::process::Command::from(command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut tree = Tree {
        child: process::spawn(command)?,
        armed: true,
    };
    let out = tree
        .child
        .stdout()
        .take()
        .ok_or_else(|| io::Error::other("stdout unavailable"))?;
    let err = tree
        .child
        .stderr()
        .take()
        .ok_or_else(|| io::Error::other("stderr unavailable"))?;
    let mut readers = Readers {
        out: tokio::spawn(drain(out, limit)),
        err: tokio::spawn(drain(err, RETAIN)),
    };
    let started = tokio::time::Instant::now();
    let finish = async {
        // 管道先结束，leader仍未wait；超时回收保留组身份，包含父退出后持管道后代。
        let (out, err) = tokio::join!(&mut readers.out, &mut readers.err);
        let (stdout, out_overflow) = out.map_err(|_| io::Error::other("stdout task failed"))??;
        let (stderr, err_overflow) = err.map_err(|_| io::Error::other("stderr task failed"))??;
        let status = process::wait(
            &mut *tree.child,
            &cancel,
            budget.saturating_sub(started.elapsed()),
        )
        .await
        .map_err(|_| io::Error::other("command wait failed"))?;
        Ok::<_, io::Error>((
            Output {
                status,
                stdout,
                stderr,
            },
            out_overflow || err_overflow,
        ))
    };
    let result = tokio::select! {
        result = tokio::time::timeout(budget, finish) => result.map_err(|_| ()),
        _ = cancel.cancelled() => Err(()),
    };
    if let Ok(Ok((output, overflow))) = result {
        tree.armed = false;
        if overflow {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "command output exceeded capture limit",
            ));
        }
        return Ok(output);
    }
    // 先关闭读取任务；即使sudo后代不可发信号，管道也不阻止本地返回。
    drop(readers);
    cancel.cancel();
    // 即使leader已退出，先向仍固定身份的组/Job请求终止。
    let kill_requested = tree.child.id().is_some() && tree.child.start_kill().is_ok();
    tree.armed = false;
    let cleanup = tokio::time::timeout(
        CLEANUP,
        process::wait(&mut *tree.child, &cancel, Duration::ZERO),
    )
    .await;
    let confirmed = kill_requested
        && matches!(&cleanup, Ok(Err(error)) if error.to_string() == "execution canceled" || error.to_string() == "execution timed out");
    let cleanup = if confirmed {
        "confirmed"
    } else {
        "unconfirmed"
    };
    let timed_out = result.is_err();
    Err(io::Error::new(
        if timed_out {
            io::ErrorKind::TimedOut
        } else {
            io::ErrorKind::Other
        },
        format!(
            "command {} ; cleanup {cleanup}",
            if timed_out { "timed out" } else { "failed" }
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, process::Stdio, time::Instant};

    const MODE: &str = "ARTIFACTORY_NATIVE_COMMAND_CHILD";

    fn child(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "command::tests::child_fixture", "--nocapture"])
            .env(MODE, mode);
        command
    }

    #[test]
    fn child_fixture() {
        let Ok(mode) = std::env::var(MODE) else {
            return;
        };
        match mode.as_str() {
            "hang" | "holder" => {
                if let Some(path) = std::env::var_os("ARTIFACTORY_NATIVE_COMMAND_PID") {
                    std::fs::write(path, std::process::id().to_string()).unwrap();
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            "descendant" => {
                // 此夹具刻意先退出父进程，让后代单独持有继承管道。
                #[allow(clippy::zombie_processes)]
                let _descendant = child("holder").stdin(Stdio::null()).spawn().unwrap();
            }
            "streams" => {
                let stdout = std::thread::spawn(|| {
                    for _ in 0..256 {
                        std::io::stdout().write_all(&[b'o'; 8192]).unwrap();
                    }
                });
                for _ in 0..256 {
                    std::io::stderr().write_all(&[b'e'; 8192]).unwrap();
                }
                stdout.join().unwrap();
            }
            "exit" => {
                print!("stdout-value");
                eprint!("stderr-value");
                std::process::exit(7);
            }
            "stdin" => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::io::stdin().read_to_end(&mut bytes).unwrap();
                assert!(bytes.is_empty());
                print!("eof");
            }
            "database_env" => {
                for name in [
                    "DATABASE_URL",
                    "ARTIFACTORY_TEST_DATABASE_URL",
                    "ARTIFACTORY_BUSINESS_DATABASE_URL",
                    "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL",
                    "ARTIFACTORY_PLANNING_DATABASE_URL",
                ] {
                    assert!(
                        std::env::var_os(name).is_none(),
                        "database credential reached child process"
                    );
                }
            }
            _ => panic!("unknown child mode"),
        }
    }

    #[test]
    fn hanging_command_has_a_process_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("leader.pid");
        let started = Instant::now();
        let mut command = child("hang");
        command.env("ARTIFACTORY_NATIVE_COMMAND_PID", &marker);
        let error = run(command, Duration::from_millis(500)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("cleanup "));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "deadline was not enforced"
        );
        assert_gone(&marker);
    }

    #[test]
    fn exited_parent_with_descendant_pipe_has_a_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("descendant.pid");
        let started = Instant::now();
        let mut command = child("descendant");
        command.env("ARTIFACTORY_NATIVE_COMMAND_PID", &marker);
        let error = run(command, Duration::from_millis(500)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "pipe deadline was not enforced"
        );
        assert_gone(&marker);
    }

    fn assert_gone(marker: &std::path::Path) {
        let pid = std::fs::read_to_string(marker)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // TerminateJobObject返回时，Windows仍可短暂报告正在退出的后代。
        // 500ms读回预算小于夹具剩余睡眠，不能把自然退出误判为回收成功。
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            let mut system = sysinfo::System::new();
            system.refresh_processes(
                sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
                true,
            );
            if system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|process| process.status() == sysinfo::ProcessStatus::Zombie)
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "ordinary process survived timeout cleanup"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn dual_streams_are_drained_with_bounded_retention() {
        let result = run(child("streams"), Duration::from_secs(5));
        assert!(
            result.is_err(),
            "oversized output must be rejected, not truncated"
        );
        let error = result.err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("output exceeded"));
    }

    #[test]
    fn explicit_file_limit_preserves_bytes_after_first_megabyte() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("binary");
        let mut bytes = vec![b'x'; (1 << 20) + 37];
        for last in [b'a', b'b'] {
            *bytes.last_mut().unwrap() = last;
            std::fs::write(&path, &bytes).unwrap();
            #[cfg(unix)]
            let mut command = Command::new("cat");
            #[cfg(unix)]
            command.arg(&path);
            #[cfg(windows)]
            let mut command = Command::new("pwsh");
            #[cfg(windows)]
            command.args(["-NoProfile", "-NonInteractive", "-Command", "$b=[IO.File]::ReadAllBytes($env:ARTIFACTORY_NATIVE_COMMAND_FILE); [Console]::OpenStandardOutput().Write($b,0,$b.Length)"])
                .env("ARTIFACTORY_NATIVE_COMMAND_FILE", &path);
            let output = run_with_limit(command, Duration::from_secs(5), 2 << 20).unwrap();
            assert!(output.status.success());
            assert!(
                output.stdout == bytes,
                "binary capture must include the differing tail"
            );
        }
    }

    #[test]
    fn nonzero_status_and_separate_streams_survive() {
        let output = run(child("exit"), Duration::from_secs(5)).unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert!(String::from_utf8_lossy(&output.stdout).contains("stdout-value"));
        assert_eq!(output.stderr, b"stderr-value");
    }

    #[test]
    fn command_input_is_closed() {
        let output = run(child("stdin"), Duration::from_secs(5)).unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("eof"));
    }

    #[test]
    fn database_credentials_remain_in_parent_process() {
        let mut command = child("database_env");
        for name in [
            "DATABASE_URL",
            "ARTIFACTORY_TEST_DATABASE_URL",
            "ARTIFACTORY_BUSINESS_DATABASE_URL",
            "ARTIFACTORY_TELEMETRY_READ_DATABASE_URL",
            "ARTIFACTORY_PLANNING_DATABASE_URL",
        ] {
            command.env(name, "test-only-sentinel");
        }
        assert!(
            run(command, Duration::from_secs(5))
                .unwrap()
                .status
                .success()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn synchronous_command_runs_inside_multithread_tokio() {
        let output = run(child("exit"), Duration::from_secs(5)).unwrap();
        assert_eq!(output.status.code(), Some(7));
    }
}
