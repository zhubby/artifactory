//! [INPUT]: 依赖设备 mTLS、固定控制会话、独立 PTY Worker 和字节 credit。
//! [OUTPUT]: 先登记 PTY 身份，再转发字节、续期与退出，并回收进程树。
//! [POS]: 每会话独立传输；终端输出不进入构建 spool 或控制队列。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    client,
    config::{Config, private_dir},
    process,
    protocol::{TerminalFrame, terminal_client::TerminalClient},
};
use anyhow::{Context, Result, ensure};
use artifactory_emissary_protocol::terminal::{
    LEASE_SECONDS, MAX_BUFFER_BYTES, OpenTerminal, TerminalExit, TerminalHello, TerminalReady,
    validate_frame, validate_size,
};
use artifactory_execution_support::terminal::{TerminalConfig, TerminalEvent, TerminalInput};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    time::Instant,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

fn frame(open: &OpenTerminal, kind: &str) -> TerminalFrame {
    TerminalFrame {
        terminal_id: open.terminal_id.to_string(),
        kind: kind.into(),
        ..Default::default()
    }
}

fn environment() -> BTreeMap<String, String> {
    [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.into(), value)))
    .collect()
}

async fn send(
    tx: &mpsc::Sender<TerminalFrame>,
    frame: TerminalFrame,
    cancel: &CancellationToken,
    lease: Instant,
) -> Result<()> {
    tokio::select! {
        _ = cancel.cancelled() => anyhow::bail!("terminal canceled"),
        _ = tokio::time::sleep_until(lease) => anyhow::bail!("terminal lease expired"),
        result = tokio::time::timeout(Duration::from_secs(3), tx.send(frame)) => { result.context("terminal output congested")?.context("terminal stream closed")?; Ok(()) }
    }
}

fn lease_seconds(payload: &str) -> Result<u64> {
    let payload: serde_json::Value = serde_json::from_str(payload)?;
    let seconds = payload
        .get("lease_seconds")
        .and_then(|v| v.as_u64())
        .context("terminal lease missing")?;
    ensure!(
        (1..=LEASE_SECONDS).contains(&seconds),
        "invalid terminal lease"
    );
    Ok(seconds)
}

pub(super) async fn run_session(
    cfg: Config,
    open: OpenTerminal,
    account: String,
    cancel: CancellationToken,
) -> Result<()> {
    let operation = async {
        let channel = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            channel = client::dial(&cfg, false) => channel?,
        };
        let mut client = TerminalClient::new(channel)
            .max_decoding_message_size(32 * 1024)
            .max_encoding_message_size(32 * 1024);
        let (tx, rx) = mpsc::channel::<TerminalFrame>(64);
        let hello = TerminalHello {
            node_id: cfg.emissary_id.parse()?,
            node_session: open.node_session,
            startup_id: open.startup_id,
        };
        let mut initial = frame(&open, "hello");
        initial.payload_json = serde_json::to_string(&hello)?;
        tx.send(initial).await?;
        let mut stream = tokio::time::timeout(
            Duration::from_secs(12),
            client.connect(ReceiverStream::new(rx)),
        )
        .await??
        .into_inner();
        let start = tokio::time::timeout(Duration::from_secs(5), stream.message())
            .await??
            .context("terminal authorization missing")?;
        validate_frame(&start)?;
        ensure!(
            start.kind == "start" && start.terminal_id == open.terminal_id.to_string(),
            "terminal start mismatch"
        );
        validate_size(start.columns, start.rows)?;
        open.validate(chrono::Utc::now().timestamp_millis())?;
        let mut lease = Instant::now() + Duration::from_secs(lease_seconds(&start.payload_json)?);
        let profile = cfg
            .terminal
            .profiles
            .get(&open.profile)
            .context("terminal profile missing")?;
        let cwd = cfg
            .terminal
            .cwd
            .clone()
            .unwrap_or_else(|| cfg.root.join("terminal-workspace"));
        private_dir(&cwd)?;
        let cwd = cwd.canonicalize()?;
        let worker_config = TerminalConfig {
            shell: profile.shell.clone(),
            args: profile.args.clone(),
            cwd: cwd.clone(),
            columns: start.columns.try_into()?,
            rows: start.rows.try_into()?,
            term: open.term.clone(),
            environment: environment(),
            require_registration: true,
        };
        ensure!(
            !cancel.is_cancelled(),
            "terminal canceled before worker start"
        );
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("terminal-worker")
            .env_clear()
            .envs(environment())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = process::spawn_worker(command)?;
        let mut stdin = child
            .stdin()
            .take()
            .context("terminal worker stdin missing")?;
        let stdout = child
            .stdout()
            .take()
            .context("terminal worker stdout missing")?;
        let (input_tx, mut input_rx) = mpsc::channel::<TerminalInput>(16);
        let writer = tokio::spawn(async move {
            let mut initial = serde_json::to_vec(&worker_config)?;
            initial.push(b'\n');
            stdin.write_all(&initial).await?;
            stdin.flush().await?;
            while let Some(input) = input_rx.recv().await {
                let mut bytes = serde_json::to_vec(&input)?;
                bytes.push(b'\n');
                stdin.write_all(&bytes).await?;
                stdin.flush().await?;
            }
            Ok::<(), anyhow::Error>(())
        });
        let (events_tx, mut events_rx) = mpsc::channel::<Result<TerminalEvent>>(8);
        let reader = tokio::spawn(async move {
            let mut stdout = BufReader::new(stdout);
            loop {
                let event = super::worker::read_record::<serde_json::Value>(&mut stdout).await;
                let parsed = match event {
                    Ok(None) => break,
                    Ok(Some(value))
                        if value.get("type").and_then(|v| v.as_str()) == Some("error") =>
                    {
                        Err(anyhow::anyhow!("terminal worker failed"))
                    }
                    Ok(Some(value)) => serde_json::from_value(value).map_err(anyhow::Error::from),
                    Err(error) => Err(error),
                };
                let failed = parsed.is_err();
                if events_tx.send(parsed).await.is_err() || failed {
                    break;
                }
            }
        });
        #[cfg(unix)]
        let session_file = cfg
            .root
            .join("terminal-sessions")
            .join(format!("{}.json", open.terminal_id));
        #[cfg(unix)]
        let mut process_session: Option<process::Session> = None;
        let mut produced = 0u64;
        let mut consumed = 0u64;
        let mut input_sequence = 0u64;
        let mut ready = false;
        let session_result = async {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break Ok(TerminalExit { reason: "closed".into(), ..Default::default() }),
                    _ = tokio::time::sleep_until(lease) => break Ok(TerminalExit { reason: "lease_expired".into(), ..Default::default() }),
                    next = stream.message() => {
                        let next = next?.context("terminal device stream interrupted")?;
                        validate_frame(&next)?;
                        ensure!(next.terminal_id == open.terminal_id.to_string(), "terminal stream identity changed");
                        match next.kind.as_str() {
                            "lease" => lease = Instant::now() + Duration::from_secs(lease_seconds(&next.payload_json)?),
                            "input" => {
                                ensure!(ready, "terminal input before ready");
                                if next.offset <= input_sequence { continue; }
                                // 短断中未送达的输入不重放；仅拒绝重复或倒序。
                                input_sequence = next.offset;
                                input_tx.try_send(TerminalInput::Input { data: next.data }).context("terminal input congested")?;
                            }
                            "resize" => { input_tx.try_send(TerminalInput::Resize { columns: next.columns.try_into()?, rows: next.rows.try_into()? }).context("terminal resize congested")?; }
                            "ack" => { ensure!(next.offset >= consumed && next.offset <= produced, "invalid terminal output acknowledgement"); consumed = next.offset; }
                            "close" => break Ok(TerminalExit { reason: "closed".into(), ..Default::default() }),
                            _ => anyhow::bail!("unexpected terminal device command"),
                        }
                    }
                    event = events_rx.recv(), if produced.saturating_sub(consumed) <= (MAX_BUFFER_BYTES - 16 * 1024) as u64 => {
                        match event.context("terminal worker exited without status")?? {
                            TerminalEvent::Ready { process_id } => {
                                ensure!(!ready, "duplicate terminal readiness");
                                #[cfg(unix)]
                                {
                                    let session = process::Session::capture_terminal(process_id).await?;
                                    crate::config::atomic_json(&session_file, &session)?;
                                    process_session = Some(session);
                                }
                                #[cfg(windows)]
                                let _ = process_id;
                                input_tx.try_send(TerminalInput::Registered).context("terminal registration congested")?;
                                let mut response = frame(&open, "ready");
                                response.payload_json = serde_json::to_string(&TerminalReady { account: account.clone(), cwd: cwd.to_string_lossy().into_owned(), shell: profile.shell.to_string_lossy().into_owned() })?;
                                send(&tx, response, &cancel, lease).await?;
                                ready = true;
                            }
                            TerminalEvent::Output { data } => {
                                ensure!(ready, "terminal output before ready");
                                let mut output = frame(&open, "output");
                                output.offset = produced;
                                output.data = data;
                                validate_frame(&output)?;
                                produced = produced.checked_add(output.data.len() as u64).context("terminal output offset overflow")?;
                                send(&tx, output, &cancel, lease).await?;
                            }
                            TerminalEvent::Exited { code, signal } => break Ok(TerminalExit { code, signal, reason: "shell_exited".into() }),
                        }
                    }
                }
            }
        }.await;
        drop(input_tx);
        writer.abort();
        let _ = writer.await;
        if tokio::time::timeout(Duration::from_secs(6), child.wait())
            .await
            .is_err()
        {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
        reader.abort();
        let _ = reader.await;
        #[cfg(unix)]
        if let Some(session) = process_session {
            session.terminate().await?;
            let _ = std::fs::remove_file(&session_file);
        }
        let exit = session_result.unwrap_or_else(|error| {
            eprintln!("device terminal transport failed: {error}");
            TerminalExit {
                reason: "terminal_transport_error".into(),
                ..Default::default()
            }
        });
        let mut exited = frame(&open, "exit");
        exited.payload_json = serde_json::to_string(&exit)?;
        // 最终状态不受用户取消令牌屏蔽；等待对端结束以排空请求队列。
        let _ = tokio::time::timeout(Duration::from_secs(2), tx.send(exited)).await;
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), stream.message()).await;
        Ok::<(), anyhow::Error>(())
    };
    operation.await
}

pub(crate) async fn sweep(cfg: &Config) -> Result<()> {
    let root = cfg.root.join("terminal-sessions");
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let session: process::Session = crate::config::read_json(&entry.path())?;
        session.terminate().await?;
        std::fs::remove_file(entry.path())?;
    }
    Ok(())
}
