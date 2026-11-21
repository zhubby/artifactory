//! [INPUT]: 依赖 buildctl 标准错误、退出结果与共享脱敏事件。
//! [OUTPUT]: 转发脱敏日志，保留有界末尾诊断并输出正式失败类型。
//! [POS]: 只按 buildctl 末尾求解诊断判定修订，忽略容器普通输出。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use artifactory_execution_support::{events::Emitter, redact};
use artifactory_image_protocol::FailureKind;
use tokio::io::AsyncRead;

#[derive(Debug)]
struct BuildFailure {
    kind: Option<FailureKind>,
    diagnostic: String,
}
impl std::fmt::Display for BuildFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "BuildKit failed: {}", self.diagnostic)
    }
}
impl std::error::Error for BuildFailure {}

pub(crate) fn kind(error: &anyhow::Error) -> Option<FailureKind> {
    error
        .downcast_ref::<BuildFailure>()
        .and_then(|error| error.kind)
}

pub(crate) fn failure(stderr: &str) -> anyhow::Error {
    let diagnostic = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    BuildFailure {
        kind: classify(diagnostic),
        diagnostic: if diagnostic.is_empty() {
            "buildctl returned no final diagnostic; inspect build logs".into()
        } else {
            diagnostic.to_owned()
        },
    }
    .into()
}

fn classify(diagnostic: &str) -> Option<FailureKind> {
    let lower = diagnostic.to_ascii_lowercase();
    let diagnostic = lower.strip_prefix("error: failed to solve: ")?;
    if [
        "failed to authorize",
        "unauthorized",
        "authentication required",
        "insufficient_scope",
        "no basic auth credentials",
        "forbidden",
        "proxy authentication required",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
    {
        return Some(FailureKind::Authorization);
    }
    if [
        "context canceled",
        "context deadline exceeded",
        "connection refused",
        "dial tcp",
        "i/o timeout",
        "tls handshake timeout",
        "no such host",
        "network is unreachable",
        "unexpected eof",
        "transport is closing",
        "code = unavailable",
        "no space left on device",
        "resource exhausted",
        "out of memory",
        "cannot allocate memory",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
    {
        return Some(FailureKind::Infrastructure);
    }
    if [
        "failed to push",
        "failed to export",
        "failed to resolve source metadata",
        "failed to fetch",
        "failed to pull",
        "failed to do request",
        "failed to copy",
    ]
    .iter()
    .any(|marker| diagnostic.contains(marker))
    {
        return Some(FailureKind::ArtifactTransport);
    }
    if diagnostic.contains("process \"") {
        let code = diagnostic
            .rsplit_once("did not complete successfully: exit code: ")?
            .1
            .parse::<u32>()
            .ok()?;
        return match code {
            0 => None,
            137 | 143 => Some(FailureKind::Infrastructure),
            _ => Some(FailureKind::Execution),
        };
    }
    if diagnostic.contains("dockerfile parse error")
        || diagnostic.contains("unknown instruction:")
        || diagnostic.contains("circular dependency detected")
        || diagnostic.contains("no build stage in current context")
        || (diagnostic.contains("failed to compute cache key")
            && diagnostic.contains("failed to calculate checksum")
            && diagnostic.contains("not found"))
        || (diagnostic.contains("failed to read dockerfile")
            && diagnostic.contains("no such file or directory"))
    {
        return Some(FailureKind::Execution);
    }
    None
}

pub(crate) async fn stderr(
    reader: impl AsyncRead + Unpin,
    emit: Emitter,
    secrets: redact::Secrets,
) -> Result<String> {
    let (redacted, mut events) = Emitter::channel();
    let (_, tail) = tokio::try_join!(
        redact::read_log(reader, "buildkit", redacted, secrets),
        async {
            let mut tail = Vec::new();
            let mut truncated = false;
            while let Some(event) = events.recv().await {
                tail.extend_from_slice(&event.data);
                if tail.len() > crate::diagnostics::LOG_LIMIT {
                    tail.drain(..tail.len() - crate::diagnostics::LOG_LIMIT);
                    truncated = true;
                }
                emit.send(event).await?;
            }
            // 截断后的首行不是完整诊断，禁止把中间字节解释为 CLI 前缀。
            if truncated {
                let start = tail
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(tail.len(), |offset| offset + 1);
                tail.drain(..start);
            }
            Ok::<_, anyhow::Error>(String::from_utf8_lossy(&tail).into_owned())
        }
    )?;
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_solver_execution_errors_are_revisionable() {
        for diagnostic in [
            "error: failed to solve: process \"/bin/sh -c make\" did not complete successfully: exit code: 2",
            "error: failed to solve: dockerfile parse error on line 3: unknown instruction: RNU",
            "error: failed to solve: failed to compute cache key: failed to calculate checksum of ref abc: \"/missing\": not found",
            "error: failed to solve: failed to read dockerfile: open Dockerfile: no such file or directory",
        ] {
            assert_eq!(
                kind(&failure(diagnostic)),
                Some(FailureKind::Execution),
                "{diagnostic}"
            );
        }
    }

    #[test]
    fn external_failures_and_container_output_do_not_consume_revisions() {
        for diagnostic in [
            "error: failed to solve: failed to authorize: failed to fetch oauth token: unexpected status: 401 Unauthorized",
            "error: failed to solve: failed to push registry/app: upload failed",
            "error: failed to solve: failed to resolve source metadata: image:latest: not found",
            "error: failed to solve: rpc error: code = Unavailable desc = connection refused",
            "error: failed to solve: process \"make\" did not complete successfully: exit code: 137",
            "error: failed to solve: context canceled",
            "error: failed to solve: process \"make\" did not complete successfully: no space left on device",
            "#5 0.001 error: failed to solve: process \"make\" did not complete successfully: exit code: 2",
            "arbitrary output that is not a CLI diagnostic",
            "",
        ] {
            assert_ne!(
                kind(&failure(diagnostic)),
                Some(FailureKind::Execution),
                "{diagnostic}"
            );
        }
        let error = failure(
            "#3 0.1 error: failed to solve: process \"make\" did not complete successfully: exit code: 2\nerror: failed to solve: failed to push registry/app: upload failed",
        );
        assert_eq!(kind(&error), Some(FailureKind::ArtifactTransport));
    }

    #[tokio::test]
    async fn captures_only_bounded_redacted_tail_and_forwards_full_logs() {
        let secret = "private-build-password";
        let raw = format!(
            "{}\nerror: failed to solve: process \"make {secret}\" did not complete successfully: exit code: 2\n",
            "x".repeat(20000)
        );
        let mut secrets = redact::Secrets::default();
        secrets.add(secret);
        let (emit, mut events) = Emitter::channel();
        let (tail, full) = tokio::join!(stderr(raw.as_bytes(), emit, secrets), async {
            let mut full = Vec::new();
            while let Some(event) = events.recv().await {
                full.extend(event.data);
            }
            String::from_utf8(full).unwrap()
        });
        let tail = tail.unwrap();
        assert!(tail.len() <= crate::diagnostics::LOG_LIMIT);
        assert!(full.len() > 20000);
        assert!(!tail.contains(secret) && !full.contains(secret));
        assert!(tail.contains("[REDACTED]"));
        assert_eq!(kind(&failure(&tail)), Some(FailureKind::Execution));
    }

    #[tokio::test]
    async fn closed_log_destination_stops_capture_under_backpressure() {
        let raw = vec![b'x'; 4 * 1024 * 1024];
        let (emit, events) = Emitter::channel();
        drop(events);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stderr(raw.as_slice(), emit, redact::Secrets::default()),
        )
        .await
        .expect("closed log destination must stop the reader");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reports_typed_execution_only_for_build_workers() {
        use std::sync::{Arc, Mutex, atomic::AtomicI64};
        let reports = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let observed = reports.clone();
        let app = axum::Router::new().route(
            "/task/report",
            axum::routing::post(move |axum::Json(value): axum::Json<serde_json::Value>| {
                let observed = observed.clone();
                async move {
                    observed.lock().unwrap().push(value);
                    axum::Json(serde_json::json!({"ok":true}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control = crate::transport::Control::new(
            format!("http://{}/task", listener.local_addr().unwrap()),
            "fixture-token".into(),
            true,
        )
        .unwrap();
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let diagnostic =
            "error: failed to solve: process \"make\" did not complete successfully: exit code: 2";
        for (platform, error) in [
            ("linux/amd64", failure(diagnostic).context("build worker")),
            ("publish", failure(diagnostic)),
            ("linux/amd64", anyhow::anyhow!(diagnostic)),
        ] {
            control
                .failure(
                    platform,
                    &error,
                    redact::Secrets::default(),
                    &AtomicI64::new(0),
                )
                .await;
        }
        let reports = reports.lock().unwrap();
        let failures: Vec<_> = reports
            .iter()
            .filter(|report| report["kind"] == "failed")
            .collect();
        assert_eq!(failures.len(), 3);
        assert_eq!(failures[0]["failure_kind"], "execution");
        assert!(failures[1]["failure_kind"].is_null());
        assert!(
            failures[2]["failure_kind"].is_null(),
            "message text cannot substitute for the typed error"
        );
        server.abort();
    }
}
