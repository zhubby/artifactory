//! [INPUT]: 依赖平台 SMTP 配置、邮箱语法和退订签名密钥。
//! [OUTPUT]: 提供地址规范化、验证与退订令牌、有总超时的 TLS 邮件发送。
//! [POS]: 邮件协议适配器；地址状态和订阅事务委托给 store。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::bad;
pub use crate::store::notifications::channel_email::{
    bind, preferences, put_preferences, retire, unsubscribe, verify,
};
use crate::{error::Result, state::AppState};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::Mailbox,
    transport::smtp::authentication::Credentials,
};
use uuid::Uuid;

pub fn normalize_address(address: &str) -> Result<String> {
    if address.chars().any(char::is_control) {
        return Err(bad("invalid email address"));
    }
    let address = address.trim();
    if address.len() > 254 || address.chars().any(char::is_control) || address.contains(['<', '>'])
    {
        return Err(bad("invalid email address"));
    }
    let mailbox = address
        .parse::<Mailbox>()
        .map_err(|_| bad("invalid email address"))?;
    if mailbox.name.is_some() {
        return Err(bad("use a bare email address"));
    }
    let addr = mailbox.email.to_string();
    let (local, domain) = addr
        .rsplit_once('@')
        .ok_or_else(|| bad("invalid email address"))?;
    Ok(format!("{local}@{}", domain.to_ascii_lowercase()))
}

pub fn unsubscribe_token(key: &[u8; 32], user: Uuid, generation: i64) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts 32 byte key");
    mac.update(b"notification-unsubscribe/v1/");
    mac.update(user.as_bytes());
    mac.update(&generation.to_be_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) fn verification_token(
    key: &[u8; 32],
    user: Uuid,
    generation: i64,
    delivery: Uuid,
) -> String {
    use hmac::{Hmac, Mac};
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).expect("HMAC accepts 32 byte key");
    mac.update(b"notification-verification/v1/");
    mac.update(user.as_bytes());
    mac.update(&generation.to_be_bytes());
    mac.update(delivery.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub async fn send(
    state: &AppState,
    id: Uuid,
    address: &str,
    subject: &str,
    body: String,
    unsubscribe_url: Option<&str>,
) -> std::result::Result<(), bool> {
    let config = state.settings.notifications.smtp.as_ref().ok_or(true)?;
    let builder = if config.starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
    }
    .map_err(|_| false)?;
    let mut builder = builder
        .port(config.port)
        .timeout(Some(std::time::Duration::from_secs(20)));
    if let (Some(user), Some(password)) = (&config.username, &config.password) {
        builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
    }
    let message =
        message(&config.from, id, address, subject, body, unsubscribe_url).map_err(|_| false)?;
    // Lettre 的连接预算不覆盖 DNS、TLS 和 SMTP 响应等待。
    // 总预算短于 90 秒投递租约，避免慢服务器耗尽全部副本。
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        builder.build().send(message),
    )
    .await
    .map_err(|_| true)?
    .map(|_| ())
    .map_err(|error| error.is_transient() || error.is_timeout() || !error.is_permanent())
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn message(
    from: &str,
    id: Uuid,
    address: &str,
    subject: &str,
    body: String,
    unsubscribe_url: Option<&str>,
) -> anyhow::Result<Message> {
    use lettre::message::{
        MultiPart,
        header::{HeaderName, HeaderValue},
    };
    let mut builder = Message::builder()
        .from(from.parse()?)
        .to(address.parse()?)
        .subject(subject)
        .message_id(Some(format!("<{id}@notifications.artifactory>")));
    if let Some(url) = unsubscribe_url {
        let url = url::Url::parse(url)?;
        anyhow::ensure!(
            url.scheme() == "https" || url.scheme() == "http",
            "invalid unsubscribe URL"
        );
        builder = builder
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe"),
                format!("<{url}>"),
            ))
            .raw_header(HeaderValue::new(
                HeaderName::new_from_ascii_str("List-Unsubscribe-Post"),
                "List-Unsubscribe=One-Click".into(),
            ));
    }
    let html_body = body
        .lines()
        .map(|line| {
            let escaped = html_escape(line);
            if url::Url::parse(line).is_ok_and(|u| matches!(u.scheme(), "https" | "http")) {
                format!("<a href=\"{escaped}\">{escaped}</a>")
            } else {
                escaped
            }
        })
        .collect::<Vec<_>>()
        .join("<br>\n");
    let html = format!(
        "<!doctype html><html><body><h1>{}</h1><p>{html_body}</p></body></html>",
        html_escape(subject)
    );
    Ok(builder.multipart(MultiPart::alternative_plain_html(body, html))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multipart_mail_escapes_html_and_keeps_stable_identity_and_unsubscribe_headers() {
        let id = Uuid::nil();
        let msg = message(
            "notify@example.org",
            id,
            "person@example.org",
            "Safe",
            "<script>secret</script>\nhttps://example.org/notifications".into(),
            Some("https://example.org/api/v1/notifications/email/one-click?token=opaque"),
        )
        .unwrap();
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("text/plain"));
        assert!(raw.contains("text/html"));
        assert!(raw.contains("List-Unsubscribe:"));
        assert!(raw.contains("List-Unsubscribe-Post:"));
        assert!(raw.contains(&format!("<{id}@notifications.artifactory>")));
        assert!(raw.contains("&lt;script&gt;"));
    }
    #[test]
    fn addresses_reject_header_injection_and_display_names() {
        for address in [
            "bad",
            "a@example.org\r\nBcc: victim@example.org",
            "Name <a@example.org>",
            "a@example.org\n",
        ] {
            assert!(normalize_address(address).is_err(), "{address:?}");
        }
        assert_eq!(
            normalize_address("local@EXAMPLE.ORG").unwrap(),
            "local@example.org"
        );
    }
}
