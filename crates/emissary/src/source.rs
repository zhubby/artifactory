//! [INPUT]: 依赖任务清单、设备 mTLS、共享检出引擎与受管目录清理。
//! [OUTPUT]: 隔离缓存并验证固定源码，回收会话后清理 Windows 缓存链接。
//! [POS]: 原生 Worker 的源码边界，不保存 Git/LFS 缓存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    client,
    config::{Config, private_dir},
    protocol::{Assignment, SourceMaterialRequest},
    worker::{Emitter, Event, Input},
};
use anyhow::{Context, Result, bail, ensure};
use artifactory_source_checkout::{Checkout, Options, PrepareResponse, SourceManifest};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub use artifactory_source_checkout::{build_environment, managed_path, valid_commit};
pub const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
pub fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id)
}
pub fn manifest(assignment: &Assignment) -> Result<SourceManifest> {
    let manifest: SourceManifest = serde_json::from_str(&assignment.source_manifest_json)
        .context("fixed source manifest required")?;
    manifest.validate()?;
    Ok(manifest)
}
fn checkout_engine(cfg: &Config, auth_root: PathBuf) -> Checkout {
    Checkout::new(Options {
        git: cfg.tool("git"),
        ca_file: cfg.git_ca_file.clone(),
        auth_root,
        ..Default::default()
    })
}
pub async fn checkout(
    input: &Input,
    base: &Path,
    workspace: &Path,
    cancel: &CancellationToken,
    emit: &Emitter,
) -> Result<()> {
    let assignment = input.assignment.as_ref().context("missing assignment")?;
    let expected = manifest(assignment)?;
    let mut control = client::control(&input.config).await?;
    let reply = tokio::select! {
        _=cancel.cancelled()=>bail!("source authorization canceled"),
        reply=control.acquire_source_material(SourceMaterialRequest{attempt_id:input.attempt_id.clone(),lease_token:input.lease_token.clone(),source_digest:expected.digest.clone()})=>reply?.into_inner(),
    };
    ensure!(
        reply.material_json.len() <= 8 * 1024 * 1024,
        "source material exceeds limit"
    );
    let material: PrepareResponse = serde_json::from_str(&reply.material_json)?;
    ensure!(
        material.manifest.as_ref() == Some(&expected) && material.source_id == expected.id,
        "source authorization identity changed"
    );
    let auth = base.join("source-auth");
    private_dir(&auth)?;
    let result = checkout_engine(&input.config, auth.clone())
        .checkout(&expected, &material.credentials, workspace, cancel)
        .await;
    drop(material);
    fs::remove_dir_all(&auth).context("remove source authentication material")?;
    result?;
    emit.send(Event {
        kind: "log".into(),
        step: "checkout".into(),
        data: format!(
            "Checked out pinned source {}\n",
            expected.root()?.commit_sha
        )
        .into_bytes(),
        ..Default::default()
    })
    .await?;
    Ok(())
}
pub async fn verify(
    cfg: &Config,
    assignment: &Assignment,
    workspace: &Path,
    cancel: &CancellationToken,
) -> Result<()> {
    let manifest = manifest(assignment)?;
    let auth = workspace
        .parent()
        .context("workspace parent missing")?
        .join("verify-auth");
    let result = checkout_engine(cfg, auth.clone())
        .verify(&manifest, workspace, cancel)
        .await;
    if auth.exists() {
        fs::remove_dir_all(auth).context("remove verification temporary directory")?;
    }
    result
}
/// 各语言的源码与中间缓存只进入当前 attempt。
pub fn isolate_environment(base: &Path, env: &mut BTreeMap<OsString, OsString>) -> Result<()> {
    let original_home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if !env.contains_key(std::ffi::OsStr::new("RUSTUP_HOME"))
        && let Some(home) = original_home.as_ref()
        && home.join(".rustup").exists()
    {
        env.insert("RUSTUP_HOME".into(), home.join(".rustup").into_os_string());
    }
    for (name, relative) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("APPDATA", "home/AppData/Roaming"),
        ("LOCALAPPDATA", "home/AppData/Local"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_STATE_HOME", "state"),
        ("CARGO_HOME", "cache/cargo"),
        ("CARGO_TARGET_DIR", "cache/cargo-target"),
        ("GOPATH", "cache/go"),
        ("GOCACHE", "cache/go-build"),
        ("GOMODCACHE", "cache/go/pkg/mod"),
        ("npm_config_cache", "cache/npm"),
        ("YARN_CACHE_FOLDER", "cache/yarn"),
        ("PNPM_HOME", "cache/pnpm"),
        ("GRADLE_USER_HOME", "cache/gradle"),
        ("MAVEN_USER_HOME", "cache/maven"),
        ("PIP_CACHE_DIR", "cache/pip"),
        ("UV_CACHE_DIR", "cache/uv"),
        ("DOTNET_CLI_HOME", "home/dotnet"),
        ("NUGET_PACKAGES", "cache/nuget"),
    ] {
        let path = base.join(relative);
        private_dir(&path)?;
        env.insert(name.into(), path.into_os_string());
    }
    let repository = base.join("cache/maven/repository");
    private_dir(&repository)?;
    let java = [
        ("user.home", base.join("home")),
        ("java.io.tmpdir", base.join("tmp")),
        ("maven.repo.local", repository),
    ]
    .into_iter()
    .map(|(name, path)| {
        let path = path.to_str().context("JVM task path must be UTF-8")?;
        ensure!(
            !path.contains(['"', '\r', '\n']),
            "JVM task path contains unsupported characters"
        );
        let path = if cfg!(windows) {
            path.replace('\\', "/")
        } else {
            path.to_owned()
        };
        Ok(format!("\"-D{name}={path}\""))
    })
    .collect::<Result<Vec<_>>>()?
    .join(" ");
    for name in ["JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS", "_JAVA_OPTIONS"] {
        let existing = env
            .get(std::ffi::OsStr::new(name))
            .map(|v| v.to_str().context("JVM options must be UTF-8"))
            .transpose()?
            .unwrap_or("");
        env.insert(name.into(), format!("{existing} {java}").into());
    }
    env.insert("PIP_NO_CACHE_DIR".into(), "1".into());
    env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
    env.insert("GIT_CONFIG_GLOBAL".into(), NULL.into());
    Ok(())
}
pub fn attempt_lock(cfg: &Config, id: &str) -> Result<File> {
    ensure!(valid_id(id), "invalid attempt ID");
    let locks = cfg.root.join("work-locks");
    private_dir(&locks)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join(id))?;
    fs2::FileExt::try_lock_exclusive(&file).context("attempt still has an active worker")?;
    Ok(file)
}
pub fn cleanup_attempt(cfg: &Config, id: &str) -> Result<()> {
    let _guard = attempt_lock(cfg, id)?;
    let path = cfg.root.join("work").join(id);
    if path.exists()
        && let Err(error) = crate::cleanup::remove_dir_all(&path)
    {
        crate::config::private_write(
            &cfg.root.join("source-cleanup-blocked"),
            b"source workspace cleanup failed",
            false,
            false,
        )?;
        return Err(error).context("source workspace cleanup failed; task admission is blocked");
    }
    Ok(())
}
pub fn register_session(cfg: &Config, id: &str, pid: u32) -> Result<()> {
    ensure!(valid_id(id), "invalid attempt ID");
    crate::config::atomic_json(
        &cfg.root.join("work-sessions").join(format!("{id}.json")),
        &crate::process::Session::capture(pid)?,
    )
}
pub async fn settle_attempt(cfg: &Config, id: &str) -> Result<()> {
    ensure!(valid_id(id), "invalid attempt ID");
    let record = cfg.root.join("work-sessions").join(format!("{id}.json"));
    if record.exists() {
        let session: crate::process::Session = crate::config::read_json(&record)?;
        if let Err(error) = session.terminate().await {
            crate::config::private_write(
                &cfg.root.join("source-cleanup-blocked"),
                b"worker session cleanup failed",
                false,
                false,
            )?;
            return Err(error);
        }
    }
    cleanup_attempt(cfg, id)?;
    if record.exists() {
        fs::remove_file(record)?;
    }
    Ok(())
}
pub fn ready(cfg: &Config) -> Result<()> {
    ensure!(
        !cfg.root.join("source-cleanup-blocked").exists(),
        "source cleanup requires recovery before accepting tasks"
    );
    Ok(())
}
pub async fn sweep(cfg: &Config) -> Result<()> {
    let work = cfg.root.join("work");
    private_dir(&work)?;
    let sessions = cfg.root.join("work-sessions");
    private_dir(&sessions)?;
    let mut ids = std::collections::BTreeSet::new();
    for entry in fs::read_dir(&sessions)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid worker session name"))?;
        let id = name
            .strip_suffix(".json")
            .context("invalid worker session record")?;
        ensure!(
            entry.file_type()?.is_file() && valid_id(id),
            "unexpected worker session record"
        );
        ids.insert(id.to_owned());
    }
    for entry in fs::read_dir(&work)? {
        let entry = entry?;
        let id = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid workspace name"))?;
        ensure!(
            entry.file_type()?.is_dir() && valid_id(&id),
            "unexpected entry in managed source workspace"
        );
        ids.insert(id);
    }
    for id in ids {
        settle_attempt(cfg, &id).await?;
    }
    match fs::remove_file(cfg.root.join("source-cleanup-blocked")) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
pub async fn lock(path: &Path, cancel: &CancellationToken, timeout: Duration) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        tokio::select! {_=cancel.cancelled()=>bail!("managed lock canceled"),_=tokio::time::sleep_until(deadline)=>bail!("managed lock timed out"),_=tokio::time::sleep(Duration::from_millis(20))=>{}}
    }
}
pub fn validate_url(value: &str, allow_http: bool) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    ensure!(
        (url.scheme() == "https" || (allow_http && url.scheme() == "http"))
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "HTTPS URL without embedded credentials required"
    );
    Ok(url)
}
