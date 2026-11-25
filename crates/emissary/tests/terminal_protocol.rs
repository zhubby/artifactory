//! [INPUT]: 依赖真实 Emissary 子进程、loopback mTLS 与原生 PTY。
//! [OUTPUT]: 验证独立字节流、逐字节突发、即刻退出状态和设备租约。
//! [POS]: 设备端完整终端链回归；不模拟 PTY 或进程监督。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]
mod support;
use artifactory_emissary::protocol::{
    self as pb,
    terminal::{OpenTerminal, TerminalExit, TerminalHello},
};
use serde_json::json;
use std::{process::Stdio, time::Duration};
use tokio::process::{Child, Command};
use uuid::Uuid;

async fn terminal(
    lease: u64,
) -> (
    tempfile::TempDir,
    support::Fixture,
    Child,
    support::Connection,
    support::TerminalConnection,
) {
    terminal_profile(lease, vec!["-i".into()]).await
}

async fn terminal_profile(
    lease: u64,
    args: Vec<String>,
) -> (
    tempfile::TempDir,
    support::Fixture,
    Child,
    support::Connection,
    support::TerminalConnection,
) {
    terminal_profile_locale(lease, args, None).await
}

async fn terminal_profile_locale(
    lease: u64,
    args: Vec<String>,
    locale: Option<&str>,
) -> (
    tempfile::TempDir,
    support::Fixture,
    Child,
    support::Connection,
    support::TerminalConnection,
) {
    let directory = tempfile::tempdir().unwrap();
    let mut fixture = support::Fixture::start(directory.path(), &[&rustls::version::TLS13]).await;
    fixture.cfg.terminal.enabled = true;
    fixture
        .cfg
        .terminal
        .profiles
        .get_mut("default")
        .unwrap()
        .args = args;
    let config = directory.path().join("emissary.json");
    fixture.cfg.save(&config).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"));
    if let Some(locale) = locale {
        for name in ["LANG", "LC_ALL", "LC_CTYPE"] {
            command.env_remove(name);
        }
        if !locale.is_empty() {
            command.env("LANG", locale);
        }
    }
    let child = command
        .args(["run", "--config"])
        .arg(&config)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let connection = fixture.connection().await;
    let startup: serde_json::Value = serde_json::from_str(&connection.hello.payload_json).unwrap();
    assert!(
        startup["startup"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "terminal-v1")
    );
    let startup_id = Uuid::parse_str(startup["startup"]["startup_id"].as_str().unwrap()).unwrap();
    let session = Uuid::new_v4();
    connection
        .commands
        .send(pb::Command {
            kind: "metadata_ack".into(),
            request_id: startup_id.to_string(),
            signing_session_id: session.to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let open = OpenTerminal {
        terminal_id: Uuid::new_v4(),
        node_session: session,
        startup_id,
        expires_at_ms: chrono::Utc::now().timestamp_millis() + 30_000,
        columns: 80,
        rows: 24,
        term: "xterm-256color".into(),
        profile: "default".into(),
    };
    connection
        .commands
        .send(pb::Command {
            kind: "terminal_open".into(),
            payload_json: serde_json::to_string(&open).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(15), fixture.terminals.recv())
        .await
        .unwrap()
        .unwrap();
    let hello: TerminalHello = serde_json::from_str(&terminal.hello.payload_json).unwrap();
    assert_eq!(hello.node_session, session);
    assert_eq!(hello.startup_id, startup_id);
    terminal
        .commands
        .send(Ok(pb::TerminalFrame {
            terminal_id: open.terminal_id.to_string(),
            kind: "start".into(),
            columns: 80,
            rows: 24,
            payload_json:
                json!({"profile":"default","term":"xterm-256color","lease_seconds":lease})
                    .to_string(),
            ..Default::default()
        }))
        .await
        .unwrap();
    (directory, fixture, child, connection, terminal)
}

async fn event(terminal: &mut support::TerminalConnection) -> pb::TerminalFrame {
    tokio::time::timeout(Duration::from_secs(12), terminal.events.recv())
        .await
        .unwrap()
        .expect("终端必须返回状态")
}

async fn stop(child: &mut Child) {
    assert!(
        std::process::Command::new("kill")
            .args(["-TERM", &child.id().unwrap().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(15), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn macos_terminal_without_locale_preserves_unicode_shell_input() {
    let (_directory, _fixture, mut child, _control, mut terminal) =
        terminal_profile_locale(45, vec!["-i".into()], Some("")).await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    terminal
        .commands
        .send(Ok(pb::TerminalFrame {
            terminal_id: terminal.hello.terminal_id.clone(),
            kind: "input".into(),
            offset: 1,
            data: "printf '__UTF8_%s__\\n' '中文'; exit 7\n"
                .as_bytes()
                .to_vec(),
            ..Default::default()
        }))
        .await
        .unwrap();
    let mut output = Vec::new();
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "output" {
            output.extend(frame.data);
        } else if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.code, Some(7));
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&output).contains("__UTF8_中文__"),
        "{}",
        String::from_utf8_lossy(&output)
    );
    stop(&mut child).await;
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn macos_terminal_preserves_explicit_user_locale() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal_profile_locale(
        45,
        vec![
            "-c".into(),
            "printf '__LOCALE_%s__\\n' \"$LANG\"; exit 7".into(),
        ],
        Some("C"),
    )
    .await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    let mut output = Vec::new();
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "output" {
            output.extend(frame.data);
        } else if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.code, Some(7));
            break;
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("__LOCALE_C__"));
    stop(&mut child).await;
}

#[tokio::test]
async fn terminal_worker_invalid_input_never_reports_a_normal_exit() {
    use artifactory_execution_support::terminal::TerminalConfig;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let directory = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_artifactory-emissary"))
        .arg("terminal-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let config = TerminalConfig {
        shell: "/bin/sh".into(),
        args: vec!["-c".into(), "sleep 60".into()],
        cwd: directory.path().into(),
        columns: 80,
        rows: 24,
        term: "xterm-256color".into(),
        environment: std::collections::BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
        require_registration: false,
    };
    let mut bytes = serde_json::to_vec(&config).unwrap();
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.unwrap();
    stdin.flush().await.unwrap();
    let ready: serde_json::Value =
        serde_json::from_str(&stdout.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(ready["type"], "ready");
    stdin.write_all(b"{invalid}\n").await.unwrap();
    stdin.flush().await.unwrap();
    let records = tokio::time::timeout(Duration::from_secs(8), async {
        let mut records = Vec::new();
        while let Some(line) = stdout.next_line().await.unwrap() {
            records.push(serde_json::from_str::<serde_json::Value>(&line).unwrap());
        }
        records
    })
    .await
    .expect("invalid worker record did not close PTY");
    assert!(records.iter().any(|record| record["type"] == "error"));
    assert!(!records.iter().any(|record| record["type"] == "exited"));
    assert!(!child.wait().await.unwrap().success());
}

#[tokio::test]
async fn character_input_burst_preserves_every_byte_and_shell_exit() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal(45).await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    let command = format!(
        "printf '__BURST_%s__\\n' 'OK'; #{}\nexit 7\n",
        "x".repeat(256)
    );
    for (index, byte) in command.bytes().enumerate() {
        terminal
            .commands
            .send(Ok(pb::TerminalFrame {
                terminal_id: terminal.hello.terminal_id.clone(),
                kind: "input".into(),
                offset: index as u64 + 1,
                data: vec![byte],
                ..Default::default()
            }))
            .await
            .unwrap();
    }
    let mut output = Vec::new();
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "output" {
            assert_eq!(frame.offset, output.len() as u64);
            output.extend(frame.data);
        } else if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.reason, "shell_exited", "{exit:?}");
            assert_eq!(exit.code, Some(7));
            break;
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("__BURST_OK__"));
    stop(&mut child).await;
}

#[tokio::test]
async fn lost_input_sequence_is_skipped_without_replay_or_shell_restart() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal(45).await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    for (sequence, data) in [
        (1, "printf 'FIRST_OK\\n'\n"),
        (1, "exit 99\n"),
        (3, "printf 'AFTER_GAP_OK\\n'; exit 7\n"),
    ] {
        terminal
            .commands
            .send(Ok(pb::TerminalFrame {
                terminal_id: terminal.hello.terminal_id.clone(),
                kind: "input".into(),
                offset: sequence,
                data: data.as_bytes().to_vec(),
                ..Default::default()
            }))
            .await
            .unwrap();
    }
    let mut output = Vec::new();
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "output" {
            output.extend(frame.data);
        }
        if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.code, Some(7), "{exit:?}");
            assert_eq!(exit.reason, "shell_exited");
            break;
        }
    }
    assert!(String::from_utf8_lossy(&output).contains("AFTER_GAP_OK"));
    stop(&mut child).await;
}

#[tokio::test]
async fn real_device_terminal_uses_mtls_pty_bytes_and_exact_exit_code() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal(45).await;
    let ready = event(&mut terminal).await;
    assert_eq!(ready.kind, "ready");
    let ready_data: serde_json::Value = serde_json::from_str(&ready.payload_json).unwrap();
    assert!(!ready_data["account"].as_str().unwrap().is_empty());
    terminal
        .commands
        .send(Ok(pb::TerminalFrame {
            terminal_id: terminal.hello.terminal_id.clone(),
            kind: "input".into(),
            offset: 1,
            data: "test -t 0 && printf '__REAL_PTY__'; stty size; printf '中文'; exit 7\n"
                .as_bytes()
                .to_vec(),
            ..Default::default()
        }))
        .await
        .unwrap();
    let mut output = Vec::new();
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "output" {
            assert_eq!(frame.offset, output.len() as u64);
            output.extend(&frame.data);
            terminal
                .commands
                .send(Ok(pb::TerminalFrame {
                    terminal_id: frame.terminal_id,
                    kind: "ack".into(),
                    offset: output.len() as u64,
                    ..Default::default()
                }))
                .await
                .unwrap();
        } else if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.code, Some(7));
            assert_eq!(exit.reason, "shell_exited");
            break;
        }
    }
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("__REAL_PTY__"));
    assert!(text.contains("24 80"));
    assert!(text.contains("中文"));
    stop(&mut child).await;
}

#[tokio::test]
async fn full_terminal_input_queue_keeps_the_device_lease_deadline() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal_profile(
        1,
        vec![
            "-c".into(),
            "stty -echo -icanon; printf '__INPUT_BLOCKED__\\n'; sleep 60".into(),
        ],
    )
    .await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains("__INPUT_BLOCKED__") {
        let frame = event(&mut terminal).await;
        assert_eq!(frame.kind, "output");
        output.extend(frame.data);
    }
    let commands = terminal.commands.clone();
    let id = terminal.hello.terminal_id.clone();
    let producer = tokio::spawn(async move {
        for offset in 1..=256 {
            if commands
                .send(Ok(pb::TerminalFrame {
                    terminal_id: id.clone(),
                    kind: "input".into(),
                    offset,
                    data: vec![b'x'; 16 * 1024],
                    ..Default::default()
                }))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.reason, "lease_expired", "{exit:?}");
            break;
        }
    }
    producer.abort();
    let _ = producer.await;
    stop(&mut child).await;
}

#[tokio::test]
async fn missing_terminal_lease_closes_the_worker() {
    let (_directory, _fixture, mut child, _control, mut terminal) = terminal(1).await;
    assert_eq!(event(&mut terminal).await.kind, "ready");
    loop {
        let frame = event(&mut terminal).await;
        if frame.kind == "exit" {
            let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
            assert_eq!(exit.reason, "lease_expired");
            assert_eq!(exit.code, None);
            break;
        }
    }
    stop(&mut child).await;
}

#[tokio::test]
async fn immediately_exiting_profile_retains_ready_and_exact_exit_code() {
    for _ in 0..4 {
        let (_directory, _fixture, mut child, _control, mut terminal) =
            terminal_profile(45, vec!["-c".into(), "exit 7".into()]).await;
        assert_eq!(event(&mut terminal).await.kind, "ready");
        loop {
            let frame = event(&mut terminal).await;
            if frame.kind == "exit" {
                let exit: TerminalExit = serde_json::from_str(&frame.payload_json).unwrap();
                assert_eq!(exit.code, Some(7));
                assert_eq!(exit.reason, "shell_exited");
                break;
            }
        }
        stop(&mut child).await;
    }
}
