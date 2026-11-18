//! [INPUT]: 依赖 Serde、Git 对象身份和 SHA-256。
//! [OUTPUT]: 提供唯一源码清单、临时凭据和安全路径。
//! [POS]: 所有执行器共享的数据契约，不保存源码字节。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;
use zeroize::Zeroize;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LfsObject {
    pub path: String,
    pub oid: String,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceRepository {
    pub path: String,
    pub upstream_url: String,
    pub commit_sha: String,
    pub tags: BTreeMap<String, String>,
    pub lfs_objects: Vec<LfsObject>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceManifest {
    pub id: Uuid,
    pub digest: String,
    pub repositories: Vec<SourceRepository>,
}
impl SourceManifest {
    pub fn new(id: Uuid, mut repositories: Vec<SourceRepository>) -> Result<Self> {
        repositories.sort_by(|a, b| a.path.cmp(&b.path));
        for repo in &mut repositories {
            repo.lfs_objects.sort_by(|a, b| a.path.cmp(&b.path));
        }
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(&repositories)?));
        let manifest = Self {
            id,
            digest,
            repositories,
        };
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            serde_json::to_vec(self)?.len() <= 4 * 1024 * 1024,
            "source manifest exceeds 4 MiB"
        );
        ensure!(
            !self.id.is_nil() && !self.repositories.is_empty() && self.repositories.len() <= 256,
            "invalid source manifest"
        );
        ensure!(
            self.repositories[0].path.is_empty(),
            "source manifest has no root"
        );
        let mut previous = None;
        for repo in &self.repositories {
            validate_repository(&repo.upstream_url)?;
            validate_path(&repo.path)?;
            ensure!(valid_commit(&repo.commit_sha), "invalid source commit");
            ensure!(
                previous.is_none_or(|p: &str| p < repo.path.as_str()),
                "source paths must be unique and sorted"
            );
            previous = Some(repo.path.as_str());
            ensure!(
                repo.path.split('/').count() <= 32,
                "source path exceeds nesting limit"
            );
            for (name, oid) in &repo.tags {
                ensure!(valid_tag(name) && valid_commit(oid), "invalid frozen tag");
            }
            let mut last = None;
            for item in &repo.lfs_objects {
                validate_path(&item.path)?;
                ensure!(
                    !item.path.is_empty() && item.oid.len() == 64 && valid_commit(&item.oid),
                    "invalid LFS identity"
                );
                ensure!(
                    last.is_none_or(|p: &str| p < item.path.as_str()),
                    "LFS paths must be unique and sorted"
                );
                last = Some(item.path.as_str());
            }
        }
        ensure!(
            self.digest == hex::encode(Sha256::digest(serde_json::to_vec(&self.repositories)?)),
            "source manifest digest mismatch"
        );
        Ok(())
    }
    pub fn root(&self) -> Result<&SourceRepository> {
        self.validate()?;
        self.repositories.first().context("source root missing")
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCredential {
    pub kind: String,
    #[serde(default)]
    pub username: String,
    pub secret: String,
    #[serde(default)]
    pub known_hosts: String,
}
impl fmt::Debug for GitCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitCredential")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl Drop for GitCredential {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCredential {
    pub upstream_url: String,
    pub credential: GitCredential,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareResponse {
    pub source_id: Uuid,
    pub repository_url: String,
    pub reference: String,
    pub pinned_ref_oid: Option<String>,
    pub frozen_tags: BTreeMap<String, BTreeMap<String, String>>,
    pub manifest: Option<SourceManifest>,
    pub credentials: Vec<SourceCredential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_result: Option<serde_json::Value>,
}

pub fn valid_commit(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64)
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && !tag.starts_with('-')
        && !tag.contains([' ', '\0', '\r', '\n', '~', '^', ':', '?', '*', '[', '\\'])
        && !tag.contains("..")
        && !tag.contains("@{")
        && tag.split('/').all(|p| {
            !p.is_empty() && !p.starts_with('.') && !p.ends_with('.') && !p.ends_with(".lock")
        })
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    ensure!(
        !path.starts_with('/') && !path.contains(['\\', ':', '\0', '\r', '\n']),
        "unsafe source path"
    );
    ensure!(
        path.is_empty()
            || path
                .split('/')
                .all(|p| !matches!(p, "" | "." | "..") && !p.eq_ignore_ascii_case(".git")),
        "unsafe source path"
    );
    Ok(())
}
pub fn managed_path(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_path(relative)?;
    let mut target = root.to_path_buf();
    for part in relative.split('/').filter(|p| !p.is_empty()) {
        target.push(part);
        match fs::symlink_metadata(&target) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "source path crosses a symbolic link"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(target)
}
pub fn validate_repository(repository: &str) -> Result<()> {
    ensure!(
        repository.len() <= 2048 && !repository.contains(['\0', '\r', '\n']),
        "invalid repository URL"
    );
    if repository.starts_with("http://")
        || repository.starts_with("https://")
        || repository.starts_with("ssh://")
    {
        let url = url::Url::parse(repository)?;
        ensure!(
            url.host_str().is_some()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "invalid repository URL"
        );
        ensure!(
            url.scheme() == "ssh" || url.username().is_empty(),
            "use a credential reference, not URL credentials"
        );
    } else {
        let (host, path) = repository
            .split_once(':')
            .context("only SSH, HTTPS and explicit HTTP repositories are supported")?;
        ensure!(
            host.contains('@')
                && !host.starts_with('-')
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"@._-".contains(&b)),
            "invalid SSH repository"
        );
        ensure!(
            !path.is_empty() && !path.starts_with('-'),
            "invalid SSH path"
        );
    }
    Ok(())
}
fn url(repository: &str) -> Result<url::Url> {
    validate_repository(repository)?;
    if repository.contains("://") {
        Ok(url::Url::parse(repository)?)
    } else {
        let (host, path) = repository
            .split_once(':')
            .context("invalid SSH repository")?;
        Ok(url::Url::parse(&format!("ssh://{host}/{path}"))?)
    }
}
pub fn same_origin(a: &str, b: &str) -> bool {
    let origin = |s| {
        url(s).ok().map(|u| {
            (
                u.scheme().to_owned(),
                u.host_str().map(str::to_owned),
                u.port_or_known_default().unwrap_or(22),
            )
        })
    };
    let a = origin(a);
    a.is_some() && a == origin(b)
}
pub fn resolve_submodule(parent: &str, child: &str) -> Result<String> {
    ensure!(
        child.len() <= 2048 && !child.contains(['\0', '\r', '\n']),
        "invalid submodule URL"
    );
    let resolved = if child.starts_with("./") || child.starts_with("../") {
        url::Url::parse(&format!("{}/", url(parent)?.as_str().trim_end_matches('/')))?
            .join(child)?
            .to_string()
    } else {
        child.to_owned()
    };
    validate_repository(&resolved)?;
    Ok(resolved)
}
