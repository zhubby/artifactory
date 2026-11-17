//! [INPUT]: 依赖发布专属 Secret、已验证平台结果与 OCI 客户端。
//! [OUTPUT]: 发布完整 OCI Index 或复制到外部仓库。
//! [POS]: 镜像构建与最终标签之间的独立发布进程。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::transport::Control;
use anyhow::{Result, ensure};
use artifactory_image_protocol::{PublishTask, Report, index, sha256};
use tokio_util::sync::CancellationToken;

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
                std::path::Path::new("/workspace"),
            )
            .await?;
        control.report(&Report::Published { digest }).await
    };
    let result = tokio::select! { result=operation=>result, _=cancel.cancelled()=>Err(anyhow::anyhow!("publication canceled")) };
    if result.is_err() {
        let _ = control
            .report(&Report::Failed {
                message: "image publication failed; retry publication without rebuilding".into(),
            })
            .await;
    }
    cancel.cancel();
    heartbeat.abort();
    result
}
