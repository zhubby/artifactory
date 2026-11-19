//! [INPUT]: 依赖任务秘密、代理凭据、错误链和共享流式脱敏器。
//! [OUTPUT]: 先清除诊断 URL，再替换任务秘密，提供有界日志与摘要。
//! [POS]: Runner 与 Publisher 共用的错误持久化边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::Result;
use artifactory_execution_support::{events::Emitter, redact};
use artifactory_image_protocol::RegistryLogin;
use base64::Engine;

pub(crate) const LOG_LIMIT: usize = 16384;
pub(crate) const SUMMARY_LIMIT: usize = 1024;

pub(crate) fn proxy_secrets(secrets: &mut redact::Secrets) {
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
        if let Ok(value) = std::env::var(name) {
            proxy_value_secrets(secrets, &value);
        }
    }
}

fn proxy_value_secrets(secrets: &mut redact::Secrets, value: &str) {
    secrets.add(value);
    if let Ok(url) = url::Url::parse(value) {
        let decode = |value: &str| {
            let encoded = format!("value={}", value.replace('+', "%2B"));
            url::form_urlencoded::parse(encoded.as_bytes())
                .next()
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default()
        };
        let username = decode(url.username());
        secrets.add(url.username());
        secrets.add(&username);
        if !url.username().is_empty() || url.password().is_some() {
            let password = url.password().unwrap_or_default();
            secrets.add(password);
            let password = decode(password);
            secrets.add(&password);
            let credentials = format!("{username}:{password}");
            secrets.add(&credentials);
            secrets.add(&base64::engine::general_purpose::STANDARD.encode(credentials));
        }
    }
}

pub(crate) fn registry_secrets(secrets: &mut redact::Secrets, login: &RegistryLogin) {
    secrets.add(&login.password);
    let credentials = format!("{}:{}", login.username, login.password);
    secrets.add(&credentials);
    secrets.add(&base64::engine::general_purpose::STANDARD.encode(credentials));
}

pub(crate) fn bounded(text: &str, maximum: usize) -> String {
    text[..text.floor_char_boundary(text.len().min(maximum))].to_owned()
}

fn without_urls(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut rest = text;
    let mut offset = 0;
    let mut safe = String::new();
    while let Some(start) = [
        lower[offset..].find("https://"),
        lower[offset..].find("http://"),
    ]
    .into_iter()
    .flatten()
    .min()
    {
        safe.push_str(&rest[..start]);
        let url = &rest[start..];
        let end = url
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '\'' | '"' | '`' | '<' | '>')
            })
            .unwrap_or(url.len());
        safe.push_str("[URL REDACTED]");
        offset += start + end;
        rest = &text[offset..];
    }
    safe.push_str(rest);
    safe
}

pub(crate) async fn error_text(error: &anyhow::Error, secrets: redact::Secrets) -> Result<String> {
    // 密码可以与 URL scheme 重叠；先移除完整 URL，再替换任务秘密。
    let raw = without_urls(&format!("{error:#}"));
    let (emit, mut events) = Emitter::channel();
    // 先完成秘密替换，再截断文本，避免输出被截断秘密的前缀。
    let (read, text) = tokio::join!(
        redact::read_log(raw.as_bytes(), "image-error", emit, secrets),
        async {
            let mut text = String::new();
            while let Some(event) = events.recv().await {
                let remaining = LOG_LIMIT.saturating_sub(text.len());
                text.push_str(&bounded(&String::from_utf8_lossy(&event.data), remaining));
            }
            text
        }
    );
    read?;
    Ok(text)
}

pub(crate) fn summary(stage: &str, text: &str) -> String {
    let printable: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let line = printable.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded(&format!("image {stage} failed: {line}"), SUMMARY_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn build_output_redacts_proxy_url_decoded_password_and_basic_authorization() {
        let mut secrets = redact::Secrets::default();
        proxy_value_secrets(
            &mut secrets,
            "http://proxy-user:private%2Bpassword@egress:18443",
        );
        let credentials =
            base64::engine::general_purpose::STANDARD.encode("proxy-user:private+password");
        let raw = format!(
            "http://proxy-user:private%2Bpassword@egress:18443 private+password private%2Bpassword {credentials} HTTP 407"
        );
        let (emit, mut events) = Emitter::channel();
        redact::read_log(raw.as_bytes(), "buildkit", emit, secrets)
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(event) = events.recv().await {
            text.push_str(&String::from_utf8_lossy(&event.data));
        }
        assert!(text.contains("HTTP 407"));
        assert!(text.contains("[REDACTED]"));
        for private in [
            "proxy-user",
            "private+password",
            "private%2Bpassword",
            &credentials,
        ] {
            assert!(!text.contains(private), "proxy credential leaked");
        }
    }

    #[tokio::test]
    async fn proxy_username_only_basic_authorization_is_redacted() {
        let mut secrets = redact::Secrets::default();
        proxy_value_secrets(&mut secrets, "http://private-user@egress:18443");
        let encoded = base64::engine::general_purpose::STANDARD.encode("private-user:");
        let raw = format!("Proxy-Authorization: Basic {encoded}; HTTP 407");
        let (emit, mut events) = Emitter::channel();
        redact::read_log(raw.as_bytes(), "buildkit", emit, secrets)
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(event) = events.recv().await {
            text.push_str(&String::from_utf8_lossy(&event.data));
        }
        assert!(text.contains("Basic [REDACTED]"));
        assert!(text.contains("HTTP 407"));
        assert!(!text.contains(&encoded));
    }

    #[tokio::test]
    async fn redacts_error_chains_before_utf8_safe_bounds() {
        let mut secrets = redact::Secrets::default();
        let secret = "私密-password";
        secrets.add(secret);
        let error = anyhow::anyhow!(
            "registry returned HTTP 407: {secret}\n{}",
            "构建".repeat(20000)
        )
        .context("push destination");
        let text = error_text(&error, secrets).await.unwrap();
        assert!(text.contains("push destination: registry returned HTTP 407: [REDACTED]"));
        assert!(!text.contains(secret));
        assert!(text.len() <= LOG_LIMIT);
        let message = summary("publication", &text);
        assert!(message.contains("HTTP 407"));
        assert!(message.len() <= SUMMARY_LIMIT);
        assert!(!message.contains('\n'));
    }

    #[tokio::test]
    async fn removes_unknown_url_credentials_and_signed_queries_from_logs_and_summary() {
        let error = anyhow::anyhow!("failed HTTPS://user:unknown-password@registry.invalid/path?X-Amz-Signature=unknown-signature&token=unknown-token: connection refused")
            .context("upload destination");
        let text = error_text(&error, redact::Secrets::default())
            .await
            .unwrap();
        let summary = summary("publication", &text);
        for value in [text, summary] {
            assert!(value.contains("upload destination"));
            assert!(value.contains("[URL REDACTED]"));
            assert!(value.contains("connection refused"));
            for private in [
                "user:",
                "unknown-password",
                "unknown-signature",
                "unknown-token",
                "X-Amz-Signature",
            ] {
                assert!(!value.contains(private));
            }
        }
    }

    #[tokio::test]
    async fn secrets_overlapping_url_schemes_cannot_expose_unknown_query_credentials() {
        for scheme in ["http", "https"] {
            let mut secrets = redact::Secrets::default();
            secrets.add(scheme);
            let error = anyhow::anyhow!(
                "failed {scheme}://user:unknown-password@registry.invalid/path?X-Amz-Signature=unknown-signature&token=unknown-token; password={scheme} connection refused"
            );
            let text = error_text(&error, secrets).await.unwrap();
            let summary = summary("publication", &text);
            for value in [text, summary] {
                assert!(value.contains("[URL REDACTED]"));
                assert!(value.contains("password=[REDACTED]"));
                assert!(value.contains("connection refused"));
                for private in [
                    "user:",
                    "unknown-password",
                    "unknown-signature",
                    "unknown-token",
                    "X-Amz-Signature",
                ] {
                    assert!(!value.contains(private), "secret overlapped {scheme}");
                }
            }
        }
    }

    #[tokio::test]
    async fn failure_uploads_redacted_log_before_a_useful_terminal_summary() {
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
            "task-token".into(),
            true,
        )
        .unwrap();
        let server = tokio::spawn(async { axum::serve(listener, app).await.unwrap() });
        let login = RegistryLogin {
            endpoint: "https://registry.invalid".into(),
            username: "user".into(),
            password: "password-to-redact".into(),
        };
        let mut secrets = redact::Secrets::default();
        registry_secrets(&mut secrets, &login);
        let credentials =
            base64::engine::general_purpose::STANDARD.encode("user:password-to-redact");
        let error = anyhow::anyhow!("registry HTTP 407; password-to-redact; {credentials}");
        let returned = control
            .failure("publish", &error, secrets, &AtomicI64::new(7))
            .await;
        let seen = reports.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0]["kind"], "log");
        assert_eq!(seen[0]["platform"], "publish");
        assert_eq!(seen[0]["sequence"], 8);
        assert_eq!(seen[1]["kind"], "failed");
        let encoded = serde_json::to_string(&*seen).unwrap();
        assert!(encoded.contains("HTTP 407"));
        assert!(encoded.contains("[REDACTED]"));
        assert!(!encoded.contains("password-to-redact"));
        assert!(!encoded.contains(&credentials));
        assert_eq!(returned, seen[1]["message"].as_str().unwrap());
        server.abort();
    }
}
