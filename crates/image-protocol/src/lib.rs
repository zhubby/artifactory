//! [INPUT]: 依赖固定清单身份、Serde 和 SHA-256。
//! [OUTPUT]: 提供镜像目标、紧凑执行材料与 OCI Index 契约。
//! [POS]: 控制平面与 Image Operator 共用的执行边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub const SUPPORTED_PLATFORMS: [&str; 2] = ["linux/amd64", "linux/arm64"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageSpec {
    pub name: String,
    #[serde(default = "context")]
    pub context: String,
    #[serde(default = "dockerfile")]
    pub dockerfile: String,
    #[serde(default = "platforms")]
    pub platforms: Vec<String>,
    #[serde(default = "pool")]
    pub pool: String,
    #[serde(default)]
    pub allow_emulation: bool,
    #[serde(default)]
    pub build_args: BTreeMap<String, String>,
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    #[serde(default)]
    pub pull_credentials: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
fn context() -> String {
    ".".into()
}
fn dockerfile() -> String {
    "Dockerfile".into()
}
fn pool() -> String {
    "default".into()
}
fn platforms() -> Vec<String> {
    SUPPORTED_PLATFORMS.iter().map(|s| (*s).into()).collect()
}

impl ImageSpec {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            repository(&self.name) && !self.name.contains('/'),
            "invalid image name"
        );
        ensure!(
            relative(&self.context, true) && relative(&self.dockerfile, false),
            "unsafe image source path"
        );
        let unique: BTreeSet<_> = self.platforms.iter().collect();
        ensure!(
            !unique.is_empty()
                && unique.len() == self.platforms.len()
                && unique
                    .iter()
                    .all(|s| SUPPORTED_PLATFORMS.contains(&s.as_str())),
            "expected unique Linux amd64/arm64 platforms"
        );
        ensure!(dns_label(&self.pool), "invalid builder pool");
        ensure!(
            self.tags.len() <= 16 && self.tags.iter().all(|s| tag(s)),
            "invalid image tags"
        );
        ensure!(
            self.build_args.len() <= 64
                && self
                    .build_args
                    .iter()
                    .all(|(k, v)| parameter(k) && v.len() <= 4096 && !v.contains('\0')),
            "invalid build arguments"
        );
        ensure!(
            self.secrets.len() <= 32
                && self
                    .secrets
                    .iter()
                    .all(|(k, v)| parameter(k) && uuid::Uuid::parse_str(v).is_ok()),
            "invalid build secret references"
        );
        ensure!(
            self.pull_credentials.len() <= 16
                && self
                    .pull_credentials
                    .iter()
                    .all(|v| uuid::Uuid::parse_str(v).is_ok()),
            "invalid registry credential references"
        );
        Ok(())
    }
}
fn parameter(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
pub fn dns_label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 63
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
}
pub fn relative(s: &str, dot: bool) -> bool {
    (dot && s == ".")
        || (!s.is_empty()
            && s.len() <= 1024
            && !s.contains(['\\', ':', '\0', '\r', '\n'])
            && s.split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".." && !p.eq_ignore_ascii_case(".git")))
}
pub fn repository(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && s.split('/').all(|p| {
            !p.is_empty()
                && p.as_bytes()[0].is_ascii_alphanumeric()
                && p.as_bytes()[p.len() - 1].is_ascii_alphanumeric()
                && p.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"._-".contains(&c))
        })
}
pub fn tag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && (s.as_bytes()[0].is_ascii_alphanumeric() || s.starts_with('_'))
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
}
pub fn digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// 凭据只进入认证服务接口与 Secret 卷。
#[derive(Clone, Serialize, Deserialize)]
pub struct RegistryLogin {
    pub endpoint: String,
    pub username: String,
    pub password: String,
}
impl RegistryLogin {
    pub fn validate(&self, insecure: bool) -> Result<()> {
        let url = url::Url::parse(&self.endpoint)?;
        ensure!(
            (url.scheme() == "https" || (insecure && url.scheme() == "http"))
                && url.host_str().is_some()
                && url.path() == "/"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "registry requires an HTTPS origin"
        );
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildTask {
    pub execution_id: String,
    pub token: String,
    pub control_url: String,
    pub registry: RegistryLogin,
    pub repository: String,
    pub image: ImageSpec,
    pub source_manifest_id: uuid::Uuid,
    pub source_digest: String,
    pub timeout_seconds: u32,
    #[serde(default)]
    pub docker_auth: serde_json::Value,
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
    pub insecure: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformResult {
    pub platform: String,
    pub digest: String,
    pub size: u64,
    pub media_type: String,
}
impl PlatformResult {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            SUPPORTED_PLATFORMS.contains(&self.platform.as_str())
                && digest(&self.digest)
                && self.size > 0
                && self.size <= 4 * 1024 * 1024
                && self.media_type == MANIFEST_MEDIA_TYPE,
            "invalid platform result"
        );
        Ok(())
    }
}

pub fn index(expected: &[String], results: &[PlatformResult]) -> Result<Vec<u8>> {
    ensure!(expected.len() == results.len(), "missing platform manifest");
    let mut seen = BTreeSet::new();
    let mut manifests = Vec::new();
    for p in expected {
        ensure!(
            seen.insert(p) && SUPPORTED_PLATFORMS.contains(&p.as_str()),
            "duplicate or invalid platform"
        );
        let r = results
            .iter()
            .find(|r| &r.platform == p)
            .ok_or_else(|| anyhow::anyhow!("missing platform manifest"))?;
        r.validate()?;
        let (os, arch) = p.split_once('/').unwrap();
        manifests.push(serde_json::json!({"mediaType":r.media_type,"digest":r.digest,"size":r.size,"platform":{"os":os,"architecture":arch}}));
    }
    Ok(serde_json::to_vec(
        &serde_json::json!({"schemaVersion":2,"mediaType":INDEX_MEDIA_TYPE,"manifests":manifests}),
    )?)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishTask {
    pub execution_id: String,
    pub token: String,
    pub control_url: String,
    pub registry: RegistryLogin,
    pub source_repository: String,
    pub destination_repository: String,
    pub tags: Vec<String>,
    pub platforms: Vec<String>,
    pub insecure: bool,
    #[serde(default)]
    pub destination: Option<RegistryLogin>,
    #[serde(default)]
    pub root_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Report {
    Heartbeat {
        platform: String,
        pod_uid: String,
    },
    Log {
        platform: String,
        sequence: i64,
        text: String,
    },
    Platform {
        result: PlatformResult,
    },
    Failed {
        message: String,
    },
    Published {
        digest: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_requires_every_requested_platform_and_exact_digests() {
        let expected = platforms();
        let mut results: Vec<_> = expected
            .iter()
            .map(|p| PlatformResult {
                platform: p.clone(),
                digest: sha256(p.as_bytes()),
                size: 42,
                media_type: MANIFEST_MEDIA_TYPE.into(),
            })
            .collect();
        assert!(index(&expected, &results[..1]).is_err());
        let value: serde_json::Value =
            serde_json::from_slice(&index(&expected, &results).unwrap()).unwrap();
        assert_eq!(value["manifests"][1]["platform"]["architecture"], "arm64");
        results[1].digest = "sha256:truncated".into();
        assert!(index(&expected, &results).is_err());
    }
    #[test]
    fn paths_and_references_do_not_accept_escapes_or_credentials() {
        for p in [
            "../private",
            "/etc",
            "a/../b",
            "a\\b",
            ".git/config",
            "a\nb",
            "",
        ] {
            assert!(!relative(p, true));
        }
        assert!(relative(".", true));
        for p in ["host/app:tag", "a/../b", "User/app", "a//b"] {
            assert!(!repository(p));
        }
        assert!(repository("tenant/project/images/app"));
    }
}
