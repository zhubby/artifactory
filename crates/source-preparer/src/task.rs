//! [INPUT]: 依赖 prepare/pin/commit API 与 Pod 投影令牌。
//! [OUTPUT]: 固定源码清单，上报阶段与安全诊断，或写入临时 checkout。
//! [POS]: 只向控制平面回传元数据，凭据不进入工作区。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::Mode;
use anyhow::{Context, Result, ensure};
use artifactory_source_checkout::{
    Checkout, GitFailure, GitFailureKind, Options, PrepareResponse, SourceDiagnostic,
    SourceManifest, SourceProgress, SourceProgressState, SourceRepository, SourceStage,
    TagSelection, private_dir, same_origin, valid_commit,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

struct Api {
    http: reqwest::Client,
    url: String,
    identity: Value,
    pod: Value,
    token: PathBuf,
}
impl Api {
    async fn new() -> Result<Self> {
        let url = required("ARTIFACTORY_SOURCE_PREPARE_URL")?;
        let parsed = url::Url::parse(&url)?;
        ensure!(
            matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none(),
            "invalid source prepare URL"
        );
        let mut http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(ca) = std::env::var_os("ARTIFACTORY_SERVICE_CA") {
            http = http
                .add_root_certificate(reqwest::Certificate::from_pem(&tokio::fs::read(ca).await?)?);
        }
        Ok(Self {
            http: http.build()?,
            url: url.trim_end_matches('/').to_owned(),
            identity: serde_json::from_str(&required("ARTIFACTORY_SOURCE_IDENTITY")?)?,
            pod: json!({"uid":required("POD_UID")?,"name":required("POD_NAME")?,"namespace":required("POD_NAMESPACE")?}),
            token: PathBuf::from(required("ARTIFACTORY_POD_TOKEN")?),
        })
    }
    async fn post<T: DeserializeOwned>(
        &self,
        suffix: &str,
        extra: Value,
        cancel: &CancellationToken,
    ) -> Result<T> {
        let attempts = if suffix.is_empty() { 30 } else { 3 };
        let deadline = tokio::time::Instant::now()
            + Duration::from_secs(if suffix.is_empty() { 60 } else { 20 });
        for attempt in 0..attempts {
            let token = tokio::fs::read_to_string(&self.token).await?;
            let mut body =
                json!({"identity":self.identity,"pod":self.pod,"bootstrap_token":token.trim()});
            body.as_object_mut()
                .context("invalid request envelope")?
                .extend(
                    extra
                        .as_object()
                        .context("invalid request payload")?
                        .clone(),
                );
            let response = tokio::select! {
                _=cancel.cancelled()=>anyhow::bail!("source preparation canceled"),
                response=self.http.post(format!("{}{suffix}",self.url)).json(&body).send()=>response,
            };
            let retry = match response {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        let bytes = response.bytes().await?;
                        ensure!(
                            bytes.len() <= 8 * 1024 * 1024,
                            "source prepare response exceeds limit"
                        );
                        return Ok(serde_json::from_slice(&bytes)?);
                    }
                    let retry = matches!(status.as_u16(), 404 | 409 | 425 | 429 | 502 | 503 | 504);
                    ensure!(
                        retry,
                        "source authorization or metadata commit rejected ({status})"
                    );
                    true
                }
                Err(_) => true,
            };
            if !retry || attempt + 1 == attempts || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::select! {_=cancel.cancelled()=>anyhow::bail!("source preparation canceled"),_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        }
        anyhow::bail!("source bootstrap or metadata commit did not complete before retry deadline")
    }
}
struct Progress {
    stage: SourceStage,
    sequence: u32,
}
impl Progress {
    async fn report(&mut self, api: &Api, stage: SourceStage, state: SourceProgressState) {
        self.stage = stage;
        if api.identity["kind"] != "source-task" {
            return;
        }
        self.sequence += 1;
        let progress = SourceProgress {
            sequence: self.sequence,
            stage,
            state,
        };
        // 观测写入失败不改变构建结果；终态提交仍携带当前阶段。
        let _ = tokio::time::timeout(
            Duration::from_secs(3),
            api.post::<Value>(
                "/progress",
                json!({"progress":progress}),
                &CancellationToken::new(),
            ),
        )
        .await;
    }
    async fn start(&mut self, api: &Api, stage: SourceStage) {
        self.report(api, stage, SourceProgressState::Started).await;
    }
    async fn complete(&mut self, api: &Api) {
        self.report(api, self.stage, SourceProgressState::Completed)
            .await;
    }
}
pub async fn run(mode: Mode, cancel: &CancellationToken) -> Result<()> {
    let api = Api::new().await?;
    let mut progress = Progress {
        stage: SourceStage::Authorization,
        sequence: 0,
    };
    let result = execute(mode, &api, &mut progress, cancel).await;
    if let Err(error) = &result
        && !matches!(mode, Mode::Checkout)
    {
        let code = error
            .downcast_ref::<GitFailure>()
            .map(GitFailure::kind)
            .unwrap_or(GitFailureKind::Unknown);
        let diagnostic = SourceDiagnostic {
            stage: progress.stage,
            code,
        };
        let _ = api
            .post::<Value>(
                "/commit",
                json!({"result":{"error":code.message(),"diagnostic":diagnostic}}),
                &CancellationToken::new(),
            )
            .await;
    }
    result
}
async fn execute(
    mode: Mode,
    api: &Api,
    progress: &mut Progress,
    cancel: &CancellationToken,
) -> Result<()> {
    let material: PrepareResponse = api.post("", json!({}), cancel).await?;
    if !matches!(mode, Mode::Checkout) && material.completed_result.is_some() {
        return Ok(());
    }
    if !matches!(mode, Mode::Checkout) {
        progress.complete(api).await;
    }
    let root = path("ARTIFACTORY_SOURCE_ROOT", "/workspace/source");
    let auth = path("ARTIFACTORY_SOURCE_AUTH_ROOT", "/run/source-auth");
    private_dir(&auth)?;
    let checkout = Checkout::new(Options {
        auth_root: auth,
        ca_file: std::env::var_os("ARTIFACTORY_SERVICE_CA")
            .map(PathBuf::from)
            .unwrap_or_default(),
        ..Default::default()
    });
    match mode {
        Mode::Checkout => {
            let manifest = material
                .manifest
                .as_ref()
                .context("fixed source manifest required")?;
            ensure!(
                manifest.id == material.source_id,
                "source identity mismatch"
            );
            checkout
                .checkout(manifest, &material.credentials, &root, cancel)
                .await?;
            let metadata = path(
                "ARTIFACTORY_SOURCE_METADATA",
                "/workspace/source-manifest.json",
            );
            ensure!(
                !metadata.starts_with(&root),
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
            file.write_all(&serde_json::to_vec(manifest)?)?;
            file.sync_all()?;
        }
        Mode::Refs => {
            progress.start(api, SourceStage::ResolveRef).await;
            let refs = checkout
                .list_refs(&material.repository_url, &material.credentials, cancel)
                .await?;
            progress.complete(api).await;
            let _: Value = api
                .post("/commit", json!({"result":{"refs":refs}}), cancel)
                .await?;
        }
        Mode::Resolve => {
            let parent = root.parent().context("source workspace parent missing")?;
            private_dir(parent)?;
            let temporary = tempfile::Builder::new()
                .prefix("resolve-")
                .tempdir_in(parent)?;
            progress.start(api, SourceStage::ResolveRef).await;
            let candidate = if let Some(oid) = material.pinned_ref_oid.as_deref() {
                ensure!(valid_commit(oid), "invalid pinned ref OID");
                oid.to_owned()
            } else {
                checkout
                    .resolve_reference(
                        &material.repository_url,
                        &material.reference,
                        &material.credentials,
                        cancel,
                    )
                    .await?
            };
            progress.complete(api).await;
            progress.start(api, SourceStage::PinRef).await;
            let pinned: Value = api
                .post(
                    "/pin",
                    json!({"pin":{"kind":"ref","oid":candidate}}),
                    cancel,
                )
                .await?;
            let oid = pinned["oid"]
                .as_str()
                .context("pinned ref OID missing")?
                .to_owned();
            ensure!(valid_commit(&oid), "invalid pinned ref OID");
            progress.complete(api).await;
            progress.start(api, SourceStage::Inspect).await;
            let mut pending = vec![(
                String::new(),
                material.repository_url.clone(),
                oid,
                None::<String>,
                0u32,
            )];
            let mut seen = BTreeSet::new();
            let mut repositories = Vec::new();
            while let Some((path, url, commit, inherited, depth)) = pending.pop() {
                ensure!(
                    depth <= 16 && repositories.len() < 256 && seen.insert(path.clone()),
                    "source submodule graph exceeds limit or repeats a path"
                );
                let frozen = material.frozen_tags.get(&path);
                let descriptor = SourceRepository {
                    path: path.clone(),
                    upstream_url: url.clone(),
                    commit_sha: commit,
                    tags: frozen.cloned().unwrap_or_default(),
                    lfs_objects: Vec::new(),
                };
                let target = temporary
                    .path()
                    .join(format!("repository-{}", repositories.len()));
                let inspected = checkout
                    .inspect_repository(
                        &descriptor,
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
                let mut repository = inspected.repository;
                let pinned: Value = api
                    .post(
                        "/pin",
                        json!({"pin":{"kind":"tags","path":path,"tags":repository.tags}}),
                        cancel,
                    )
                    .await?;
                repository.tags = serde_json::from_value(
                    pinned.get("tags").cloned().context("frozen tags missing")?,
                )?;
                let effective = material
                    .credentials
                    .iter()
                    .find(|c| c.upstream_url == url)
                    .map(|c| c.upstream_url.clone())
                    .or(inherited);
                for child in inspected.submodules.into_iter().rev() {
                    let child_path = if path.is_empty() {
                        child.path
                    } else {
                        format!("{path}/{}", child.path)
                    };
                    let inherited = effective
                        .as_ref()
                        .filter(|origin| same_origin(origin, &child.upstream_url))
                        .cloned();
                    pending.push((
                        child_path,
                        child.upstream_url,
                        child.commit_sha,
                        inherited,
                        depth + 1,
                    ));
                }
                repositories.push(repository);
            }
            progress.complete(api).await;
            progress.start(api, SourceStage::Manifest).await;
            let manifest = SourceManifest::new(material.source_id, repositories)?;
            let _: Value = api
                .post("/commit", json!({"result":{"manifest":manifest}}), cancel)
                .await?;
            temporary
                .close()
                .context("remove source resolution workspace")?;
        }
    }
    Ok(())
}
fn required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} is required"))
}
fn path(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(default).to_owned())
}
