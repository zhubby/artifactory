//! ```text
//! [INPUT]: 依赖清单身份、共享执行、源码检出、OCI 客户端和 BuildKit。
//! [OUTPUT]: 注入固定 Release 版本并读回 OCI 元数据，收尾后提交候选。
//! [POS]: BuildKit Pod 的隔离执行进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use crate::{lifecycle, transport::Control};
use anyhow::{Context, Result, ensure};
use artifactory_execution_support::{events::Emitter, fs::private_write, process, redact};
use artifactory_image_protocol::{BuildTask, PlatformResult, Report};
use artifactory_source_checkout::{self as source, private_dir};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

fn log_chunks(data: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(data);
    let mut rest = text.as_ref();
    let mut chunks = Vec::new();
    while !rest.is_empty() {
        let mut end = rest.len().min(16384);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    chunks
}

async fn finish_log<T>(mut task: tokio::task::JoinHandle<Result<T>>, limit: Duration) -> Result<T> {
    match tokio::time::timeout(limit, &mut task).await {
        Ok(result) => result?,
        Err(_) => {
            task.abort();
            let _ = task.await;
            anyhow::bail!("build log drain timed out")
        }
    }
}

async fn diagnostic_chunks(
    mut diagnostic: lifecycle::Diagnostic,
    secrets: redact::Secrets,
) -> Result<Vec<String>> {
    if diagnostic.truncated {
        // 环形尾部能切断密钥。丢弃最长期望密钥之前缀窗口，不输出截断密钥的后缀。
        let skip = secrets
            .0
            .iter()
            .map(|value| value.len())
            .max()
            .unwrap_or(0)
            .saturating_sub(1)
            .min(diagnostic.bytes.len());
        diagnostic.bytes.drain(..skip);
    }
    let (emit, mut events) = Emitter::channel();
    redact::read_log(&diagnostic.bytes[..], "buildkit-daemon", emit, secrets).await?;
    let mut text = String::from(if diagnostic.channel == 0 {
        "[BuildKit daemon stdout]\n"
    } else {
        "[BuildKit daemon stderr]\n"
    });
    while let Some(event) = events.recv().await {
        text.push_str(&String::from_utf8_lossy(&event.data));
    }
    Ok(log_chunks(text.as_bytes()))
}
async fn report_diagnostics(
    control: &Control,
    platform: &str,
    task: &BuildTask,
    diagnostics: &lifecycle::Diagnostics,
    sequence: &AtomicI64,
) -> Result<()> {
    for diagnostic in diagnostics.snapshot()? {
        for text in diagnostic_chunks(diagnostic, log_secrets(task)).await? {
            control
                .report(&Report::Log {
                    platform: platform.to_owned(),
                    sequence: sequence.fetch_add(1, Ordering::Relaxed) + 1,
                    text,
                })
                .await?;
        }
    }
    Ok(())
}

fn buildctl(docker: &Path, registry: &str, ca: Option<&Path>) -> Command {
    let mut command = Command::new("buildctl");
    command
        .env_clear()
        .envs(source::build_environment())
        .env("DOCKER_CONFIG", docker);
    command.args(["--addr", "unix:///run/buildkit/buildkitd.sock", "build"]);
    if let Some(ca) = ca {
        // Buildctl 独立获取仓库令牌；BuildKit daemon 的 CA 不覆盖此连接。
        command
            .arg("--registry-auth-tlscontext")
            .arg(format!("host={registry},ca={}", ca.display()));
    }
    command
}

fn build_arguments(
    command: &mut Command,
    recipe: &BTreeMap<String, String>,
    proxy_environment: Option<&BTreeMap<OsString, OsString>>,
) -> Result<()> {
    let mut arguments = recipe.clone();
    if let Some(environment) = proxy_environment {
        // 标准代理参数由 Dockerfile frontend 特殊处理，默认不写入镜像历史或缓存键。
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            if let Some(value) = environment.get(std::ffi::OsStr::new(name)) {
                let value = value
                    .to_str()
                    .context("pool proxy environment must be UTF-8")?;
                if !value.is_empty() {
                    arguments.insert(name.into(), value.into());
                }
            }
        }
    }
    for (name, value) in arguments {
        command
            .arg("--opt")
            .arg(format!("build-arg:{name}={value}"));
    }
    Ok(())
}

fn log_secrets(task: &BuildTask) -> redact::Secrets {
    use base64::Engine;
    let mut secrets = redact::Secrets::default();
    crate::diagnostics::proxy_secrets(&mut secrets);
    secrets.add(&task.token);
    crate::diagnostics::registry_secrets(&mut secrets, &task.registry);
    if let Some(auths) = task.docker_auth["auths"].as_object() {
        for auth in auths.values() {
            for key in ["password", "identitytoken", "registrytoken"] {
                if let Some(value) = auth[key].as_str() {
                    secrets.add(value);
                }
            }
            if let Some(encoded) = auth["auth"].as_str() {
                secrets.add(encoded);
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded)
                    && let Ok(value) = String::from_utf8(bytes)
                {
                    secrets.add(&value);
                    if let Some((_, password)) = value.split_once(':') {
                        secrets.add(password);
                    }
                }
            }
        }
    }
    for value in task.secrets.values() {
        secrets.add(value);
    }
    secrets
}

fn prepared_source(
    id: uuid::Uuid,
    digest: &str,
    bytes: &[u8],
) -> Result<artifactory_source_checkout::SourceManifest> {
    let prepared: artifactory_source_checkout::SourceManifest = serde_json::from_slice(bytes)?;
    ensure!(
        prepared.id == id && prepared.digest == digest,
        "prepared source differs from authorized manifest"
    );
    prepared.validate()?;
    Ok(prepared)
}

async fn build(
    task: &BuildTask,
    platform: &str,
    control: &Control,
    cancel: &CancellationToken,
    log_sequence: Arc<AtomicI64>,
) -> Result<PlatformResult> {
    task.image.validate()?;
    ensure!(
        task.image.platforms.iter().any(|p| p == platform),
        "platform is not authorized"
    );
    let pod = uuid::Uuid::parse_str(&std::env::var("POD_UID")?)?;
    let root = Path::new("/work").join(pod.to_string());
    std::fs::create_dir(&root).context("isolated workspace already exists")?;
    let workspace = Path::new("/workspace/source");
    let prepared = prepared_source(
        task.source_manifest_id,
        &task.source_digest,
        &tokio::fs::read("/workspace/source-manifest.json").await?,
    )?;
    let (emit, mut events) = Emitter::channel();
    let logs = control.clone();
    let log_platform = platform.to_owned();
    let log_cancel = cancel.clone();
    let drain = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            for text in log_chunks(&event.data) {
                let sequence = log_sequence.fetch_add(1, Ordering::Relaxed) + 1;
                if let Err(error) = logs
                    .report(&Report::Log {
                        platform: log_platform.clone(),
                        sequence,
                        text,
                    })
                    .await
                {
                    log_cancel.cancel();
                    return Err(error);
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    let result = async {
        artifactory_source_checkout::verify(&prepared, workspace, cancel).await?;
        let context = artifactory_source_checkout::managed_path(
            workspace,
            if task.image.context == "." {
                ""
            } else {
                &task.image.context
            },
        )?;
        let dockerfile =
            artifactory_source_checkout::managed_path(workspace, &task.image.dockerfile)?;
        ensure!(
            context.is_dir() && dockerfile.is_file(),
            "build context or Dockerfile missing"
        );
        let docker = root.join("docker");
        private_dir(&docker)?;
        let mut auth = task.docker_auth.clone();
        if !auth.is_object() {
            auth = serde_json::json!({"auths":{}});
        }
        let registry =
            artifactory_oci_registry_client::Registry::new(task.registry.clone(), task.insecure)?;
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(format!(
            "{}:{}",
            task.registry.username, task.registry.password
        ));
        auth["auths"][registry.host()] = serde_json::json!({"auth":encoded});
        private_write(
            &docker.join("config.json"),
            &serde_json::to_vec(&auth)?,
            false,
            true,
        )?;
        let metadata = root.join("metadata.json");
        let arch = platform
            .strip_prefix("linux/")
            .context("invalid platform")?;
        let ca = std::env::var_os("ARTIFACTORY_SERVICE_CA").map(std::path::PathBuf::from);
        let mut command = buildctl(&docker, registry.host(), ca.as_deref());
        command
            .args([
                "--progress",
                "plain",
                "--frontend",
                "dockerfile.v0",
                "--local",
            ])
            .arg(format!("context={}", context.display()))
            .arg("--local")
            .arg(format!(
                "dockerfile={}",
                dockerfile.parent().unwrap().display()
            ))
            .arg("--opt")
            .arg(format!(
                "filename={}",
                dockerfile.file_name().unwrap().to_string_lossy()
            ))
            .arg("--opt")
            .arg(format!("platform={platform}"))
            .arg("--metadata-file")
            .arg(&metadata)
            .arg("--output")
            .arg(format!(
                "type=image,name={}/{}:{arch},push=true,oci-mediatypes=true{}",
                registry.host(),
                task.repository,
                if task.insecure {
                    ",registry.insecure=true"
                } else {
                    ""
                }
            ));
        let proxy_environment = (std::env::var("ARTIFACTORY_POOL_PROXY").as_deref() == Ok("1"))
            .then(source::build_environment);
        build_arguments(
            &mut command,
            &task.image.build_args,
            proxy_environment.as_ref(),
        )?;
        release_arguments(&mut command, task)?;
        let secrets = log_secrets(task);
        for (name, value) in &task.secrets {
            let path = root.join(format!("secret-{name}"));
            private_write(&path, value.as_bytes(), false, true)?;
            command
                .arg("--secret")
                .arg(format!("id={name},src={}", path.display()));
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process::spawn(command)?;
        let stdout = child.stdout().take().context("build stdout missing")?;
        let stderr = child.stderr().take().context("build stderr missing")?;
        let out = tokio::spawn(redact::read_log(
            stdout,
            "buildkit",
            emit.clone(),
            secrets.clone(),
        ));
        let err = tokio::spawn(redact::read_log(stderr, "buildkit", emit.clone(), secrets));
        let result = process::wait(
            &mut *child,
            cancel,
            Duration::from_secs(u64::from(task.timeout_seconds)),
        )
        .await;
        finish_log(out, Duration::from_secs(5)).await?;
        finish_log(err, Duration::from_secs(5)).await?;
        ensure!(result?.success(), "BuildKit failed; inspect build logs");
        artifactory_source_checkout::verify(&prepared, workspace, cancel).await?;
        let bytes = tokio::fs::read(&metadata).await?;
        ensure!(bytes.len() <= 65536, "oversized BuildKit metadata");
        let metadata: serde_json::Value = serde_json::from_slice(&bytes)?;
        let digest = metadata["containerimage.digest"]
            .as_str()
            .context("BuildKit result digest missing")?;
        let result = registry
            .verify_platform(&task.repository, digest, platform)
            .await?;
        registry
            .verify_release_metadata(
                &task.repository,
                &result.digest,
                platform,
                &task.release_version,
                &task.source_revision,
            )
            .await?;
        Ok::<_, anyhow::Error>(result)
    }
    .await;
    drop(emit);
    let log_result = finish_log(drain, Duration::from_secs(20)).await;
    let _ = tokio::fs::remove_dir_all(&root).await;
    let result = result?;
    log_result?;
    Ok(result)
}

fn release_arguments(command: &mut Command, task: &BuildTask) -> Result<()> {
    ensure!(
        !task.release_id.is_nil()
            && !task.release_version.is_empty()
            && [40, 64].contains(&task.source_revision.len())
            && task.source_revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid fixed Release identity"
    );
    ensure!(
        !task
            .image
            .build_args
            .keys()
            .any(|key| key.starts_with("ARTIFACTORY_RELEASE_")),
        "Release build arguments are reserved"
    );
    for (name, value) in [
        (
            "build-arg:ARTIFACTORY_RELEASE_ID",
            task.release_id.to_string(),
        ),
        (
            "build-arg:ARTIFACTORY_RELEASE_VERSION",
            task.release_version.clone(),
        ),
        (
            "label:org.opencontainers.image.version",
            task.release_version.clone(),
        ),
        (
            "label:org.opencontainers.image.revision",
            task.source_revision.clone(),
        ),
    ] {
        command.arg("--opt").arg(format!("{name}={value}"));
    }
    Ok(())
}

pub async fn run() -> Result<()> {
    let path = std::env::var("ARTIFACTORY_IMAGE_TASK")?;
    let bytes = tokio::fs::read(path).await?;
    ensure!(bytes.len() <= 1024 * 1024, "oversized task Secret");
    let task: BuildTask = serde_json::from_slice(&bytes)?;
    let control = Control::new(task.control_url.clone(), task.token.clone(), task.insecure)?;
    let platform = std::env::var("ARTIFACTORY_IMAGE_PLATFORM")?;
    let seconds: u64 = std::env::var("ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS")?.parse()?;
    ensure!(
        (1..=604800).contains(&seconds),
        "invalid BuildKit task deadline"
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let cancel = CancellationToken::new();
    let heartbeat = control.heartbeat(platform.clone(), cancel.clone());
    let diagnostics = lifecycle::Diagnostics::default();
    let log_sequence = Arc::new(AtomicI64::new(0));
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut result = async {
        let client = tokio::select! {
            result = lifecycle::Client::connect_recording(Path::new(lifecycle::SOCKET), deadline, cancel.clone(), diagnostics.clone()) => result?,
            _ = term.recv() => { cancel.cancel(); anyhow::bail!("image execution interrupted before BuildKit readiness"); },
            _ = interrupt.recv() => { cancel.cancel(); anyhow::bail!("image execution interrupted before BuildKit readiness"); },
        };
        let mut pending = Box::pin(build(&task, &platform, &control, &cancel, log_sequence.clone()));
        let (built, finished) = tokio::select! {
            result = &mut pending => (result, true),
            _ = tokio::time::sleep_until(deadline) => { cancel.cancel(); (Err(anyhow::anyhow!("image task deadline exceeded")), false) },
            _ = cancel.cancelled() => (Err(anyhow::anyhow!("image execution canceled")), false),
            _ = term.recv() => { cancel.cancel(); (Err(anyhow::anyhow!("image execution interrupted")), false) },
            _ = interrupt.recv() => { cancel.cancel(); (Err(anyhow::anyhow!("image execution interrupted")), false) },
        };
        if !finished {
            let _ = tokio::time::timeout(Duration::from_secs(5), &mut pending).await;
        }
        drop(pending);
        let stopped = client.stop().await;
        let result = built?;
        stopped?;
        ensure!(!cancel.is_cancelled(), "image execution canceled before result report");
        tokio::time::timeout_at(deadline, control.report(&Report::Platform { result })).await
            .context("platform result acknowledgement timed out")??;
        Ok(())
    }.await;
    if let Err(error) = &result {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            report_diagnostics(&control, &platform, &task, &diagnostics, &log_sequence),
        )
        .await;
        let message = control
            .failure(&platform, error, log_secrets(&task), &log_sequence)
            .await;
        // main 的错误输出也必须使用脱敏摘要。
        result = Err(anyhow::anyhow!(message));
    }
    cancel.cancel();
    heartbeat.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_arguments_are_fixed_and_oci_labels_follow_the_pinned_source() {
        let mut task: BuildTask = serde_json::from_value(serde_json::json!({
            "execution_id":"fixture","token":"test-only","control_url":"https://control.example.com",
            "registry":{"endpoint":"https://registry.example.com","username":"fixture","password":"test-only"},
            "repository":"candidate/fixture","image":{"name":"app"},
            "source_manifest_id":"00000000-0000-0000-0000-000000000001","source_digest":"a".repeat(64),
            "release_id":"00000000-0000-0000-0000-000000000002","release_version":"1.2.3-rc.1",
            "source_revision":"b".repeat(40),"timeout_seconds":300,"insecure":false
        })).unwrap();
        let mut command = Command::new("buildctl");
        release_arguments(&mut command, &task).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"build-arg:ARTIFACTORY_RELEASE_VERSION=1.2.3-rc.1".into()));
        assert!(args.contains(&"label:org.opencontainers.image.version=1.2.3-rc.1".into()));
        assert!(args.contains(&format!(
            "label:org.opencontainers.image.revision={}",
            "b".repeat(40)
        )));
        task.image
            .build_args
            .insert("ARTIFACTORY_RELEASE_VERSION".into(), "9.0.0".into());
        assert!(release_arguments(&mut Command::new("buildctl"), &task).is_err());
    }
    #[test]
    fn pool_proxy_build_arguments_are_present_only_when_configured_and_nonempty() {
        let recipe = BTreeMap::from([
            ("HTTP_PROXY".into(), "http://recipe-proxy:8080".into()),
            ("RELEASE".into(), "1.2.3".into()),
        ]);
        let proxy = BTreeMap::from([
            (
                OsString::from("HTTP_PROXY"),
                OsString::from("http://pool-proxy:18443"),
            ),
            (
                OsString::from("http_proxy"),
                OsString::from("http://pool-proxy:18443"),
            ),
            (
                OsString::from("HTTPS_PROXY"),
                OsString::from("http://pool-proxy:18443"),
            ),
            (
                OsString::from("NO_PROXY"),
                OsString::from("control,registry,.svc"),
            ),
            (
                OsString::from("no_proxy"),
                OsString::from("control,registry,.svc"),
            ),
        ]);
        let arguments = |environment| {
            let mut command = Command::new("buildctl");
            build_arguments(&mut command, &recipe, environment).unwrap();
            command
                .as_std()
                .get_args()
                .map(|argument| argument.to_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        let without = arguments(None);
        assert!(without.contains(&"build-arg:HTTP_PROXY=http://recipe-proxy:8080".to_owned()));
        assert!(
            !without
                .iter()
                .any(|argument| argument.contains("pool-proxy"))
        );
        let with = arguments(Some(&proxy));
        for expected in [
            "build-arg:HTTP_PROXY=http://pool-proxy:18443",
            "build-arg:http_proxy=http://pool-proxy:18443",
            "build-arg:HTTPS_PROXY=http://pool-proxy:18443",
            "build-arg:NO_PROXY=control,registry,.svc",
            "build-arg:no_proxy=control,registry,.svc",
            "build-arg:RELEASE=1.2.3",
        ] {
            assert!(with.contains(&expected.to_owned()), "missing {expected}");
        }
        assert!(
            !with
                .iter()
                .any(|argument| argument.contains("recipe-proxy"))
        );
        let empty = BTreeMap::from([(OsString::from("HTTP_PROXY"), OsString::new())]);
        assert_eq!(
            arguments(Some(&empty)),
            without,
            "empty pool values do not erase recipe arguments"
        );
    }
    #[tokio::test]
    async fn daemon_diagnostics_redact_secrets_and_hide_cut_secret_suffixes() {
        let mut secrets = redact::Secrets::default();
        secrets.add("private-password");
        let text = diagnostic_chunks(
            lifecycle::Diagnostic {
                channel: 1,
                truncated: false,
                bytes: b"authentication failed for private-password".to_vec(),
            },
            secrets.clone(),
        )
        .await
        .unwrap()
        .concat();
        assert!(text.contains("authentication failed for [REDACTED]"));
        assert!(!text.contains("private-password"));
        let text = diagnostic_chunks(
            lifecycle::Diagnostic {
                channel: 1,
                truncated: true,
                bytes: b"password and a later safe failure".to_vec(),
            },
            secrets,
        )
        .await
        .unwrap()
        .concat();
        assert!(!text.contains("password"));
        assert!(text.contains("safe failure"));
    }
    #[test]
    fn prepared_manifest_requires_authorized_identity_and_valid_contents() {
        use artifactory_source_checkout::{SourceManifest, SourceRepository};
        let manifest = SourceManifest::new(
            uuid::Uuid::from_u128(1),
            vec![SourceRepository {
                path: String::new(),
                upstream_url: "https://example.test/org/repo.git".into(),
                commit_sha: "a".repeat(40),
                tags: Default::default(),
                lfs_objects: vec![],
            }],
        )
        .unwrap();
        let bytes = serde_json::to_vec(&manifest).unwrap();
        assert_eq!(
            prepared_source(manifest.id, &manifest.digest, &bytes).unwrap(),
            manifest
        );
        assert!(prepared_source(uuid::Uuid::from_u128(2), &manifest.digest, &bytes).is_err());
        assert!(prepared_source(manifest.id, &"b".repeat(64), &bytes).is_err());
        let mut tampered = manifest.clone();
        tampered.repositories[0].commit_sha = "c".repeat(40);
        assert!(
            prepared_source(
                manifest.id,
                &manifest.digest,
                &serde_json::to_vec(&tampered).unwrap(),
            )
            .is_err(),
            "matching claimed identity cannot hide changed manifest contents"
        );
    }

    #[test]
    fn buildctl_token_provider_trusts_private_ca_without_disabling_tls() {
        let command = buildctl(
            Path::new("/private/docker"),
            "registry.internal:5443",
            Some(Path::new("/run/ca/ca.crt")),
        );
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert!(args.windows(2).any(|a| a
            == [
                "--registry-auth-tlscontext",
                "host=registry.internal:5443,ca=/run/ca/ca.crt"
            ]));
        assert!(!args.iter().any(|arg| arg.contains("insecure")));
        assert_eq!(
            command
                .as_std()
                .get_envs()
                .find(|(name, _)| *name == "DOCKER_CONFIG")
                .unwrap()
                .1
                .unwrap(),
            "/private/docker"
        );
        assert_eq!(
            buildctl(Path::new("/docker"), "public.example", None)
                .as_std()
                .get_args()
                .count(),
            3
        );
    }
    #[test]
    fn logs_fit_report_limits_after_utf8_conversion() {
        for bytes in [
            format!("{}中文日志", "a".repeat(16383)).into_bytes(),
            vec![0xff; 16384],
        ] {
            let chunks = log_chunks(&bytes);
            assert!(
                chunks.iter().all(|text| text.len() <= 16384),
                "every report must fit the API byte limit"
            );
            assert_eq!(chunks.concat(), String::from_utf8_lossy(&bytes));
        }
    }
    #[tokio::test]
    async fn external_pull_passwords_and_encoded_auth_are_redacted() {
        let task: BuildTask = serde_json::from_value(serde_json::json!({
            "execution_id":"test", "token":"task-token", "control_url":"https://control.example/",
            "registry":{"endpoint":"https://registry.example/","username":"runner","password":"internal-password"},
            "repository":"candidate", "image":{"name":"app"},
            "source_manifest_id":"00000000-0000-0000-0000-000000000001","source_digest":"a".repeat(64), "release_id":"00000000-0000-0000-0000-000000000002", "release_version":"1.2.3", "source_revision":"a".repeat(40), "timeout_seconds":300, "docker_auth":{"auths":{"external.example":{"auth":"dXNlcjpleHRlcm5hbC1wYXNzd29yZA=="}}},
            "insecure":false
        })).unwrap();
        let (emit, mut events) = Emitter::channel();
        redact::read_log(
            &b"external-password dXNlcjpleHRlcm5hbC1wYXNzd29yZA== task-token"[..],
            "test",
            emit,
            log_secrets(&task),
        )
        .await
        .unwrap();
        let mut bytes = Vec::new();
        while let Some(event) = events.recv().await {
            bytes.extend(event.data);
        }
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "[REDACTED] [REDACTED] [REDACTED]"
        );
    }
}
