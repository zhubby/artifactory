//! [INPUT]: 依赖真实隔离 Git 进程和伪造的敏感 stderr。
//! [OUTPUT]: 验证失败分类、输出上限和超时不泄露原始诊断。
//! [POS]: 源码故障公开边界的进程级回归。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#![cfg(unix)]

use artifactory_source_checkout::{Checkout, GitFailure, GitFailureKind, Options};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio_util::sync::CancellationToken;

async fn failure(stderr: &str, pause: bool) -> anyhow::Result<anyhow::Error> {
    let root = tempfile::tempdir()?;
    let git = root.path().join("git");
    let script = if pause {
        "#!/bin/sh\nsleep 10\n".to_owned()
    } else {
        format!("#!/bin/sh\ncat <<'DIAGNOSTIC' >&2\n{stderr}\nDIAGNOSTIC\nexit 128\n")
    };
    std::fs::write(&git, script)?;
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700))?;
    let checkout = Checkout::new(Options {
        git: git.to_string_lossy().into_owned(),
        auth_root: root.path().join("auth"),
        timeout: if pause {
            Duration::from_millis(150)
        } else {
            Duration::from_secs(10)
        },
        ..Default::default()
    });
    Ok(checkout
        .list_refs(
            "https://example.com/repo.git",
            &[],
            &CancellationToken::new(),
        )
        .await
        .expect_err("fake Git must fail"))
}

#[tokio::test]
async fn git_failures_are_classified_without_returning_credentials() -> anyhow::Result<()> {
    for (message, kind) in [
        (
            "Could not resolve host: private.example",
            GitFailureKind::Dns,
        ),
        (
            "Failed to connect to private.example port 443",
            GitFailureKind::Connection,
        ),
        (
            "Authentication failed for https://user:super-secret@private.example",
            GitFailureKind::Authentication,
        ),
        (
            "SSL certificate problem: self-signed certificate",
            GitFailureKind::Tls,
        ),
        (
            "fatal: couldn't find remote ref main",
            GitFailureKind::Reference,
        ),
        (
            "private unknown failure with token=super-secret",
            GitFailureKind::Unknown,
        ),
    ] {
        let error = failure(message, false).await?;
        assert_eq!(
            error.downcast_ref::<GitFailure>().map(GitFailure::kind),
            Some(kind)
        );
        let visible = format!("{error:#}");
        for secret in ["super-secret", "private.example", "user:", "token="] {
            assert!(!visible.contains(secret), "diagnostic leaked {secret}");
        }
    }
    Ok(())
}

#[tokio::test]
async fn timeout_keeps_its_safe_classification_and_kills_the_process() -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    let error = failure("", true).await?;
    assert_eq!(
        error.downcast_ref::<GitFailure>().map(GitFailure::kind),
        Some(GitFailureKind::Timeout)
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "Git process was not reaped"
    );
    Ok(())
}

#[tokio::test]
async fn large_stderr_is_drained_without_leaking_or_deadlocking() -> anyhow::Result<()> {
    let stderr = format!("Authentication failed\n{}", "super-secret".repeat(200_000));
    let error = failure(&stderr, false).await?;
    assert_eq!(
        error.downcast_ref::<GitFailure>().map(GitFailure::kind),
        Some(GitFailureKind::Authentication)
    );
    assert!(!format!("{error:#}").contains("super-secret"));
    Ok(())
}
