//! [INPUT]: 依赖隔离 Git、引用广告和安全失败分类。
//! [OUTPUT]: 保留 tag 对象列表，将首次构建引用固定为剥离提交。
//! [POS]: 只查询引用；不创建仓库或长期缓存。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::diagnostics::{GitFailure, GitFailureKind};
use crate::git::Git;
use crate::{Checkout, Options, SourceCredential, valid_commit};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GitRef {
    pub name: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub oid: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RepositoryRefs {
    pub default_ref: Option<String>,
    pub branches: Vec<GitRef>,
    pub tags: Vec<GitRef>,
}

struct Advertisement {
    refs: RepositoryRefs,
    peeled_tags: BTreeMap<String, String>,
}

pub async fn list_refs(
    options: &Options,
    repository: &str,
    credentials: &[SourceCredential],
    cancel: &CancellationToken,
) -> Result<RepositoryRefs> {
    Ok(advertisement(options, repository, credentials, cancel)
        .await?
        .refs)
}

async fn advertisement(
    options: &Options,
    repository: &str,
    credentials: &[SourceCredential],
    cancel: &CancellationToken,
) -> Result<Advertisement> {
    let git = Git::new(options, repository, credentials, None)?;
    let data = git
        .run(
            git.directory(),
            &[
                "ls-remote",
                "--symref",
                "--",
                repository,
                "HEAD",
                "refs/heads/*",
                "refs/tags/*",
            ],
            cancel,
        )
        .await?;
    git.close()?;
    ensure!(
        data.len() <= 4 * 1024 * 1024,
        "repository reference list exceeds 4 MiB"
    );
    parse(&data)
}
impl Checkout {
    pub async fn list_refs(
        &self,
        repository: &str,
        credentials: &[SourceCredential],
        cancel: &CancellationToken,
    ) -> Result<RepositoryRefs> {
        list_refs(self.options(), repository, credentials, cancel).await
    }
    pub async fn resolve_reference(
        &self,
        repository: &str,
        reference: &str,
        credentials: &[SourceCredential],
        cancel: &CancellationToken,
    ) -> Result<String> {
        ensure!(
            !reference.is_empty()
                && reference.len() <= 255
                && !reference.starts_with('-')
                && !reference.contains(['\0', '\r', '\n']),
            "invalid Git reference"
        );
        if valid_commit(reference) {
            return Ok(reference.to_owned());
        }
        let advertised = advertisement(self.options(), repository, credentials, cancel).await?;
        let refs = &advertised.refs;
        let requested = if reference == "HEAD" {
            refs.default_ref
                .as_deref()
                .context("repository default branch is unavailable")?
        } else {
            reference
        };
        let matches = refs
            .branches
            .iter()
            .chain(refs.tags.iter())
            .filter(|r| r.reference == requested || r.name == requested)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(GitFailure(GitFailureKind::Reference).into());
        }
        let selected = matches[0];
        Ok(advertised
            .peeled_tags
            .get(&selected.reference)
            .unwrap_or(&selected.oid)
            .clone())
    }
}
fn parse(data: &[u8]) -> Result<Advertisement> {
    let mut default_ref = None;
    let mut branches = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut peeled_tags = BTreeMap::new();
    for line in std::str::from_utf8(data)?.lines().filter(|l| !l.is_empty()) {
        let (value, reference) = line
            .split_once('\t')
            .context("invalid repository reference advertisement")?;
        if let Some(symbolic) = value.strip_prefix("ref: ") {
            if reference == "HEAD" && symbolic.starts_with("refs/heads/") {
                default_ref = Some(symbolic.to_owned());
            }
            continue;
        }
        ensure!(valid_commit(value), "invalid repository reference OID");
        let (map, name) = if let Some(name) = reference.strip_prefix("refs/heads/") {
            (&mut branches, name)
        } else if let Some(name) = reference.strip_prefix("refs/tags/") {
            if let Some(name) = name.strip_suffix("^{}") {
                ensure!(!name.is_empty(), "invalid peeled tag reference");
                peeled_tags.insert(format!("refs/tags/{name}"), value.to_owned());
                continue;
            }
            (&mut tags, name)
        } else {
            continue;
        };
        ensure!(
            !name.is_empty() && !name.chars().any(char::is_control),
            "invalid repository reference name"
        );
        map.insert(
            reference.to_owned(),
            GitRef {
                name: name.to_owned(),
                reference: reference.to_owned(),
                oid: value.to_owned(),
            },
        );
    }
    Ok(Advertisement {
        refs: RepositoryRefs {
            default_ref: default_ref.filter(|r| branches.contains_key(r)),
            branches: branches.into_values().collect(),
            tags: tags.into_values().collect(),
        },
        peeled_tags,
    })
}
