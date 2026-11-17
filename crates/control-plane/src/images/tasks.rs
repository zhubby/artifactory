//! [INPUT]: 依赖固定 Assignment、项目凭据引用与执行令牌。
//! [OUTPUT]: 生成独立任务材料，核对发布仓库来源快照。
//! [POS]: 明文材料只进入加密信封和 Kubernetes Secret。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::Config;
use crate::{crypto, state::AppState, store::images::ImageExecution};
use anyhow::{Result, ensure};
use artifactory_image_protocol::{BuildTask, PublishTask, RegistryLogin};
use serde_json::{Value, json};
use uuid::Uuid;

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
) -> Result<(BuildTask, String)> {
    let context = state.store.assignment_context(e.job_id).await?;
    ensure!(
        context.source_snapshot_id == Some(e.snapshot_id)
            && context.plan_revision_id == e.plan_revision_id,
        "source identity changed"
    );
    let snapshot = crate::source_cache::load(state, e.snapshot_id).await?;
    crate::source_cache::ensure_available(state, &snapshot).await?;
    let source_token = crypto::token();
    let origin = crate::runtime_bridge::source_download_origin(state, e.snapshot_id)
        .await?
        .unwrap_or_else(|| state.settings.public_url.clone());
    let root = format!("{origin}/api/v1/source/{}", e.snapshot_id);
    let source = crate::protocol::SourceDescriptor {
        snapshot_id: e.snapshot_id.to_string(),
        access_token: source_token.clone(),
        repositories: snapshot
            .repositories
            .iter()
            .map(|r| crate::protocol::SourceRepository {
                path: r.path.clone(),
                repository_id: r.id.to_string(),
                commit_sha: r.commit.clone(),
                git_url: format!("{root}/{}/git", r.id),
                lfs_url: format!("{root}/{}/lfs", r.id),
                has_lfs: !r.lfs.is_empty(),
                tags: r.tags.clone().into_iter().collect(),
                cache_id: r.repository_id.to_string(),
            })
            .collect(),
    };
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
    let assignment = crate::protocol::Assignment {
        job_id: e.job_id.to_string(),
        build_id: context.id.to_string(),
        target: context.target,
        repository_url: context.repository_url,
        commit_sha: context.commit_sha,
        source: Some(source),
        timeout_seconds: e.task_timeout_seconds.saturating_sub(600).max(1),
        plan_revision_id: e
            .plan_revision_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        ..Default::default()
    };
    ensure!(
        snapshot.commit == assignment.commit_sha,
        "snapshot commit mismatch"
    );
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
        cache_repository: format!("{}/{}/cache/{}", e.tenant_id, e.project_id, e.spec.name),
        image: e.spec.clone(),
        assignment,
        docker_auth,
        secrets,
        insecure: state.settings.insecure_dev,
    };
    Ok((task, crypto::hash(source_token)))
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
