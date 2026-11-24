//! [INPUT]: 依赖真实 PTY、受控输入、取消信号和进程状态。
//! [OUTPUT]: 验证终端交互、注册期限、排空失败和会话后代回收。
//! [POS]: 可选 terminal 特性的真实进程回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(feature = "terminal")]

use artifactory_execution_support::terminal::{TerminalConfig, TerminalEvent, TerminalInput, run};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
fn config() -> TerminalConfig {
    TerminalConfig {
        shell: PathBuf::from("/bin/sh"),
        args: vec!["-i".into()],
        cwd: std::env::temp_dir(),
        columns: 91,
        rows: 37,
        term: "xterm-256color".into(),
        require_registration: false,
        environment: BTreeMap::from([
            ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
            ("LANG".into(), "en_US.UTF-8".into()),
        ]),
    }
}

#[cfg(windows)]
fn config() -> TerminalConfig {
    let system_root = std::env::var("SystemRoot").expect("Windows SystemRoot is required");
    TerminalConfig {
        shell: PathBuf::from(&system_root).join("System32/cmd.exe"),
        args: vec!["/Q".into(), "/D".into()],
        cwd: std::env::temp_dir(),
        columns: 91,
        rows: 37,
        term: "xterm-256color".into(),
        require_registration: false,
        environment: BTreeMap::from([("SystemRoot".into(), system_root)]),
    }
}

#[tokio::test]
#[cfg(windows)]
async fn terminal_windows_conpty_retains_exit_code() {
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(config(), rx, tx, cancel.clone()));
    assert!(matches!(
        output.recv().await,
        Some(TerminalEvent::Ready { .. })
    ));
    input
        .send(TerminalInput::Input {
            data: b"exit 7\r\n".to_vec(),
        })
        .await
        .unwrap();
    let code = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = output.recv().await {
            if let TerminalEvent::Exited { code, signal } = event {
                assert_eq!(signal, None);
                return code;
            }
        }
        None
    })
    .await
    .expect("ConPTY exit timed out");
    assert_eq!(code, Some(7));
    task.await.unwrap().unwrap();
}

#[cfg(unix)]
async fn collect(command: &[u8]) -> (Vec<u8>, Option<i32>, Option<String>) {
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(config(), rx, tx, cancel.clone()));
    input
        .send(TerminalInput::Input {
            data: command.to_vec(),
        })
        .await
        .unwrap();
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = output.recv().await {
            match event {
                TerminalEvent::Output { data } => bytes.extend(data),
                TerminalEvent::Exited { code, signal } => return (code, signal),
                TerminalEvent::Ready { .. } => {}
            }
        }
        panic!("terminal ended without exit information");
    })
    .await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let (code, signal) = result.expect("terminal interaction timed out");
    (bytes, code, signal)
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_has_real_tty_and_initial_window() {
    let (bytes, code, signal) =
        collect(b"test -t 0 && test -t 1 && printf 'REAL_TTY\\n'; stty size; exit 0\n").await;
    let output = String::from_utf8_lossy(&bytes);
    assert!(
        output.contains("REAL_TTY\r\n"),
        "missing tty evidence: {output}"
    );
    assert!(output.contains("37 91"), "wrong initial window: {output}");
    assert_eq!(code, Some(0));
    assert_eq!(signal, None);
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_preserves_nonzero_exit_statuses() {
    for code in [2, 7] {
        let (_, actual, signal) = collect(format!("exit {code}\n").as_bytes()).await;
        assert_eq!(actual, Some(code), "wrong exit status for {code}");
        assert_eq!(signal, None);
    }
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_preserves_signal_exit() {
    let (_, code, signal) = collect(b"kill -KILL $$\n").await;
    assert_eq!(code, None);
    assert!(signal.is_some(), "signal exit was flattened to code 1");
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_accepts_protocol_window_boundaries() {
    for (columns, rows) in [(0, 1), (1, 0), (501, 1), (1, 501)] {
        let mut cfg = config();
        cfg.columns = columns;
        cfg.rows = rows;
        let (_input, rx) = mpsc::channel(1);
        let (tx, mut output) = mpsc::channel(1);
        assert!(run(cfg, rx, tx, CancellationToken::new()).await.is_err());
        assert!(output.recv().await.is_none());
    }
    let (input, rx) = mpsc::channel(4);
    let (tx, mut output) = mpsc::channel(8);
    let mut cfg = config();
    cfg.columns = 1;
    cfg.rows = 500;
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(cfg, rx, tx, cancel.clone()));
    assert!(matches!(
        output.recv().await,
        Some(TerminalEvent::Ready { .. })
    ));
    input.send(TerminalInput::Close).await.unwrap();
    while output.recv().await.is_some() {}
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_control_c_interrupts_foreground_command() {
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(config(), rx, tx, cancel.clone()));
    assert!(matches!(
        output.recv().await,
        Some(TerminalEvent::Ready { .. })
    ));
    input
        .send(TerminalInput::Input {
            data: b"printf 'RUNNING\\n'; sleep 120\n".to_vec(),
        })
        .await
        .unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !String::from_utf8_lossy(&bytes).contains("\r\nRUNNING\r\n") {
            if let Some(TerminalEvent::Output { data }) = output.recv().await {
                bytes.extend(data);
            }
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    input
        .send(TerminalInput::Input { data: vec![3] })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    input
        .send(TerminalInput::Input {
            data: b"printf 'AFTER_INTERRUPT\\n'; exit 0\n".to_vec(),
        })
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = output.recv().await {
            match event {
                TerminalEvent::Output { data } => bytes.extend(data),
                TerminalEvent::Exited { code, .. } => return code,
                TerminalEvent::Ready { .. } => {}
            }
        }
        None
    })
    .await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(
        result.expect("Ctrl+C did not interrupt foreground sleep"),
        Some(0)
    );
    assert!(String::from_utf8_lossy(&bytes).contains("\r\nAFTER_INTERRUPT\r\n"));
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_parent_can_clean_after_worker_is_killed() {
    const CHILD: &str = "ARTIFACTORY_TEST_TERMINAL_CHILD";
    const MARKER: &str = "ARTIFACTORY_TEST_TERMINAL_MARKER";
    if std::env::var(CHILD).as_deref() == Ok("1") {
        let (input, rx) = mpsc::channel(8);
        let (tx, mut output) = mpsc::channel(8);
        let mut cfg = config();
        cfg.environment
            .insert("MARKER".into(), std::env::var(MARKER).unwrap());
        let task = tokio::spawn(run(cfg, rx, tx, CancellationToken::new()));
        input
            .send(TerminalInput::Input {
                data: b"sh -c 'trap \"\" HUP; printf \"%s\" $$ > \"$MARKER.tmp\"; mv \"$MARKER.tmp\" \"$MARKER\"; sleep 120 & wait' & wait\n".to_vec(),
            })
            .await
            .unwrap();
        while let Some(event) = output.recv().await {
            if let TerminalEvent::Ready { process_id } = event {
                println!("TERMINAL_PID={process_id}");
            }
        }
        task.await.unwrap().unwrap();
        return;
    }
    use tokio::io::{AsyncBufReadExt, BufReader};
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("orphan.pid");
    let mut worker = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "terminal_parent_can_clean_after_worker_is_killed",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(MARKER, &marker)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(worker.stdout.take().unwrap()).lines();
    let terminal_pid = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let line = stdout
                .next_line()
                .await
                .unwrap()
                .expect("worker quit before PTY ready");
            if let Some(pid) = line.strip_prefix("TERMINAL_PID=") {
                break pid.parse::<u32>().unwrap();
            }
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let child_pid = std::fs::read_to_string(&marker)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    worker.start_kill().unwrap();
    worker.wait().await.unwrap();
    let mut surviving = sysinfo::System::new();
    surviving.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(child_pid)]),
        true,
    );
    assert!(
        surviving
            .process(sysinfo::Pid::from_u32(child_pid))
            .is_some_and(|p| p.status() != sysinfo::ProcessStatus::Zombie),
        "test child did not survive worker death; SID cleanup was not exercised"
    );
    artifactory_execution_support::terminal::terminate_session(terminal_pid)
        .await
        .unwrap();
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[
            sysinfo::Pid::from_u32(terminal_pid),
            sysinfo::Pid::from_u32(child_pid),
        ]),
        true,
    );
    for pid in [terminal_pid, child_pid] {
        assert!(
            system
                .process(sysinfo::Pid::from_u32(pid))
                .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
            "process {pid} survived supervisor cleanup"
        );
    }
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_handles_resize_and_unicode_bytes() {
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(config(), rx, tx, cancel.clone()));
    assert!(matches!(
        output.recv().await,
        Some(TerminalEvent::Ready { .. })
    ));
    input
        .send(TerminalInput::Resize {
            columns: 113,
            rows: 43,
        })
        .await
        .unwrap();
    input
        .send(TerminalInput::Input {
            data: "stty size; printf '中文终端\\n'; exit 0\n"
                .as_bytes()
                .to_vec(),
        })
        .await
        .unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = output.recv().await {
            match event {
                TerminalEvent::Output { data } => bytes.extend(data),
                TerminalEvent::Exited { code, signal } => {
                    assert_eq!(code, Some(0));
                    assert_eq!(signal, None);
                    break;
                }
                TerminalEvent::Ready { .. } => {}
            }
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    task.await.unwrap().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("43 113"), "resize was not applied: {text}");
    assert!(text.contains("中文终端"), "unicode bytes changed: {text}");
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_rejects_missing_shell_without_fallback() {
    let mut cfg = config();
    cfg.shell = PathBuf::from("/artifactory-terminal-missing-shell");
    let (_input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    assert!(run(cfg, rx, tx, CancellationToken::new()).await.is_err());
    assert!(output.recv().await.is_none(), "failed PTY reported ready");
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_cancel_reaps_background_processes() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("terminal-child.pid");
    let mut cfg = config();
    cfg.environment
        .insert("MARKER".into(), marker.to_string_lossy().into_owned());
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(cfg, rx, tx, cancel.clone()));
    input
        .send(TerminalInput::Input {
            data: b"sleep 120 & printf '%s' $! > \"$MARKER.tmp\"; mv \"$MARKER.tmp\" \"$MARKER\"; wait\n".to_vec(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !marker.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let pid = std::fs::read_to_string(marker)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    cancel.cancel();
    let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("cancel hung")
        .unwrap()
        .unwrap();
    drain.await.unwrap();
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(pid)]),
        true,
    );
    assert!(
        system
            .process(sysinfo::Pid::from_u32(pid))
            .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
        "terminal descendant {pid} survived cancel"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn stalled_output_does_not_block_cancel_or_leave_processes() {
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let _cancel_on_drop = cancel.clone().drop_guard();
    let task = tokio::spawn(run(config(), rx, tx, cancel.clone()));
    let Some(TerminalEvent::Ready { process_id }) = output.recv().await else {
        panic!("PTY did not become ready");
    };
    input
        .send(TerminalInput::Input {
            data: b"yes\n".to_vec(),
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while output.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let started = tokio::time::Instant::now();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("stalled output prevented cancellation")
        .unwrap();
    assert!(
        result.is_err(),
        "stalled consumer silently lost the exit event"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(process_id)]),
        true,
    );
    assert!(
        system
            .process(sysinfo::Pid::from_u32(process_id))
            .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie),
        "PTY survived cancellation with a full output queue"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_drain_failure_never_reports_normal_exit() {
    let mut cfg = config();
    cfg.args = vec![
        "-c".into(),
        "printf first; sleep 0.2; printf last; exit 0".into(),
    ];
    let (_input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(1);
    let task = tokio::spawn(run(cfg, rx, tx, CancellationToken::new()));
    assert!(matches!(
        output.recv().await,
        Some(TerminalEvent::Ready { .. })
    ));
    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut normal_exit = false;
    while let Some(event) = output.recv().await {
        normal_exit |= matches!(event, TerminalEvent::Exited { .. });
    }
    let error = task
        .await
        .unwrap()
        .expect_err("output drain timeout must be an error");
    assert!(
        error.to_string().contains("did not drain"),
        "unexpected failure: {error}"
    );
    assert!(
        !normal_exit,
        "known output loss was reported as a normal shell exit"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_parent_eof_before_registration_reaps_child() {
    let mut cfg = config();
    cfg.require_registration = true;
    cfg.args = vec!["-c".into(), "sleep 120".into()];
    let (input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let task = tokio::spawn(run(cfg, rx, tx, CancellationToken::new()));
    let Some(TerminalEvent::Ready { process_id }) = output.recv().await else {
        panic!("PTY did not become ready");
    };
    drop(input);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(process_id)]),
        true,
    );
    assert!(
        system
            .process(sysinfo::Pid::from_u32(process_id))
            .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie)
    );
}

#[tokio::test]
#[cfg(unix)]
async fn terminal_unconfirmed_registration_times_out_and_reaps_child() {
    let mut cfg = config();
    cfg.require_registration = true;
    cfg.args = vec!["-c".into(), "sleep 120".into()];
    let (_input, rx) = mpsc::channel(8);
    let (tx, mut output) = mpsc::channel(8);
    let task = tokio::spawn(run(cfg, rx, tx, CancellationToken::new()));
    let Some(TerminalEvent::Ready { process_id }) = output.recv().await else {
        panic!("PTY did not become ready");
    };
    let error = tokio::time::timeout(Duration::from_secs(8), task)
        .await
        .unwrap()
        .unwrap()
        .expect_err("missing parent registration must time out");
    assert!(error.to_string().contains("registration timed out"));
    while let Ok(event) = output.try_recv() {
        assert!(
            !matches!(event, TerminalEvent::Exited { .. }),
            "failed registration reported success"
        );
    }
    let mut system = sysinfo::System::new();
    system.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[sysinfo::Pid::from_u32(process_id)]),
        true,
    );
    assert!(
        system
            .process(sysinfo::Pid::from_u32(process_id))
            .is_none_or(|p| p.status() == sysinfo::ProcessStatus::Zombie)
    );
}

#[test]
fn terminal_debug_output_does_not_expose_input_or_environment_values() {
    let secret = "terminal-secret-value";
    let mut cfg = config();
    cfg.environment
        .insert("LOCAL_PRIVATE_VALUE".into(), secret.into());
    cfg.args.push(secret.into());
    assert!(!format!("{cfg:?}").contains(secret));
    assert!(
        !format!(
            "{:?}",
            TerminalInput::Input {
                data: secret.as_bytes().to_vec()
            }
        )
        .contains(secret)
    );
    assert!(
        !format!(
            "{:?}",
            TerminalEvent::Output {
                data: secret.as_bytes().to_vec()
            }
        )
        .contains(secret)
    );
}
