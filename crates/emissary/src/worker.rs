//! [INPUT]: 依赖固定清单、租约、私有配置和共享日志事件管道。
//! [OUTPUT]: 执行步骤并上传制品；失败时有界关闭日志，再检查目录清理。
//! [POS]: 每任务子进程；凭据在用户步骤前删除。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    artifacts, client,
    config::{Config, private_dir, private_write},
    process, protocol as pb, source,
};
use anyhow::{Context, Result, ensure};
pub use artifactory_execution_support::events::{Emitter, Event};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt,
        BufReader,
    },
    process::Command,
};
use tokio_util::{io::ReaderStream, sync::CancellationToken};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Input {
    pub kind: String,
    pub config: Config,
    pub attempt_id: String,
    pub lease_token: String,
    pub lease_seconds: u32,
    pub assignment: Option<pb::Assignment>,
    pub reason: String,
    pub signing_session_id: String,
}
#[derive(Clone, Copy, Debug)]
pub struct Lease {
    deadline: Instant,
}
impl Lease {
    pub fn new(now: Instant, seconds: u32) -> Self {
        Self {
            deadline: now + Duration::from_secs(seconds.into()),
        }
    }
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn renew(&mut self, now: Instant, anchor: Instant, seconds: u32) -> bool {
        let deadline = anchor + Duration::from_secs(seconds.into());
        if self.expired(now) || now >= deadline {
            return false;
        }
        self.deadline = deadline;
        true
    }
}
#[derive(Debug, thiserror::Error)]
#[error("{error}")]
pub struct Failure {
    pub kind: &'static str,
    #[source]
    pub error: anyhow::Error,
}
impl Failure {
    fn wrap(kind: &'static str, error: anyhow::Error) -> anyhow::Error {
        Self { kind, error }.into()
    }
}

pub async fn run(
    input: impl AsyncBufRead + Unpin,
    mut output: impl AsyncWrite + Unpin + Send + 'static,
    parent: CancellationToken,
) -> Result<()> {
    let mut lines = input.lines();
    let line = lines
        .next_line()
        .await?
        .context("worker requires a start assignment")?;
    let initial: Input = serde_json::from_str(&line)?;
    ensure!(
        initial.kind == "start"
            && source::valid_id(&initial.attempt_id)
            && initial.assignment.is_some()
            && initial.lease_seconds > 0,
        "invalid worker assignment"
    );
    let assignment_timeout = initial
        .assignment
        .as_ref()
        .context("missing assignment")?
        .timeout_seconds;
    let mut lease = Lease::new(Instant::now(), initial.lease_seconds);
    let cancel = parent.child_token();
    let (emit, mut events) = Emitter::channel();
    let writer = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let mut line = serde_json::to_vec(&event)?;
            line.push(b'\n');
            output.write_all(&line).await?;
            output.flush().await?;
        }
        Ok::<_, anyhow::Error>(())
    });
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(assignment_timeout.into());
    let mut reason = None;
    let session = crate::signing::Assignment::parse(
        &initial
            .assignment
            .as_ref()
            .context("missing assignment")?
            .managed_signing_json,
    )?
    .session_id;
    let (sessions, session_updates) = tokio::sync::watch::channel(session);
    let result = {
        let result = execute_with_session(&initial, &cancel, &emit, session_updates);
        tokio::pin!(result);
        loop {
            tokio::select! {
                result=&mut result => break result,
                line=lines.next_line(), if reason.is_none() => {
                    match line {
                        Ok(Some(line)) => match serde_json::from_str::<Input>(&line) {
                            Ok(message) if message.kind=="renew" => {
                                if !message.signing_session_id.is_empty() { sessions.send_replace(message.signing_session_id); }
                                let now=Instant::now();
                                if !lease.renew(now,now,message.lease_seconds) {reason=Some("execution lease expired".to_string());cancel.cancel();}
                            },
                            Ok(message) if message.kind=="cancel" => {reason=Some(message.reason);cancel.cancel();},
                            Ok(_) => {},
                            Err(_) => {reason=Some("invalid worker control message".into());cancel.cancel();},
                        },
                        _ => {reason=Some("Emissary control pipe closed".into());cancel.cancel();}
                    }
                },
                _=parent.cancelled(), if reason.is_none() => {reason=Some("worker stopping".into());cancel.cancel();},
                _=tokio::time::sleep_until(deadline), if reason.is_none() => {reason=Some("execution timed out".into());cancel.cancel();},
                _=tick.tick(), if reason.is_none() => if lease.expired(Instant::now()) {reason=Some("execution lease expired".into());cancel.cancel();},
            }
        }
    };
    let kind = result
        .as_ref()
        .err()
        .and_then(|e| e.downcast_ref::<Failure>())
        .map(|e| e.kind)
        .unwrap_or("execution");
    let (status, message) = match reason {
        Some(reason) => (
            if reason == "cancel requested" {
                "canceled"
            } else {
                "failed"
            },
            reason,
        ),
        None => match result {
            Ok(()) => ("succeeded", String::new()),
            Err(e) => ("failed", e.to_string()),
        },
    };
    emit.send(Event {
        kind: "finished".into(),
        payload: json!({"status":status,"error":message,"failure_kind":kind}),
        ..Default::default()
    })
    .await?;
    drop(emit);
    writer.await??;
    Ok(())
}

pub async fn execute(input: &Input, cancel: &CancellationToken, emit: &Emitter) -> Result<()> {
    let session = crate::signing::Assignment::parse(
        &input
            .assignment
            .as_ref()
            .context("missing assignment")?
            .managed_signing_json,
    )?
    .session_id;
    let (_, sessions) = tokio::sync::watch::channel(session);
    execute_with_session(input, cancel, emit, sessions).await
}
async fn execute_with_session(
    input: &Input,
    cancel: &CancellationToken,
    emit: &Emitter,
    sessions: tokio::sync::watch::Receiver<String>,
) -> Result<()> {
    let mut materials = None;
    let result = execute_inner(input, cancel, emit, &mut materials, sessions).await;
    if let Some(materials) = &mut materials {
        materials.cleanup(&input.config).await?;
    }
    source::cleanup_attempt(&input.config, &input.attempt_id)?;
    result
}
async fn execute_inner(
    input: &Input,
    cancel: &CancellationToken,
    emit: &Emitter,
    materials: &mut Option<crate::signing::Materials>,
    sessions: tokio::sync::watch::Receiver<String>,
) -> Result<()> {
    let assignment = input.assignment.as_ref().context("missing assignment")?;
    let mut managed = crate::signing::Assignment::parse(&assignment.managed_signing_json)?;
    managed.sessions = Some(sessions);
    let mut secrets = crate::signing::redact::Secrets::default();
    source::ready(&input.config)?;
    let _attempt_lock = source::attempt_lock(&input.config, &input.attempt_id)?;
    let work = input.config.root.join("work");
    private_dir(&work)?;
    let base = work.join(&input.attempt_id);
    // create_dir is deliberately exclusive: a replay cannot share an active workspace.
    fs::create_dir(&base).context("attempt workspace already exists or is unavailable")?;
    let base = Workspace(base);
    let workspace = base.path().join("source");
    let scripts = base.path().join("scripts");
    private_dir(&scripts)?;
    source::checkout(input, base.path(), &workspace, cancel, emit)
        .await
        .map_err(|e| Failure::wrap("source", e))?;
    for script in &assignment.scripts {
        ensure!(
            safe_script_name(&script.name),
            "unsafe external script name"
        );
        ensure!(
            hex::encode(Sha256::digest(script.content.as_bytes())) == script.sha256,
            "external script digest mismatch"
        );
        private_write(
            &scripts.join(&script.name),
            script.content.as_bytes(),
            true,
            true,
        )?;
    }
    for (index, step) in assignment.steps.iter().enumerate() {
        ensure!(!cancel.is_cancelled(), "execution canceled");
        let mut env = source::build_environment();
        env.extend(
            assignment
                .environment
                .iter()
                .map(|(k, v)| (k.into(), v.into())),
        );
        source::isolate_environment(base.path(), &mut env)?;
        env.insert(
            "ARTIFACTORY_SCRIPT_DIR".into(),
            scripts.clone().into_os_string(),
        );
        env.insert(
            "ARTIFACTORY_WORK_DIR".into(),
            base.path().as_os_str().into(),
        );
        for (key, value) in [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", source::NULL),
        ] {
            env.insert(key.into(), value.into());
        }
        emit.send(Event {
            kind: "step_started".into(),
            payload: json!({"index":index}),
            ..Default::default()
        })
        .await?;
        if step.phase != "sign"
            && let Some(mut active) = materials.take()
        {
            active.cleanup(&input.config).await?;
        }
        if step.phase == "sign" && !managed.bindings.is_empty() {
            if materials.is_none() {
                let active = crate::signing::Materials::acquire(input, &managed, index, cancel)
                    .await
                    .map_err(|e| Failure::wrap("authorization", e))?;
                secrets.0.extend(active.secrets.0.clone());
                *materials = Some(active);
            }
            env.extend(
                materials
                    .as_ref()
                    .context("signing material missing")?
                    .environment
                    .clone(),
            );
        } else if managed.bindings.is_empty() && matches!(step.phase.as_str(), "sign" | "verify") {
            for name in &assignment.signing_profiles {
                ensure!(
                    !name.starts_with("managed:"),
                    "managed profile requires fixed credential binding"
                );
                let profile = input
                    .config
                    .signing_profiles
                    .get(name)
                    .with_context(|| format!("local signing profile unavailable: {name}"))
                    .map_err(|e| Failure::wrap("authorization", e))?;
                env.extend(profile.iter().map(|(k, v)| (k.into(), v.into())));
                for value in profile.values() {
                    secrets.add(value);
                }
            }
        }
        let mut command = shell(&input.config, &step.shell, &step.run, base.path(), index)?;
        command
            .current_dir(&workspace)
            .env_clear()
            .envs(env)
            // stdin carries worker leases and cancellation, never build input.
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process::spawn(command)?;
        let stdout = child.stdout().take().context("missing step stdout")?;
        let stderr = child.stderr().take().context("missing step stderr")?;
        let out = {
            let emit = emit.clone();
            let name = step.name.clone();
            let secrets = secrets.clone();
            tokio::spawn(async move {
                crate::signing::redact::read_log(stdout, &name, emit, secrets).await
            })
        };
        let err = {
            let emit = emit.clone();
            let name = step.name.clone();
            let secrets = secrets.clone();
            tokio::spawn(async move {
                crate::signing::redact::read_log(stderr, &name, emit, secrets).await
            })
        };
        let result = process::wait(
            &mut *child,
            cancel,
            Duration::from_secs(step.timeout_seconds.into()),
        )
        .await;
        let out_result =
            artifactory_execution_support::process::finish_reader(out, result.is_err()).await;
        let err_result =
            artifactory_execution_support::process::finish_reader(err, result.is_err()).await;
        out_result??;
        err_result??;
        let exit = result.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
        emit.send(Event {
            kind: "step_finished".into(),
            payload: json!({"index":index,"exit_code":exit}),
            ..Default::default()
        })
        .await?;
        source::verify(&input.config, assignment, &workspace, cancel).await?;
        ensure!(
            result?.success(),
            "step {:?} failed: exit {exit}",
            step.name
        );
    }
    if let Some(mut active) = materials.take() {
        active.cleanup(&input.config).await?;
    }
    let paths = artifacts::collect(&workspace, &assignment.artifacts)?;
    let verified = if managed.bindings.is_empty() {
        None
    } else {
        Some(crate::signing::verify_outputs(input, &managed, &workspace, cancel).await?)
    };
    let upload_root = verified.as_ref().map(|v| v.path()).unwrap_or(&workspace);
    if !paths.is_empty() {
        let mut control = client::control(&input.config).await?;
        for path in paths {
            tokio::select! {
                _=cancel.cancelled()=>anyhow::bail!("artifact upload canceled"),
                result=upload(input,&mut control,upload_root,&path)=>result.map_err(|e|Failure::wrap("artifact_transport",e))?,
            }
        }
    }
    Ok(())
}
pub fn safe_script_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn shell(
    cfg: &Config,
    name: &str,
    script: &str,
    directory: &Path,
    index: usize,
) -> Result<Command> {
    let mut command = Command::new(cfg.tool(name));
    match name {
        "bash" => {
            command.args([
                "--noprofile",
                "--norc",
                "-e",
                "-o",
                "pipefail",
                "-c",
                script,
            ]);
        }
        "sh" => {
            command.args(["-e", "-c", script]);
        }
        "pwsh" | "powershell" => {
            let path = directory.join(format!("step-{index}.ps1"));
            private_write(&path,format!("$ErrorActionPreference = 'Stop'\r\n{script}\r\nif ($LASTEXITCODE -and $LASTEXITCODE -ne 0) {{ exit $LASTEXITCODE }}\r\n").as_bytes(),false,true)?;
            command
                .args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ])
                .arg(path);
        }
        _ => anyhow::bail!("unsupported shell"),
    }
    Ok(command)
}
async fn upload(
    input: &Input,
    control: &mut pb::control_client::ControlClient<tonic::transport::Channel>,
    root: &Path,
    path: &str,
) -> Result<()> {
    let file = artifacts::open(root, path)?;
    let mut file = tokio::fs::File::from_std(file);
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        size += count as u64;
        ensure!(size <= 20 << 30, "artifact exceeds 20 GiB");
        hash.update(&buffer[..count]);
    }
    ensure!(size > 0, "artifact must contain at least one byte");
    file.rewind().await?;
    let ticket = control
        .prepare_artifact(pb::ArtifactRequest {
            attempt_id: input.attempt_id.clone(),
            lease_token: input.lease_token.clone(),
            path: path.into(),
            size,
            sha256: hex::encode(hash.finalize()),
        })
        .await?
        .into_inner();
    if !ticket.url.is_empty() {
        let url = source::validate_url(&ticket.url, input.config.allow_http_uploads)?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let response = http
            .put(url)
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(ReaderStream::new(
                file.take(size),
            )))
            .send()
            .await
            .context("artifact upload failed")?;
        ensure!(
            response.status().is_success(),
            "artifact upload rejected: {}",
            response.status()
        );
    }
    control
        .complete_artifact(pb::CompleteArtifactRequest {
            attempt_id: input.attempt_id.clone(),
            lease_token: input.lease_token.clone(),
            upload_id: ticket.upload_id,
        })
        .await?;
    Ok(())
}
pub async fn stdin_worker(cancel: CancellationToken) -> Result<()> {
    run(
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
        cancel,
    )
    .await
}
struct Workspace(std::path::PathBuf);
impl Workspace {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod stack_budget_tests {
    use super::*;
    #[test]
    fn worker_future_fits_native_stack_budget() {
        let input = Input::default();
        let cancel = CancellationToken::new();
        let (emit, _events) = Emitter::channel();
        let execute_future = execute(&input, &cancel, &emit);
        let run_future = run(
            BufReader::new(tokio::io::empty()),
            tokio::io::sink(),
            cancel.clone(),
        );
        // Windows 原生服务的线程栈较小。异步帧不内嵌制品缓冲区。
        for bytes in [
            std::mem::size_of_val(&execute_future),
            std::mem::size_of_val(&run_future),
        ] {
            assert!(
                bytes < 64 * 1024,
                "worker future exceeds the native stack budget: {bytes}"
            );
        }
    }
}
