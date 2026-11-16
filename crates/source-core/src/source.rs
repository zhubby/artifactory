// [INPUT]: 依赖源码存储和运行时凭据加密。
// [OUTPUT]: 提供仓库校验和凭据读取。
// [POS]: 统一源码服务与控制平面的仓库输入边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::{AppState, crypto};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitCredential {
    pub kind: String,
    #[serde(default)]
    pub username: String,
    pub secret: String,
    #[serde(default)]
    pub known_hosts: String,
}

pub fn validate_repository(repository: &str) -> Result<()> {
    ensure!(
        repository.len() <= 2048 && !repository.contains(['\0', '\n', '\r']),
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
            .context("only SSH, HTTPS and HTTP Git repositories are supported")?;
        ensure!(
            host.contains('@')
                && !host.starts_with('-')
                && host
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"@._-".contains(&value)),
            "invalid SSH repository"
        );
        ensure!(
            !path.is_empty() && !path.starts_with('-'),
            "invalid SSH path"
        );
    }
    Ok(())
}

pub async fn credential(state: &AppState, id: Option<uuid::Uuid>) -> Result<Option<GitCredential>> {
    if let Some(id) = id {
        let encrypted = state.store.credential_ciphertext(id).await?;
        Ok(Some(serde_json::from_slice(&crypto::open(
            &state.settings.master_key,
            &encrypted,
        )?)?))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_repository;

    #[test]
    fn accepts_http_repository_with_external_credentials() {
        assert!(
            validate_repository(
                "http://codehub.longshine.com/git/gl-longshine/ai-workspace/ai-workspace.git"
            )
            .is_ok()
        );
    }

    #[test]
    fn rejects_web_repository_credentials_and_url_metadata() {
        for scheme in ["http", "https"] {
            for suffix in [
                "user@example.com/app.git",
                "user:secret@example.com/app.git",
                "example.com/app.git?token=secret",
                "example.com/app.git#secret",
            ] {
                let repository = format!("{scheme}://{suffix}");
                assert!(validate_repository(&repository).is_err(), "{repository}");
            }
        }
    }

    #[test]
    fn preserves_ssh_and_https_repository_support() {
        for repository in [
            "https://example.com/org/app.git",
            "ssh://git@example.com:2222/org/app.git",
            "git@example.com:org/app.git",
        ] {
            assert!(validate_repository(repository).is_ok(), "{repository}");
        }
        for repository in [
            "file:///etc/passwd",
            "git://example.com/app.git",
            "ext::command",
            "-oProxyCommand=x:path",
        ] {
            assert!(validate_repository(repository).is_err(), "{repository}");
        }
    }
}
