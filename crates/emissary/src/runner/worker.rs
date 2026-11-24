//! [INPUT]: 依赖任务方案、私有父管道和隔离进程监督。
//! [OUTPUT]: 启动构建 Worker，转发租约与事件并完成源码会话回收。
//! [POS]: Runner 的构建子进程边界；与独立终端 Worker 分离。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Message;
use crate::{
    config::Config,
    process, protocol as pb, source,
    worker::{Event, Input},
};
use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};

pub(super) async fn worker_task(
    mut cfg: Config,
    id: String,
    token: String,
    assignment: pb::Assignment,
    seconds: u32,
    (mut controls, registry): (mpsc::Receiver<Input>, crate::telemetry::Registry),
    send: mpsc::Sender<Message>,
) -> Result<()> {
    crate::signing::ready(&cfg)?;
    source::ready(&cfg)?;
    cfg.signing_profiles.retain(|name, _| {
        assignment.managed_signing_json.is_empty() && assignment.signing_profiles.contains(name)
    });
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = process::spawn_worker(command)?;
    source::register_session(&cfg, &id, child.id().context("worker PID missing")?)?;
    let attempt = uuid::Uuid::parse_str(&id)?;
    let _managed = if let Some(pid) = child.id() {
        registry.register(attempt, pid).await.ok()
    } else {
        None
    };
    let result = async {
        let mut stdin = child.stdin().take().context("worker stdin missing")?;
        let stdout = child.stdout().take().context("worker stdout missing")?;
        let initial = Input {
            kind: "start".into(),
            config: cfg.clone(),
            attempt_id: id.clone(),
            lease_token: token,
            lease_seconds: seconds,
            assignment: Some(assignment),
            ..Default::default()
        };
        let mut bytes = serde_json::to_vec(&initial)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes).await?;
        let writer = tokio::spawn(async move {
            while let Some(input) = controls.recv().await {
                let mut bytes = serde_json::to_vec(&input)?;
                bytes.push(b'\n');
                stdin.write_all(&bytes).await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let read = async {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) = lines.next_line().await? {
                let event: Event = serde_json::from_str(&line)?;
                if event.kind == "step_started" {
                    registry.step(
                        attempt,
                        event.payload["index"]
                            .as_u64()
                            .and_then(|n| u32::try_from(n).ok()),
                    );
                }
                if event.kind == "step_finished" {
                    registry.step(attempt, None);
                }
                send.send(Message::Event(id.clone(), event)).await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        writer.abort();
        let _ = writer.await;
        read
    }
    .await;
    if result.is_err() {
        let _ = child.start_kill();
    }
    let waited = child.wait().await;
    source::settle_attempt(&cfg, &id).await?;
    result?;
    waited?;
    Ok(())
}
