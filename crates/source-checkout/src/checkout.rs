//! [INPUT]: 依赖固定清单、上游 Git/LFS 和临时认证目录。
//! [OUTPUT]: 在最终目录发现仓库、冻结标签、下载 LFS 并验证清单。
//! [POS]: 规划 init 分阶段复用同一目录；执行器按固定清单检出。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{
    LfsObject, Options, SourceCredential, SourceManifest, SourceRepository, git::Git, managed_path,
    private_dir, resolve_submodule, same_origin, valid_commit,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submodule {
    pub path: String,
    pub upstream_url: String,
    pub commit_sha: String,
}
#[derive(Clone, Debug)]
pub struct InspectedRepository {
    pub repository: SourceRepository,
    pub submodules: Vec<Submodule>,
}
#[derive(Clone, Debug, Default)]
pub struct Checkout {
    options: Options,
}
#[derive(Clone, Copy, Debug)]
pub enum TagSelection {
    Upstream,
    Frozen,
}
struct FetchOptions {
    tags: TagSelection,
    lfs: bool,
}
impl Checkout {
    pub fn new(options: Options) -> Self {
        Self { options }
    }
    pub(crate) fn options(&self) -> &Options {
        &self.options
    }
    pub async fn checkout(
        &self,
        manifest: &SourceManifest,
        credentials: &[SourceCredential],
        workspace: &Path,
        cancel: &CancellationToken,
    ) -> Result<()> {
        manifest.validate()?;
        ensure!(!workspace.exists(), "source workspace already exists");
        let mut expected = BTreeMap::<String, (String, String)>::new();
        for repo in &manifest.repositories {
            if !repo.path.is_empty() {
                ensure!(
                    expected.remove(&repo.path)
                        == Some((repo.upstream_url.clone(), repo.commit_sha.clone())),
                    "source submodule differs from fixed Git link"
                );
            }
            let inherited = manifest
                .repositories
                .iter()
                .filter(|p| p.path.is_empty() || repo.path.starts_with(&format!("{}/", p.path)))
                .filter_map(|p| {
                    credentials.iter().find(|c| {
                        c.upstream_url == p.upstream_url
                            && same_origin(&p.upstream_url, &repo.upstream_url)
                    })
                })
                .next_back()
                .map(|c| (c.upstream_url.as_str(), &c.credential));
            let target = managed_path(workspace, &repo.path)?;
            self.fetch(
                repo,
                credentials,
                inherited,
                &target,
                FetchOptions {
                    tags: TagSelection::Frozen,
                    lfs: true,
                },
                cancel,
            )
            .await?;
            let git = Git::new(&self.options, &repo.upstream_url, credentials, inherited)?;
            let inspected = inspect(&git, &target, repo, cancel).await?;
            ensure!(
                inspected.repository.lfs_objects == repo.lfs_objects,
                "source LFS identities differ from manifest"
            );
            for child in inspected.submodules {
                let path = if repo.path.is_empty() {
                    child.path
                } else {
                    format!("{}/{}", repo.path, child.path)
                };
                ensure!(
                    expected
                        .insert(path, (child.upstream_url, child.commit_sha))
                        .is_none(),
                    "duplicate source submodule"
                );
            }
            git.close()?;
        }
        ensure!(expected.is_empty(), "source manifest omits a Git submodule");
        self.verify(manifest, workspace, cancel).await
    }
    pub async fn inspect_repository(
        &self,
        repository: &SourceRepository,
        credentials: &[SourceCredential],
        inherited_url: Option<&str>,
        target: &Path,
        selection: TagSelection,
        cancel: &CancellationToken,
    ) -> Result<InspectedRepository> {
        let inherited = inherited_url
            .and_then(|url| credentials.iter().find(|c| c.upstream_url == url))
            .map(|c| (c.upstream_url.as_str(), &c.credential));
        self.fetch(
            repository,
            credentials,
            inherited,
            target,
            FetchOptions {
                tags: selection,
                lfs: true,
            },
            cancel,
        )
        .await?;
        let git = Git::new(
            &self.options,
            &repository.upstream_url,
            credentials,
            inherited,
        )?;
        let result = inspect(&git, target, repository, cancel).await;
        git.close()?;
        result
    }
    /// 检出到最终目录；LFS 留待全部仓库已发现后下载。
    pub async fn discover_repository(
        &self,
        repository: &SourceRepository,
        credentials: &[SourceCredential],
        inherited_url: Option<&str>,
        target: &Path,
        selection: TagSelection,
        cancel: &CancellationToken,
    ) -> Result<InspectedRepository> {
        let inherited = inherited_url
            .and_then(|url| credentials.iter().find(|c| c.upstream_url == url))
            .map(|c| (c.upstream_url.as_str(), &c.credential));
        self.fetch(
            repository,
            credentials,
            inherited,
            target,
            FetchOptions {
                tags: selection,
                lfs: false,
            },
            cancel,
        )
        .await?;
        let git = Git::new(
            &self.options,
            &repository.upstream_url,
            credentials,
            inherited,
        )?;
        let result = inspect(&git, target, repository, cancel).await;
        git.close()?;
        result
    }
    /// 清单只保留祖先 tag，本地引用必须与冻结结果完全一致。
    pub async fn freeze_local_tags(
        &self,
        repo: &SourceRepository,
        credentials: &[SourceCredential],
        inherited_url: Option<&str>,
        target: &Path,
        cancel: &CancellationToken,
    ) -> Result<()> {
        SourceManifest::new(
            uuid::Uuid::new_v4(),
            vec![SourceRepository {
                path: String::new(),
                ..repo.clone()
            }],
        )?;
        let inherited = inherited_url
            .and_then(|url| credentials.iter().find(|c| c.upstream_url == url))
            .map(|c| (c.upstream_url.as_str(), &c.credential));
        let git = Git::new(&self.options, &repo.upstream_url, credentials, inherited)?;
        for (name, _) in tags(&git, target, cancel).await? {
            if !repo.tags.contains_key(&name) {
                git.run(
                    target,
                    &["update-ref", "-d", &format!("refs/tags/{name}")],
                    cancel,
                )
                .await?;
            }
        }
        for (name, oid) in &repo.tags {
            ensure!(valid_commit(oid), "invalid frozen tag object");
            if !git
                .output(target, &["cat-file", "-e", oid], cancel)
                .await?
                .0
                .success()
            {
                git.run(
                    target,
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-recurse-submodules",
                        "--",
                        "origin",
                        oid,
                    ],
                    cancel,
                )
                .await?;
            }
            ensure!(
                git.output(
                    target,
                    &[
                        "merge-base",
                        "--is-ancestor",
                        &format!("{oid}^{{commit}}"),
                        &repo.commit_sha
                    ],
                    cancel
                )
                .await?
                .0
                .success(),
                "frozen tag is not a source ancestor"
            );
            git.run(
                target,
                &["update-ref", &format!("refs/tags/{name}"), oid],
                cancel,
            )
            .await?;
        }
        git.close()?;
        Ok(())
    }
    pub async fn prepare_repository_lfs(
        &self,
        repo: &SourceRepository,
        credentials: &[SourceCredential],
        inherited_url: Option<&str>,
        target: &Path,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let inherited = inherited_url
            .and_then(|url| credentials.iter().find(|c| c.upstream_url == url))
            .map(|c| (c.upstream_url.as_str(), &c.credential));
        let git = Git::new(&self.options, &repo.upstream_url, credentials, inherited)?;
        let result = download_lfs(&git, target, &repo.commit_sha, &repo.lfs_objects, cancel).await;
        git.close()?;
        result
    }
    async fn fetch(
        &self,
        repo: &SourceRepository,
        credentials: &[SourceCredential],
        inherited: Option<(&str, &crate::GitCredential)>,
        target: &Path,
        options: FetchOptions,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let FetchOptions {
            tags,
            lfs: prepare_lfs,
        } = options;
        ensure!(valid_commit(&repo.commit_sha), "invalid pinned object");
        ensure!(
            !target.exists() || (target.is_dir() && fs::read_dir(target)?.next().is_none()),
            "source directory is not empty"
        );
        private_dir(target)?;
        let git = Git::new(&self.options, &repo.upstream_url, credentials, inherited)?;
        let format = if repo.commit_sha.len() == 64 {
            "--object-format=sha256"
        } else {
            "--object-format=sha1"
        };
        git.run(target, &["init", "--template=", format, "."], cancel)
            .await?;
        git.run(
            target,
            &["remote", "add", "origin", &repo.upstream_url],
            cancel,
        )
        .await?;
        // 同次 fetch 带回根提交与 tag；不创建第二棵临时工作树。
        git.run(
            target,
            &[
                "fetch",
                if matches!(tags, TagSelection::Upstream) {
                    "--tags"
                } else {
                    "--no-tags"
                },
                "--no-recurse-submodules",
                "--force",
                "--",
                "origin",
                &repo.commit_sha,
            ],
            cancel,
        )
        .await?;
        if matches!(tags, TagSelection::Frozen) {
            for (name, oid) in &repo.tags {
                ensure!(valid_commit(oid), "invalid frozen tag object");
                if !git
                    .output(target, &["cat-file", "-e", oid], cancel)
                    .await?
                    .0
                    .success()
                {
                    git.run(
                        target,
                        &[
                            "fetch",
                            "--no-tags",
                            "--no-recurse-submodules",
                            "--",
                            "origin",
                            oid,
                        ],
                        cancel,
                    )
                    .await?;
                }
                git.run(
                    target,
                    &["update-ref", &format!("refs/tags/{name}"), oid],
                    cancel,
                )
                .await?;
            }
        }
        let commit = String::from_utf8(
            git.run(
                target,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", repo.commit_sha),
                ],
                cancel,
            )
            .await?,
        )?
        .trim()
        .to_owned();
        git.run(target, &["checkout", "--detach", &commit], cancel)
            .await?;
        validate_lfs_config(&git, target, cancel).await?;
        let lfs = lfs_objects(&git, target, &commit, cancel).await?;
        if prepare_lfs {
            download_lfs(&git, target, &commit, &lfs, cancel).await?;
        }
        git.close()?;
        Ok(())
    }
    pub async fn peel(
        &self,
        repository: &str,
        oid: &str,
        credentials: &[SourceCredential],
        target: &Path,
        cancel: &CancellationToken,
    ) -> Result<String> {
        let repo = SourceRepository {
            path: String::new(),
            upstream_url: repository.to_owned(),
            commit_sha: oid.to_owned(),
            tags: BTreeMap::new(),
            lfs_objects: Vec::new(),
        };
        self.fetch(
            &repo,
            credentials,
            None,
            target,
            FetchOptions {
                tags: TagSelection::Frozen,
                lfs: true,
            },
            cancel,
        )
        .await?;
        let git = Git::new(&self.options, repository, credentials, None)?;
        let result = String::from_utf8(git.run(target, &["rev-parse", "HEAD"], cancel).await?)?
            .trim()
            .to_owned();
        git.close()?;
        Ok(result)
    }
    pub async fn verify(
        &self,
        manifest: &SourceManifest,
        workspace: &Path,
        cancel: &CancellationToken,
    ) -> Result<()> {
        manifest.validate()?;
        for repo in &manifest.repositories {
            let target = managed_path(workspace, &repo.path)?;
            let mut git = Git::new(&self.options, &repo.upstream_url, &[], None)?;
            git.isolate_index();
            let head = String::from_utf8(git.run(&target, &["rev-parse", "HEAD"], cancel).await?)?;
            ensure!(
                head.trim() == repo.commit_sha,
                "source_modified: pinned source commit changed"
            );
            git.run(&target, &["read-tree", "--reset", &repo.commit_sha], cancel)
                .await?;
            let (status, _) = git
                .output(
                    &target,
                    &[
                        "-c",
                        if cfg!(windows) {
                            "core.filemode=false"
                        } else {
                            "core.filemode=true"
                        },
                        "-c",
                        "core.fsmonitor=false",
                        "diff",
                        "--exit-code",
                        "--no-ext-diff",
                        "--ignore-submodules=untracked",
                        "HEAD",
                        "--",
                    ],
                    cancel,
                )
                .await?;
            ensure!(
                status.success(),
                "source_modified: tracked files or submodule state changed"
            );
            ensure!(
                tags(&git, &target, cancel).await? == repo.tags,
                "source_modified: frozen tags changed"
            );
            for object in &repo.lfs_objects {
                verify_file(&managed_path(&target, &object.path)?, object).await?;
            }
            git.close()?;
        }
        Ok(())
    }
}
pub async fn verify(
    manifest: &SourceManifest,
    workspace: &Path,
    cancel: &CancellationToken,
) -> Result<()> {
    let checkout = Checkout::default();
    checkout.verify(manifest, workspace, cancel).await
}
async fn tags(
    git: &Git,
    target: &Path,
    cancel: &CancellationToken,
) -> Result<BTreeMap<String, String>> {
    let data = String::from_utf8(
        git.run(
            target,
            &[
                "for-each-ref",
                "--format=%(refname:strip=2) %(objectname)",
                "refs/tags",
            ],
            cancel,
        )
        .await?,
    )?;
    data.lines()
        .map(|line| {
            let (name, oid) = line.split_once(' ').context("invalid tag metadata")?;
            Ok((name.to_owned(), oid.to_owned()))
        })
        .collect()
}
async fn inspect(
    git: &Git,
    target: &Path,
    repo: &SourceRepository,
    cancel: &CancellationToken,
) -> Result<InspectedRepository> {
    let commit = String::from_utf8(git.run(target, &["rev-parse", "HEAD"], cancel).await?)?
        .trim()
        .to_owned();
    ensure!(valid_commit(&commit), "invalid inspected commit");
    let mut repository = repo.clone();
    repository.commit_sha = commit.clone();
    repository.tags = tags(git, target, cancel).await?;
    // 只有当前提交祖先上的 tag 进入固定清单。
    let mut retained = BTreeMap::new();
    for (name, oid) in &repository.tags {
        if git
            .output(
                target,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &format!("{oid}^{{commit}}"),
                    &commit,
                ],
                cancel,
            )
            .await?
            .0
            .success()
        {
            retained.insert(name.clone(), oid.clone());
        }
    }
    repository.tags = retained;
    repository.lfs_objects = lfs_objects(git, target, &commit, cancel).await?;
    let tree = tree(git, target, &commit, cancel).await?;
    let mut modules = BTreeMap::<String, BTreeMap<String, String>>::new();
    if git
        .output(
            target,
            &["cat-file", "-e", &format!("{commit}:.gitmodules")],
            cancel,
        )
        .await?
        .0
        .success()
    {
        let data = git
            .run(
                target,
                &[
                    "config",
                    "--null",
                    "--blob",
                    &format!("{commit}:.gitmodules"),
                    "--get-regexp",
                    r"^submodule\..*\.(path|url)$",
                ],
                cancel,
            )
            .await?;
        for entry in data.split(|b| *b == 0).filter(|b| !b.is_empty()) {
            let entry = std::str::from_utf8(entry)?;
            let (key, value) = entry
                .split_once('\n')
                .context("invalid submodule configuration")?;
            let (name, field) = key.rsplit_once('.').context("invalid submodule key")?;
            modules
                .entry(name.to_owned())
                .or_default()
                .insert(field.to_owned(), value.to_owned());
        }
    }
    let mut by_path = BTreeMap::new();
    for module in modules.values() {
        let path = module.get("path").context("submodule path missing")?;
        managed_path(target, path)?;
        ensure!(
            by_path
                .insert(
                    path.clone(),
                    module.get("url").context("submodule URL missing")?.clone()
                )
                .is_none(),
            "duplicate submodule path"
        );
    }
    let mut submodules = Vec::new();
    for entry in tree {
        if entry.mode == "160000" {
            submodules.push(Submodule {
                path: entry.path.clone(),
                upstream_url: resolve_submodule(
                    &repo.upstream_url,
                    by_path
                        .get(&entry.path)
                        .context("Git link has no submodule URL")?,
                )?,
                commit_sha: entry.oid,
            });
        }
    }
    ensure!(submodules.len() <= 128, "too many submodules");
    Ok(InspectedRepository {
        repository,
        submodules,
    })
}
struct Entry {
    mode: String,
    path: String,
    oid: String,
    size: Option<u64>,
}
async fn tree(
    git: &Git,
    target: &Path,
    commit: &str,
    cancel: &CancellationToken,
) -> Result<Vec<Entry>> {
    let data = git
        .run(target, &["ls-tree", "-rlz", "--full-tree", commit], cancel)
        .await?;
    data.split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .map(|line| {
            let line = std::str::from_utf8(line)?;
            let (metadata, path) = line.split_once('\t').context("invalid Git tree")?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            ensure!(fields.len() == 4, "invalid Git tree entry");
            crate::manifest::validate_path(path)?;
            Ok(Entry {
                mode: fields[0].to_owned(),
                path: path.to_owned(),
                oid: fields[2].to_owned(),
                size: if fields[3] == "-" {
                    None
                } else {
                    Some(fields[3].parse()?)
                },
            })
        })
        .collect()
}
async fn lfs_objects(
    git: &Git,
    target: &Path,
    commit: &str,
    cancel: &CancellationToken,
) -> Result<Vec<LfsObject>> {
    let entries = tree(git, target, commit, cancel).await?;
    let mut objects = Vec::new();
    let candidates = entries
        .into_iter()
        .filter(|entry| {
            matches!(entry.mode.as_str(), "100644" | "100755")
                && entry.size.is_some_and(|n| n <= 1024)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(objects);
    }
    let input = candidates
        .iter()
        .map(|entry| format!("{}\n", entry.oid))
        .collect::<String>();
    let (status, output) = git
        .output_with_input(
            target,
            &["cat-file", "--batch"],
            Some(input.as_bytes()),
            cancel,
        )
        .await?;
    ensure!(status.success(), "cannot inspect source pointer blobs");
    let mut data = output.as_slice();
    for entry in candidates {
        let newline = data
            .iter()
            .position(|b| *b == b'\n')
            .context("invalid Git blob output")?;
        let header = std::str::from_utf8(&data[..newline])?
            .split_whitespace()
            .collect::<Vec<_>>();
        ensure!(
            header.len() == 3 && header[0] == entry.oid && header[1] == "blob",
            "invalid Git blob identity"
        );
        let size: usize = header[2].parse()?;
        data = &data[newline + 1..];
        ensure!(
            data.len() > size && data[size] == b'\n',
            "incomplete Git blob"
        );
        let blob = &data[..size];
        data = &data[size + 1..];
        if !blob.starts_with(b"version https://git-lfs.github.com/spec/v1\n") {
            continue;
        }
        let text = std::str::from_utf8(blob)?;
        let oid = text
            .lines()
            .find_map(|line| line.strip_prefix("oid sha256:"))
            .context("LFS pointer has no OID")?;
        let size = text
            .lines()
            .find_map(|line| line.strip_prefix("size "))
            .context("LFS pointer has no size")?
            .parse()?;
        ensure!(oid.len() == 64 && valid_commit(oid), "invalid LFS OID");
        objects.push(LfsObject {
            path: entry.path,
            oid: oid.to_owned(),
            size,
        });
    }
    objects.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(objects)
}
async fn validate_lfs_config(git: &Git, target: &Path, cancel: &CancellationToken) -> Result<()> {
    if git
        .output(target, &["cat-file", "-e", "HEAD:.lfsconfig"], cancel)
        .await?
        .0
        .success()
    {
        let (status, values) = git
            .output(
                target,
                &[
                    "config",
                    "--blob",
                    "HEAD:.lfsconfig",
                    "--get-regexp",
                    r"(^lfs\.(url|pushurl)$|^remote\..*\.lfsurl$)",
                ],
                cancel,
            )
            .await?;
        ensure!(
            status.code() == Some(1) || (status.success() && values.is_empty()),
            "custom LFS endpoints are not authorized"
        );
    }
    Ok(())
}
async fn verify_file(path: &Path, object: &LfsObject) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && meta.len() == object.size,
        "source_modified: LFS file size changed"
    );
    let mut file = tokio::fs::File::open(path).await?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let size = file.read(&mut buffer).await?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    ensure!(
        hex::encode(hash.finalize()) == object.oid,
        "source_modified: LFS file digest changed"
    );
    Ok(())
}
async fn download_lfs(
    git: &Git,
    target: &Path,
    commit: &str,
    lfs: &[LfsObject],
    cancel: &CancellationToken,
) -> Result<()> {
    if !lfs.is_empty() {
        for (key, value) in [
            ("filter.lfs.clean", "git-lfs clean -- %f"),
            ("filter.lfs.smudge", "git-lfs smudge -- %f"),
            ("filter.lfs.process", "git-lfs filter-process"),
            ("filter.lfs.required", "true"),
        ] {
            git.run(target, &["config", "--local", key, value], cancel)
                .await?;
        }
        git.run(target, &["lfs", "fetch", "origin", commit], cancel)
            .await?;
        git.run(target, &["lfs", "checkout"], cancel).await?;
        for object in lfs {
            verify_file(&managed_path(target, &object.path)?, object).await?;
        }
        git.run(target, &["lfs", "fsck", "--objects", commit], cancel)
            .await?;
    }
    Ok(())
}
