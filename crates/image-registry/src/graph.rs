//! [INPUT]: 依赖认证 OCI 请求、Manifest 描述符与配置 Blob。
//! [OUTPUT]: 验证镜像真实架构并复制摘要引用的完整镜像图。
//! [POS]: 构建报告与镜像发布之间的内容校验边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::Registry;
use anyhow::{Context, Result, ensure};
use artifactory_image_protocol::{
    INDEX_MEDIA_TYPE, MANIFEST_MEDIA_TYPE, PlatformResult, digest, sha256,
};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

impl Registry {
    pub async fn verify_platform(
        &self,
        repo: &str,
        id: &str,
        platform: &str,
    ) -> Result<PlatformResult> {
        let mut bytes = self.manifest(repo, id).await?;
        let mut value: Value = serde_json::from_slice(&bytes)?;
        if let Some(manifests) = value["manifests"].as_array() {
            ensure!(
                value["schemaVersion"] == 2 && value["mediaType"] == INDEX_MEDIA_TYPE,
                "expected OCI image index"
            );
            let (os, arch) = platform.split_once('/').context("invalid platform")?;
            let matches: Vec<_> = manifests
                .iter()
                .filter(|m| m["platform"]["os"] == os && m["platform"]["architecture"] == arch)
                .collect();
            ensure!(matches.len() == 1, "platform manifest missing or ambiguous");
            let id = matches[0]["digest"]
                .as_str()
                .context("manifest digest missing")?;
            bytes = self.manifest(repo, id).await?;
            ensure!(
                matches[0]["size"].as_u64() == Some(bytes.len() as u64),
                "platform descriptor size mismatch"
            );
            value = serde_json::from_slice(&bytes)?;
        }
        ensure!(
            value["schemaVersion"] == 2 && value["mediaType"] == MANIFEST_MEDIA_TYPE,
            "expected OCI image manifest"
        );
        let config = value["config"]["digest"]
            .as_str()
            .context("configuration digest missing")?;
        let config_bytes = self.small_blob(repo, config).await?;
        ensure!(
            value["config"]["size"].as_u64() == Some(config_bytes.len() as u64),
            "configuration descriptor size mismatch"
        );
        let config: Value = serde_json::from_slice(&config_bytes)?;
        let actual = format!(
            "{}/{}",
            config["os"].as_str().unwrap_or(""),
            config["architecture"].as_str().unwrap_or("")
        );
        ensure!(
            actual == platform,
            "image configuration architecture does not match requested platform"
        );
        let result = PlatformResult {
            platform: platform.into(),
            digest: sha256(&bytes),
            size: bytes.len() as u64,
            media_type: MANIFEST_MEDIA_TYPE.into(),
        };
        result.validate()?;
        Ok(result)
    }
    pub async fn copy(
        &self,
        source: &Registry,
        source_repo: &str,
        destination_repo: &str,
        root: &str,
        tags: &[String],
        temp: &Path,
    ) -> Result<()> {
        ensure!(digest(root), "copy requires an immutable digest");
        let mut pending = vec![(root.to_owned(), 0usize, false, None)];
        let mut seen = BTreeSet::new();
        while let Some((id, depth, ready, expected_size)) = pending.pop() {
            ensure!(depth <= 8 && seen.len() < 4096, "image graph exceeds limit");
            if !ready && !seen.insert(id.clone()) {
                continue;
            }
            let bytes = source.manifest(source_repo, &id).await?;
            ensure!(
                expected_size.is_none_or(|size| size == bytes.len() as u64),
                "manifest descriptor size mismatch"
            );
            let value: Value = serde_json::from_slice(&bytes)?;
            ensure!(value["schemaVersion"] == 2, "unsupported manifest version");
            if let Some(children) = value["manifests"].as_array() {
                ensure!(
                    value["mediaType"] == INDEX_MEDIA_TYPE,
                    "unsupported image index type"
                );
                ensure!(children.len() <= 256, "oversized image index");
                if !ready {
                    pending.push((id.clone(), depth, true, expected_size));
                    for child in children {
                        let id = child["digest"].as_str().context("child digest missing")?;
                        ensure!(digest(id), "invalid child digest");
                        let size = child["size"].as_u64().context("child size missing")?;
                        ensure!(size > 0 && size <= 4 * 1024 * 1024, "invalid child size");
                        pending.push((id.into(), depth + 1, false, Some(size)));
                    }
                    continue;
                }
            } else {
                ensure!(
                    value["mediaType"] == MANIFEST_MEDIA_TYPE
                        && value["config"].is_object()
                        && value["layers"].is_array(),
                    "invalid image manifest"
                );
                let layers = value["layers"].as_array().unwrap();
                ensure!(layers.len() <= 1024, "too many image layers");
                for blob in std::iter::once(&value["config"]).chain(layers.iter()) {
                    self.copy_blob(
                        source,
                        source_repo,
                        destination_repo,
                        blob["digest"].as_str().context("blob digest missing")?,
                        blob["size"].as_u64().context("blob size missing")?,
                        temp,
                    )
                    .await?;
                }
            }
            self.put_manifest(destination_repo, &id, &bytes).await?;
        }
        let bytes = source.manifest(source_repo, root).await?;
        for tag in tags {
            self.put_manifest(destination_repo, tag, &bytes).await?;
        }
        Ok(())
    }
}
