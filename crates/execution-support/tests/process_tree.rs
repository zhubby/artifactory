//! [INPUT]: 依赖真实工具进程、工作目录和取消信号。
//! [OUTPUT]: 验证输入EOF、取消/超时后代回收与失败日志读取截止。
//! [POS]: 共享进程原语的跨平台回归；PID 标记原子发布。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use artifactory_execution_support::process;
use std::{fs, time::Duration};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn tools_receive_eof_without_reading_parent_input() {
    const CHILD: &str = "ARTIFACTORY_TEST_TOOL_STDIN";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        let command = || {
            #[cfg(unix)]
            let mut command = Command::new("sh");
            #[cfg(unix)]
            command.args(["-c", "cat; printf tool-eof"]);
            #[cfg(windows)]
            let mut command = Command::new("pwsh");
            #[cfg(windows)]
            command.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[Console]::In.ReadToEnd(); Write-Output tool-eof",
            ]);
            command
        };
        let timeout = Duration::from_secs(3);
        let (status, bytes) = process::output(command(), &CancellationToken::new(), timeout)
            .await
            .expect("tool must see EOF while the parent input pipe remains open");
        assert!(status.success());
        assert!(String::from_utf8_lossy(&bytes).contains("tool-eof"));
        let (status, bytes) = process::probe(command(), timeout)
            .await
            .expect("probe must see EOF while the parent input pipe remains open");
        assert!(status.success());
        assert!(String::from_utf8_lossy(&bytes).contains("tool-eof"));
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tools_receive_eof_without_reading_parent_input",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    // 保持父进程输入管道开启，以识别工具继承输入后的阻塞。
    let _input = child.stdin.take().unwrap();
    let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .expect("tool input isolation test hung")
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn process_tree_terminates_descendants() {
    terminates_descendants(true).await;
}

#[tokio::test]
async fn timeout_terminates_descendants() {
    terminates_descendants(false).await;
}

async fn terminates_descendants(canceled: bool) {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("descendant.pid");
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "sh -c 'sleep 120 & wait' & pid=$!; printf '%s\\n' \"$pid\" > \"$MARKER.tmp\" && mv \"$MARKER.tmp\" \"$MARKER\"; wait",
        ]);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("pwsh");
        command.args(["-NoProfile","-Command","$p=Start-Process pwsh -ArgumentList '-NoProfile','-Command','Start-Sleep 120' -PassThru; [IO.File]::WriteAllText($env:MARKER+'.tmp',$p.Id.ToString()); [IO.File]::Move($env:MARKER+'.tmp',$env:MARKER); Wait-Process -Id $p.Id"]);
        command
    };
    command
        .env("MARKER", &marker)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = process::spawn(command).unwrap();
    let stdout = child.stdout().take().unwrap();
    let stderr = child.stderr().take().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    let pid = fs::read_to_string(&marker)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    let cancel = CancellationToken::new();
    if canceled {
        cancel.cancel();
    }
    assert!(
        process::wait(&mut *child, &cancel, Duration::from_millis(100))
            .await
            .is_err()
    );
    use tokio::io::AsyncReadExt;
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut stdout = stdout;
        let mut stderr = stderr;
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (out_result, err_result) =
            tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        out_result.unwrap();
        err_result.unwrap();
    })
    .await
    .expect("canceled descendants must close inherited output pipes");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let mut system = sysinfo::System::new();
            system.refresh_processes(
                sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
                true,
            );
            if system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_during_fork_closes_inherited_output() {
    for _ in 0..12 {
        terminates_descendants(true).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn failed_reader_aborts_when_process_keeps_pipe_open() {
    use tokio::io::AsyncReadExt;
    struct ReaderGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for ReaderGuard {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let mut child = Command::new("sleep")
        .arg("120")
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ReaderGuard(dropped.clone());
    let reader = tokio::spawn(async move {
        let _guard = guard;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await
    });
    let result = tokio::time::timeout(Duration::from_secs(4), process::finish_reader(reader, true))
        .await
        .expect("failed output reader must have a deadline");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("output remained open")
    );
    assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
}
