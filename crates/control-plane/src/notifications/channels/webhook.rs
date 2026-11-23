//! [INPUT]: 依赖 URL 语法、固定出口策略、签名密钥与请求权限。
//! [OUTPUT]: 提供 Webhook 输入、DNS 固定客户端、SSRF 校验与 HMAC 签名。
//! [POS]: 唯一 Webhook 协议出口；配置和投递诊断委托给 store。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::bad;
pub use crate::store::notifications::channel_webhook::{deliveries, list, mutate, write};
use crate::{
    auth::Auth,
    error::{ApiError, Result},
    state::AppState,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::{IpAddr, Ipv4Addr};
use uuid::Uuid;

pub const EVENTS: &[&str] = &[
    "build.status.changed",
    "build.phase.changed",
    "release.publication.changed",
    "permission.member.changed",
    "permission.resources.changed",
    "device.status.changed",
    "device.update.changed",
    "credential.status.changed",
    "account.security.changed",
    "workspace.lifecycle.changed",
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub name: Option<String>,
    pub url: Option<String>,
    pub enabled: Option<bool>,
    pub event_types: Option<Vec<String>>,
}

pub fn validate_url(value: &str) -> Result<url::Url> {
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(bad("invalid webhook URL"));
    }
    let url = url::Url::parse(value).map_err(|_| bad("invalid webhook URL"))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(bad(
            "webhooks require an HTTPS URL without credentials or fragment",
        ));
    }
    Ok(url)
}

fn ipv4_private(ip: Ipv4Addr) -> bool {
    ip.is_private()
}
pub fn address_allowed(ip: IpAddr, allow: &[ipnet::IpNet]) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            if ip == Ipv4Addr::new(168, 63, 129, 16) {
                return false;
            }
            let [a, b, _, _] = ip.octets();
            if a == 0
                || a == 127
                || a >= 224
                || (a == 169 && b == 254)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && (b == 0 || b == 88))
                || ip.is_documentation()
                || ip.is_broadcast()
                || (a == 198 && (b == 18 || b == 19))
            {
                return false;
            }
            !ipv4_private(ip) || allow.iter().any(|n| n.contains(&IpAddr::V4(ip)))
        }
        IpAddr::V6(ip) => {
            if ip
                == "fd00:ec2::254"
                    .parse::<std::net::Ipv6Addr>()
                    .expect("constant IP")
            {
                return false;
            }
            // Reject transition, mapped, link local and metadata paths even in an allowlist.
            let oct = ip.octets();
            if ip.to_ipv4_mapped().is_some()
                || ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
            {
                return false;
            }
            if oct[0] & 0xfe == 0xfc {
                return allow.iter().any(|n| n.contains(&IpAddr::V6(ip)));
            }
            if oct[0] & 0xe0 != 0x20 {
                return false;
            }
            if (oct[0] == 0x3f && oct[1] == 0xff && oct[2] < 0x10)
                || oct[..2] == [0x20, 0x02]
                || oct[..4] == [0x20, 0x01, 0x00, 0x00]
                || oct[..4] == [0x20, 0x01, 0x0d, 0xb8]
                || oct[..3] == [0x20, 0x01, 0x00]
            {
                return false;
            }
            true
        }
    }
}

pub async fn client(state: &AppState, url: &url::Url) -> Result<reqwest::Client> {
    let host = url.host_str().ok_or_else(|| bad("invalid webhook host"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addresses = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(443))),
    )
    .await
    .map_err(|_| bad("webhook DNS resolution timed out"))?
    .map_err(|_| bad("webhook DNS resolution failed"))?
    .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses.iter().any(|a| {
            !address_allowed(
                a.ip(),
                &state.settings.notifications.webhook_private_allowlist,
            )
        })
    {
        return Err(bad("webhook destination is blocked by platform policy"));
    }
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(5))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| ApiError::Internal(e.into()))
}

pub fn signature(secret: &str, timestamp: i64, id: Uuid, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{timestamp}.{id}.").as_bytes());
    mac.update(body);
    format!("v1={}", hex::encode(mac.finalize().into_bytes()))
}

pub fn authorize(auth: &Auth, project: Option<Uuid>) -> Result<()> {
    if auth.token_projects.is_some() {
        return Err(ApiError::Forbidden);
    }
    if let Some(project) = project {
        auth.project(project, "admin")
    } else {
        auth.session_role("admin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssrf_denies_metadata_transition_and_local_addresses() {
        for ip in [
            "127.0.0.1",
            "0.0.0.0",
            "169.254.169.254",
            "100.100.100.200",
            "192.0.0.8",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:1::",
            "2001::1",
            "2001:db8::1",
            "fe80::1",
            "fc00::1",
            "168.63.129.16",
            "fd00:ec2::254",
            "3fff::1",
        ] {
            assert!(!address_allowed(ip.parse().unwrap(), &[]), "{ip}");
        }
        assert!(address_allowed("8.8.8.8".parse().unwrap(), &[]));
        assert!(address_allowed(
            "2606:4700:4700::1111".parse().unwrap(),
            &[]
        ));
    }
    #[test]
    fn private_access_requires_narrow_explicit_platform_allowlist() {
        let allow = vec![
            "10.2.0.0/16".parse().unwrap(),
            "fc00:1234::/32".parse().unwrap(),
        ];
        assert!(address_allowed("10.2.0.4".parse().unwrap(), &allow));
        assert!(!address_allowed("10.3.0.4".parse().unwrap(), &allow));
        assert!(address_allowed("fc00:1234::2".parse().unwrap(), &allow));
        assert!(!address_allowed(
            "127.0.0.1".parse().unwrap(),
            &["0.0.0.0/0".parse().unwrap()]
        ));
    }
    #[test]
    fn endpoints_reject_credentials_fragments_and_plaintext() {
        for u in [
            "http://example.com/",
            "https://user:secret@example.com/",
            "https://example.com/#secret",
            "https://example.com/\n",
        ] {
            assert!(validate_url(u).is_err());
        }
    }
    #[test]
    fn signature_binds_time_id_and_exact_body() {
        let id = Uuid::nil();
        let sig = signature("secret", 123, id, b"{}");
        assert_eq!(sig.len(), 67);
        assert_ne!(sig, signature("secret", 124, id, b"{}"));
        assert_ne!(sig, signature("secret", 123, Uuid::new_v4(), b"{}"));
        assert_ne!(sig, signature("secret", 123, id, b"{} "));
    }
}
