//! [INPUT]: 依赖固定根提交、prepare RPC 和共享检出引擎。
//! [OUTPUT]: 在最终目录生成并绑定清单，确认本代源码就绪。
//! [POS]: init 与 AI 共用 emptyDir；认证材料留在独立临时目录。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
use artifactory_source_checkout::{
    SourceManifest, SourceRepository, TagSelection, managed_path, same_origin,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) async fn prepare(
    api: &Api,
    progress: &mut Progress,
    material: &PrepareResponse,
    checkout: &Checkout,
    root: &Path,
    cancel: &CancellationToken,
) -> Result<()> {
    let fixed = material.manifest.as_ref();
    if let Some(manifest) = fixed {
        manifest.validate()?;
        ensure!(
            manifest.id == material.source_id,
            "source identity mismatch"
        );
        ensure!(
            manifest.root()?.upstream_url == material.repository_url,
            "source repository mismatch"
        );
    }
    let commit = fixed
        .map(|manifest| manifest.root().map(|repo| repo.commit_sha.clone()))
        .transpose()?
        .or_else(|| material.pinned_ref_oid.clone())
        .context("pinned source commit required")?;
    ensure!(valid_commit(&commit), "invalid source commit");
    if let Some(pinned) = &material.pinned_ref_oid {
        ensure!(*pinned == commit, "pinned source commit mismatch");
    }
    ensure!(!root.exists(), "source workspace already exists");
    let mut pending = vec![(
        String::new(),
        material.repository_url.clone(),
        commit,
        None::<String>,
        0u32,
    )];
    let mut seen = BTreeSet::new();
    let mut repos = Vec::new();
    let mut inherited_repos = BTreeMap::new();
    progress.start(api, SourceStage::Fetch).await;
    let mut modules = false;
    while let Some((path, url, commit, inherited, depth)) = pending.pop() {
        ensure!(
            depth <= 16 && repos.len() < 256 && seen.insert(path.clone()),
            "source submodule graph exceeds limit or repeats a path"
        );
        if !path.is_empty() && !modules {
            progress.complete(api).await;
            progress.start(api, SourceStage::Submodules).await;
            modules = true;
        }
        let fixed_repo =
            fixed.and_then(|manifest| manifest.repositories.iter().find(|repo| repo.path == path));
        if let Some(expected) = fixed_repo {
            ensure!(
                expected.upstream_url == url && expected.commit_sha == commit,
                "source submodule differs from fixed Git link"
            );
        } else {
            ensure!(fixed.is_none(), "source manifest omits a Git submodule");
        }
        let frozen = fixed_repo
            .map(|repo| &repo.tags)
            .or_else(|| material.frozen_tags.get(&path));
        let desc = SourceRepository {
            path: path.clone(),
            upstream_url: url.clone(),
            commit_sha: commit,
            tags: frozen.cloned().unwrap_or_default(),
            lfs_objects: Vec::new(),
        };
        let target = managed_path(root, &path)?;
        let inspected = checkout
            .discover_repository(
                &desc,
                &material.credentials,
                inherited.as_deref(),
                &target,
                if frozen.is_some() {
                    TagSelection::Frozen
                } else {
                    TagSelection::Upstream
                },
                cancel,
            )
            .await?;
        let mut repo = inspected.repository;
        if let Some(expected) = fixed_repo {
            ensure!(
                repo == *expected,
                "source repository differs from fixed manifest"
            );
        } else {
            let pin: Value = api
                .post(
                    "/pin",
                    json!({"pin":{
                        "kind":"tags","path":path,"tags":repo.tags
                    }}),
                    cancel,
                )
                .await?;
            repo.tags = serde_json::from_value(pin["tags"].clone())?;
        }
        checkout
            .freeze_local_tags(
                &repo,
                &material.credentials,
                inherited.as_deref(),
                &target,
                cancel,
            )
            .await?;
        let effective = material
            .credentials
            .iter()
            .find(|c| c.upstream_url == url)
            .map(|c| c.upstream_url.clone())
            .or(inherited.clone());
        for child in inspected.submodules.into_iter().rev() {
            let next = if path.is_empty() {
                child.path
            } else {
                format!("{path}/{}", child.path)
            };
            let inherited = effective
                .as_ref()
                .filter(|origin| same_origin(origin, &child.upstream_url))
                .cloned();
            pending.push((
                next,
                child.upstream_url,
                child.commit_sha,
                inherited,
                depth + 1,
            ));
        }
        inherited_repos.insert(path, inherited);
        repos.push(repo);
    }
    progress.complete(api).await;
    if !modules {
        progress.start(api, SourceStage::Submodules).await;
        progress.complete(api).await;
    }
    let manifest = SourceManifest::new(material.source_id, repos)?;
    if let Some(expected) = fixed {
        ensure!(
            manifest == *expected,
            "source manifest differs from final tree"
        );
    }
    progress.start(api, SourceStage::Lfs).await;
    {
        for repo in &manifest.repositories {
            checkout
                .prepare_repository_lfs(
                    repo,
                    &material.credentials,
                    inherited_repos
                        .get(&repo.path)
                        .and_then(|url| url.as_deref()),
                    &managed_path(root, &repo.path)?,
                    cancel,
                )
                .await?;
        }
    }
    progress.complete(api).await;
    progress.start(api, SourceStage::Verify).await;
    checkout.verify(&manifest, root, cancel).await?;
    progress.complete(api).await;
    progress.start(api, SourceStage::Manifest).await;
    let until = tokio::time::Instant::now() + Duration::from_secs(900);
    loop {
        let result: Value = api
            .post("/commit", json!({"result":{"manifest":manifest}}), cancel)
            .await?;
        if result["accepted"] == true {
            ensure!(
                result["source_manifest_id"] == json!(manifest.id)
                    && result["source_digest"] == manifest.digest,
                "source binding receipt mismatch"
            );
            break;
        }
        ensure!(
            result["pending"] == true && tokio::time::Instant::now() < until,
            "source binding was not accepted"
        );
        tokio::select! {
            _ = cancel.cancelled() => anyhow::bail!("source preparation canceled"),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    }
    progress.complete(api).await;
    let metadata = path(
        "ARTIFACTORY_SOURCE_METADATA",
        "/workspace/source-manifest.json",
    );
    ensure!(
        !metadata.starts_with(root),
        "source metadata must be outside source checkout"
    );
    private_dir(
        metadata
            .parent()
            .context("source metadata parent missing")?,
    )?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(metadata)?;
    file.write_all(&serde_json::to_vec(&manifest)?)?;
    file.sync_all()?;
    progress.start(api, SourceStage::Ready).await;
    let ready: Value = api
        .post(
            "/ready",
            json!({
                "source_manifest_id":manifest.id,"source_digest":manifest.digest
            }),
            cancel,
        )
        .await?;
    ensure!(
        ready["ok"] == true,
        "source local readiness was not accepted"
    );
    // Ready 回执是执行证据；后续观测失败不撤销该证据。
    progress.complete(api).await;
    Ok(())
}
