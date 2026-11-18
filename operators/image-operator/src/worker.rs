//! [INPUT]: 依赖清单身份与推送 Secret、init 清单、只读源码和 BuildKit。
//! [OUTPUT]: 构建单平台镜像，信任私有认证 CA 并提交限长脱敏日志。
//! [POS]: BuildKit Job 的隔离执行进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::transport::Control;
use anyhow::{Context, Result, ensure};
use artifactory_emissary::{
    config::{private_dir, private_write},
    process,
    signing::redact,
    source,
    worker::Emitter,
};
use artifactory_image_protocol::{BuildTask, Report};
use std::{path::Path, process::Stdio, time::Duration};
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

fn log_secrets(task: &BuildTask) -> redact::Secrets {
    use base64::Engine;
    let mut secrets = redact::Secrets::default();
    secrets.add(&task.token);
    secrets.add(&task.registry.password);
    secrets.add(&base64::engine::general_purpose::STANDARD.encode(format!(
        "{}:{}",
        task.registry.username, task.registry.password
    )));
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
) -> Result<()> {
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
        let mut sequence = 0;
        while let Some(event) = events.recv().await {
            for text in log_chunks(&event.data) {
                sequence += 1;
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
            artifactory_image_registry::Registry::new(task.registry.clone(), task.insecure)?;
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
        for (name, value) in &task.image.build_args {
            command
                .arg("--opt")
                .arg(format!("build-arg:{name}={value}"));
        }
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
        out.await??;
        err.await??;
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
        control.report(&Report::Platform { result }).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    drop(emit);
    let log_result = drain.await?;
    let _ = tokio::fs::remove_dir_all(&root).await;
    result?;
    log_result?;
    Ok(())
}

pub async fn run() -> Result<()> {
    let path = std::env::var("ARTIFACTORY_IMAGE_TASK")?;
    let bytes = tokio::fs::read(path).await?;
    ensure!(bytes.len() <= 1024 * 1024, "oversized task Secret");
    let task: BuildTask = serde_json::from_slice(&bytes)?;
    let control = Control::new(task.control_url.clone(), task.token.clone(), task.insecure)?;
    let platform = std::env::var("ARTIFACTORY_IMAGE_PLATFORM")?;
    let cancel = CancellationToken::new();
    let heartbeat = control.heartbeat(platform.clone(), cancel.clone());
    let result = build(&task, &platform, &control, &cancel).await;
    if result.is_err() {
        let _ = control
            .report(&Report::Failed {
                message: "image build failed; inspect platform logs".into(),
            })
            .await;
    }
    cancel.cancel();
    heartbeat.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
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
            "source_manifest_id":"00000000-0000-0000-0000-000000000001","source_digest":"a".repeat(64), "timeout_seconds":300, "docker_auth":{"auths":{"external.example":{"auth":"dXNlcjpleHRlcm5hbC1wYXNzd29yZA=="}}},
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
