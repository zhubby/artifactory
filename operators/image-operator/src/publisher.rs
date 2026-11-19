//! [INPUT]: 依赖发布专属 Secret、池代理、OCI 客户端和临时卷。
//! [OUTPUT]: 发布完整 OCI Index，失败时保存脱敏日志和摘要。
//! [POS]: 镜像构建与最终标签之间的独立发布进程，不写源码卷。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::transport::Control;
use anyhow::{Result, ensure};
use artifactory_emissary::signing::redact;
use artifactory_image_protocol::{PublishTask, Report, index, sha256};
use std::sync::atomic::AtomicI64;
use tokio_util::sync::CancellationToken;

/// Blob 校验的临时目录必须落在 Pod 的可写磁盘卷。
pub(crate) const TEMPORARY_DIRECTORY: &str = "/work";

pub async fn run() -> Result<()> {
    let bytes = tokio::fs::read(std::env::var("ARTIFACTORY_IMAGE_TASK")?).await?;
    ensure!(bytes.len() <= 1024 * 1024, "oversized publication Secret");
    let task: PublishTask = serde_json::from_slice(&bytes)?;
    let control = Control::new(task.control_url.clone(), task.token.clone(), task.insecure)?;
    let cancel = CancellationToken::new();
    let heartbeat = control.heartbeat("publish".into(), cancel.clone());
    let operation = async {
        let results = control.publication().await?;
        let source =
            artifactory_image_registry::Registry::new(task.registry.clone(), task.insecure)?;
        let digest = if let Some(digest) = &task.root_digest {
            ensure!(
                artifactory_image_protocol::digest(digest),
                "invalid publication digest"
            );
            digest.clone()
        } else {
            let bytes = index(&task.platforms, &results)?;
            let digest = sha256(&bytes);
            source
                .put_manifest(&task.source_repository, &digest, &bytes)
                .await?;
            digest
        };
        let destination = if let Some(login) = &task.destination {
            artifactory_image_registry::Registry::new(login.clone(), task.insecure)?
        } else {
            source.clone()
        };
        destination
            .copy(
                &source,
                &task.source_repository,
                &task.destination_repository,
                &digest,
                &task.tags,
                std::path::Path::new(TEMPORARY_DIRECTORY),
            )
            .await?;
        control.report(&Report::Published { digest }).await
    };
    let mut result = tokio::select! { result=operation=>result, _=cancel.cancelled()=>Err(anyhow::anyhow!("publication canceled")) };
    if let Err(error) = &result {
        let mut secrets = redact::Secrets::default();
        crate::diagnostics::proxy_secrets(&mut secrets);
        secrets.add(&task.token);
        crate::diagnostics::registry_secrets(&mut secrets, &task.registry);
        if let Some(destination) = &task.destination {
            crate::diagnostics::registry_secrets(&mut secrets, destination);
        }
        let message = control
            .failure("publish", error, secrets, &AtomicI64::new(0))
            .await;
        result = Err(anyhow::anyhow!(message));
    }
    cancel.cancel();
    heartbeat.abort();
    result
}
