//! [INPUT]: 依赖完整固定源码清单、Assignment、凭据引用与执行令牌。
//! [OUTPUT]: 校验清单与 commit，只向任务材料交付清单 ID 和摘要。
//! [POS]: 镜像 Secret 仅携带固定源码引用与执行凭据。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Config;
use crate::{crypto, state::AppState, store::images::ImageExecution};
use anyhow::{Result, ensure};
use artifactory_image_protocol::{BuildTask, PublishTask, RegistryLogin};
use serde_json::{Value, json};
use uuid::Uuid;

fn fixed_source_identity(
    source: &artifactory_source_checkout::SourceManifest,
    expected_id: Uuid,
    expected_commit: &str,
) -> Result<(Uuid, String)> {
    ensure!(source.id == expected_id, "source manifest identity changed");
    ensure!(
        source.root()?.commit_sha == expected_commit,
        "source commit mismatch"
    );
    Ok((source.id, source.digest.clone()))
}

pub async fn credential(
    state: &AppState,
    e: &ImageExecution,
    id: Uuid,
    kind: &str,
) -> Result<Vec<u8>> {
    let envelope = state.store.image_credential(e.project_id, id, kind).await?;
    crypto::open_context(
        &state.settings.master_key,
        &envelope,
        format!("image-credential/{}/{id}", e.project_id).as_bytes(),
    )
}
pub async fn publication(
    state: &AppState,
    config: &Config,
    e: &ImageExecution,
    token: &str,
) -> Result<PublishTask> {
    let destination = if let Some(id) = e.destination_credential {
        let login: RegistryLogin =
            serde_json::from_slice(&credential(state, e, id, "registry").await?)?;
        login.validate(state.settings.insecure_dev)?;
        ensure!(
            e.destination_registry.as_deref()
                == Some(
                    url::Url::parse(&login.endpoint)?
                        .origin()
                        .ascii_serialization()
                        .as_str()
                ),
            "publication registry origin changed"
        );
        Some(login)
    } else {
        None
    };
    Ok(PublishTask {
        execution_id: e.id.to_string(),
        token: token.into(),
        control_url: format!(
            "{}/api/v1/image-tasks/{}",
            config.control_url.trim_end_matches('/'),
            e.id
        ),
        registry: config.login(e.id, token),
        source_repository: e.candidate_repository.clone(),
        destination_repository: e.destination_repository.clone(),
        tags: e.tags.clone(),
        platforms: e.spec.platforms.clone(),
        insecure: state.settings.insecure_dev,
        destination,
        root_digest: e.root_digest.clone(),
    })
}
pub async fn build(
    state: &AppState,
    config: &Config,
    e: &ImageExecution,
    token: &str,
) -> Result<BuildTask> {
    let context = state.store.assignment_context(e.job_id).await?;
    ensure!(
        context.source_manifest_id == Some(e.source_manifest_id)
            && context.plan_revision_id == e.plan_revision_id,
        "source identity changed"
    );
    let source = state.store.source_manifest(e.source_manifest_id).await?;
    let mut docker_auth = json!({"auths":{}});
    use base64::Engine;
    for id in &e.spec.pull_credentials {
        let login: RegistryLogin =
            serde_json::from_slice(&credential(state, e, Uuid::parse_str(id)?, "registry").await?)?;
        login.validate(state.settings.insecure_dev)?;
        let origin = url::Url::parse(&login.endpoint)?;
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", login.username, login.password));
        docker_auth["auths"][origin.authority()] = json!({"auth":auth});
    }
    let mut secrets = std::collections::BTreeMap::new();
    for (name, id) in &e.spec.secrets {
        secrets.insert(
            name.clone(),
            String::from_utf8(credential(state, e, Uuid::parse_str(id)?, "secret").await?)?,
        );
    }
    let (source_manifest_id, source_digest) =
        fixed_source_identity(&source, e.source_manifest_id, &context.commit_sha)?;
    let task = BuildTask {
        execution_id: e.id.to_string(),
        token: token.into(),
        control_url: format!(
            "{}/api/v1/image-tasks/{}",
            config.control_url.trim_end_matches('/'),
            e.id
        ),
        registry: config.login(e.id, token),
        repository: e.candidate_repository.clone(),
        image: e.spec.clone(),
        source_manifest_id,
        source_digest,
        timeout_seconds: e.task_timeout_seconds.saturating_sub(600).max(1),
        docker_auth,
        secrets,
        insecure: state.settings.insecure_dev,
    };
    Ok(task)
}
pub async fn verify_report(
    state: &AppState,
    e: &ImageExecution,
    token: &str,
    report: &artifactory_image_protocol::Report,
) -> Result<()> {
    let config = Config::load()?;
    let registry = artifactory_image_registry::Registry::new(
        config.login(e.id, token),
        state.settings.insecure_dev,
    )?;
    match report {
        artifactory_image_protocol::Report::Platform { result } => {
            result.validate()?;
            ensure!(
                e.spec.platforms.contains(&result.platform),
                "unrequested platform"
            );
            let verified = registry
                .verify_platform(&e.candidate_repository, &result.digest, &result.platform)
                .await?;
            ensure!(
                &verified == result,
                "platform report does not match registry content"
            );
        }
        artifactory_image_protocol::Report::Published { digest } => {
            let expected = if let Some(root) = &e.root_digest {
                root.clone()
            } else {
                artifactory_image_protocol::sha256(&artifactory_image_protocol::index(
                    &e.spec.platforms,
                    &e.results,
                )?)
            };
            ensure!(digest == &expected, "published index digest mismatch");
            let destination = if let Some(id) = e.destination_credential {
                artifactory_image_registry::Registry::new(
                    serde_json::from_slice(&credential(state, e, id, "registry").await?)?,
                    state.settings.insecure_dev,
                )?
            } else {
                registry
            };
            let bytes = destination
                .manifest(&e.destination_repository, digest)
                .await?;
            let index: Value = serde_json::from_slice(&bytes)?;
            ensure!(
                index["mediaType"] == artifactory_image_protocol::INDEX_MEDIA_TYPE,
                "publication must be an OCI Index"
            );
            for platform in &e.spec.platforms {
                destination
                    .verify_platform(&e.destination_repository, digest, platform)
                    .await?;
            }
            for tag in &e.tags {
                ensure!(
                    artifactory_image_protocol::sha256(
                        &destination.manifest(&e.destination_repository, tag).await?
                    ) == *digest,
                    "tag points at a different image"
                );
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactory_source_checkout::{LfsObject, SourceManifest, SourceRepository};
    use std::collections::BTreeMap;

    fn source(objects: usize) -> SourceManifest {
        SourceManifest::new(
            Uuid::new_v4(),
            vec![SourceRepository {
                path: String::new(),
                upstream_url: "https://git.example.com/repository.git".into(),
                commit_sha: "a".repeat(40),
                tags: BTreeMap::new(),
                lfs_objects: (0..objects)
                    .map(|index| LfsObject {
                        path: format!("payload-{index:05}.bin"),
                        oid: "b".repeat(64),
                        size: 1,
                    })
                    .collect(),
            }],
        )
        .unwrap()
    }

    fn materials(source: &SourceManifest) -> (BuildTask, PublishTask) {
        let (source_manifest_id, source_digest) =
            fixed_source_identity(source, source.id, &"a".repeat(40)).unwrap();
        let registry = RegistryLogin {
            endpoint: "https://registry.example.com".into(),
            username: "fixture".into(),
            password: "synthetic-test-value".into(),
        };
        let image = serde_json::from_value::<artifactory_image_protocol::ImageSpec>(json!({
            "name": "application", "platforms": ["linux/arm64"],
        }))
        .unwrap();
        image.validate().unwrap();
        (
            BuildTask {
                execution_id: Uuid::new_v4().to_string(),
                token: "synthetic-build-token".into(),
                control_url: "https://control.example.com/api/v1/image-tasks/fixture".into(),
                registry: registry.clone(),
                repository: "candidate/fixture".into(),
                image,
                source_manifest_id,
                source_digest,
                timeout_seconds: 600,
                docker_auth: json!({"auths": {}}),
                secrets: BTreeMap::new(),
                insecure: false,
            },
            PublishTask {
                execution_id: Uuid::new_v4().to_string(),
                token: "synthetic-publish-token".into(),
                control_url: "https://control.example.com/api/v1/image-tasks/fixture".into(),
                registry,
                source_repository: "candidate/fixture".into(),
                destination_repository: "project/application".into(),
                tags: vec!["latest".into()],
                platforms: vec!["linux/arm64".into()],
                insecure: false,
                destination: None,
                root_digest: None,
            },
        )
    }

    #[test]
    fn large_fixed_manifest_stays_out_of_kubernetes_task_secret() {
        let source = source(32_000);
        source.validate().unwrap();
        let manifest_bytes = serde_json::to_vec(&source).unwrap();
        assert!(manifest_bytes.len() > 3 * 1024 * 1024);
        let (build, publish) = materials(&source);
        let bytes = super::super::dispatch::serialize_materials(&build, &publish).unwrap();
        assert!(bytes.len() < 16 * 1024);
        let serialized: BTreeMap<String, String> = serde_json::from_slice(&bytes).unwrap();
        let decoded: BuildTask = serde_json::from_str(&serialized["build.json"]).unwrap();
        assert_eq!(decoded.source_manifest_id, source.id);
        assert_eq!(decoded.source_digest, source.digest);
        let mut raw: Value = serde_json::from_str(&serialized["build.json"]).unwrap();
        assert!(raw.get("source").is_none());
        raw["source"] = json!({});
        assert!(serde_json::from_value::<BuildTask>(raw).is_err());
        println!(
            "manifest_bytes={}, material_envelope_bytes={}, secret_data_bytes={}",
            manifest_bytes.len(),
            bytes.len(),
            serialized.values().map(String::len).sum::<usize>()
        );
    }

    #[test]
    fn fixed_source_identity_still_validates_manifest_id_commit_and_digest() {
        let mut source = source(1);
        assert!(fixed_source_identity(&source, Uuid::new_v4(), &"a".repeat(40)).is_err());
        assert!(fixed_source_identity(&source, source.id, &"c".repeat(40)).is_err());
        source.digest = "d".repeat(64);
        assert!(fixed_source_identity(&source, source.id, &"a".repeat(40)).is_err());
    }

    #[test]
    fn oversized_execution_credentials_still_exceed_secret_limit() {
        let (mut build, publish) = materials(&source(1));
        build
            .secrets
            .insert("LARGE_FIXTURE".into(), "x".repeat(900 * 1024));
        let error = super::super::dispatch::serialize_materials(&build, &publish).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("task Secret exceeds Kubernetes limit")
        );
    }
}
