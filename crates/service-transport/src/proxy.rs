//! [INPUT]: 依赖管理员 HTTP(S) 代理、显式直连清单与内部服务地址。
//! [OUTPUT]: 校验池级出口代理，生成大小写标准环境变量。
//! [POS]: 两个 Operator 共用的网络出口契约，不提供任意 Pod 环境入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 管理员为执行池指定出口；内部服务必须显式列入直连清单。
#[derive(Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProxyConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^https?://[^\s\x00-\x1f\x7f]+$"))]
    pub http_proxy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = r"^https?://[^\s\x00-\x1f\x7f]+$"))]
    pub https_proxy: Option<String>,
    #[schemars(length(min = 1, max = 8192), regex(pattern = r"^[^\x00-\x1f\x7f]+$"))]
    pub no_proxy: String,
}

impl std::fmt::Debug for ProxyConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProxyConfig")
            .field(
                "http_proxy",
                &self.http_proxy.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "https_proxy",
                &self.https_proxy.as_ref().map(|_| "[REDACTED]"),
            )
            .field("no_proxy", &self.no_proxy)
            .finish()
    }
}

impl ProxyConfig {
    /// 校验代理和直连边界，再生成 Git、Rust 与 Go 使用的环境变量。
    ///
    /// # Errors
    /// 代理不是 HTTP(S) origin、包含控制字符，或内部服务不在直连清单时失败。
    pub fn environment(
        &self,
        internal_urls: &[&str],
        api_host: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        ensure!(
            self.http_proxy.is_some() || self.https_proxy.is_some(),
            "pool proxy requires httpProxy or httpsProxy"
        );
        ensure!(
            !self.no_proxy.is_empty()
                && self.no_proxy.len() <= 8192
                && !self.no_proxy.chars().any(char::is_control)
                && self
                    .no_proxy
                    .split(',')
                    .all(|entry| !entry.trim().is_empty()),
            "pool proxy requires a nonempty noProxy without control characters"
        );
        ensure!(
            self.no_proxy.split(',').all(|entry| {
                let entry = entry.trim();
                entry == "*" || !entry.contains('*')
            }),
            "pool proxy noProxy supports hosts, DNS suffixes and a standalone wildcard only"
        );
        for proxy in [&self.http_proxy, &self.https_proxy].into_iter().flatten() {
            ensure!(
                proxy.len() <= 4096
                    && !proxy
                        .chars()
                        .any(|character| character.is_whitespace() || character.is_control()),
                "pool proxy URL contains whitespace or exceeds the size limit"
            );
            let parsed =
                url::Url::parse(proxy).map_err(|_| anyhow::anyhow!("invalid pool proxy URL"))?;
            ensure!(
                matches!(parsed.scheme(), "http" | "https")
                    && parsed.host_str().is_some()
                    && parsed.path() == "/"
                    && parsed.query().is_none()
                    && parsed.fragment().is_none(),
                "pool proxy requires an HTTP(S) origin"
            );
        }
        for internal in internal_urls {
            let parsed = url::Url::parse(internal)
                .map_err(|_| anyhow::anyhow!("invalid internal service URL"))?;
            self.require_bypass(
                parsed
                    .host_str()
                    .ok_or_else(|| anyhow::anyhow!("internal service URL requires a host"))?,
            )?;
        }
        self.require_bypass("kubernetes.default.svc")?;
        if let Some(host) = api_host {
            self.require_bypass(host)?;
        }
        let mut environment = Vec::new();
        for (name, value) in [
            ("HTTP_PROXY", self.http_proxy.as_ref()),
            ("HTTPS_PROXY", self.https_proxy.as_ref()),
            ("NO_PROXY", Some(&self.no_proxy)),
        ] {
            if let Some(value) = value {
                environment.push((name.to_owned(), value.clone()));
                environment.push((name.to_ascii_lowercase(), value.clone()));
            }
        }
        Ok(environment)
    }

    fn require_bypass(&self, host: &str) -> Result<()> {
        let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
        let address = host.parse::<std::net::IpAddr>().ok();
        ensure!(
            self.no_proxy.split(',').any(|entry| {
                let entry = entry.trim().to_ascii_lowercase();
                if let Some(address) = address {
                    return entry
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                        == Some(address);
                }
                if entry == "*" {
                    return true;
                }
                let suffix_only = entry.starts_with('.');
                let domain = entry.strip_prefix('.').unwrap_or(&entry);
                (!suffix_only && domain == host)
                    || (!domain.is_empty()
                        && host
                            .strip_suffix(domain)
                            .is_some_and(|prefix| prefix.ends_with('.')))
            }),
            "pool proxy noProxy must cover internal services and the Kubernetes API host"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxy() -> ProxyConfig {
        ProxyConfig {
            http_proxy: Some("http://private-user:private-password@egress:18443".into()),
            https_proxy: Some("https://egress:18444".into()),
            no_proxy: "localhost,127.0.0.1,.svc,.cluster.local,control,runtime,registry".into(),
        }
    }

    #[test]
    fn proxy_environment_is_explicit_and_preserves_both_client_conventions() {
        let proxy = proxy();
        let environment = proxy
            .environment(
                &["https://control:8445", "https://runtime:8445/planner"],
                None,
            )
            .unwrap();
        assert_eq!(environment.len(), 6);
        for (upper, lower) in [
            ("HTTP_PROXY", "http_proxy"),
            ("HTTPS_PROXY", "https_proxy"),
            ("NO_PROXY", "no_proxy"),
        ] {
            let value = |name| {
                environment
                    .iter()
                    .find(|(key, _)| key == name)
                    .unwrap()
                    .1
                    .clone()
            };
            assert_eq!(value(upper), value(lower));
        }
        assert_eq!(environment[4].1, proxy.no_proxy);
        assert!(!format!("{proxy:?}").contains("private-user"));
        assert!(!format!("{proxy:?}").contains("private-password"));
    }

    #[test]
    fn rejects_unsafe_proxy_urls_without_echoing_credentials() {
        for value in [
            "socks5://private-user:private-password@egress:1080",
            "http://private-user:private-password@egress\nINJECT=value",
            "http://egress/path",
            "http://egress/?token=private-password",
            "http://",
        ] {
            let mut proxy = proxy();
            proxy.http_proxy = Some(value.into());
            let error = proxy.environment(&[], None).unwrap_err().to_string();
            assert!(!error.contains("private-user"));
            assert!(!error.contains("private-password"));
        }
    }

    #[test]
    fn rejects_missing_direct_routes_and_newline_in_no_proxy() {
        let mut proxy = proxy();
        assert!(
            proxy
                .environment(&["https://control"], Some("10.233.0.1"))
                .is_err()
        );
        proxy.no_proxy.push_str(",10.233.0.1");
        assert!(
            proxy
                .environment(&["https://control"], Some("10.233.0.1"))
                .is_ok()
        );
        assert!(
            proxy
                .environment(&["https://missing-control"], None)
                .is_err()
        );
        proxy.no_proxy = "control,runtime,notkubernetes.default.svc".into();
        assert!(proxy.environment(&["https://control"], None).is_err());
        proxy.no_proxy = ".svc,control\nHTTPS_PROXY=malicious".into();
        assert!(proxy.environment(&[], None).is_err());
        proxy.no_proxy = "*.svc,control,runtime".into();
        assert!(proxy.environment(&["https://control"], None).is_err());
        assert!(ProxyConfig::default().environment(&[], None).is_err());
    }

    #[test]
    fn direct_route_checks_do_not_confuse_dns_suffixes_with_ip_or_apex_hosts() {
        let mut proxy = proxy();
        proxy.no_proxy = "*".into();
        assert!(proxy.environment(&[], Some("10.233.0.1")).is_err());
        proxy.no_proxy = "*,10.233.0.1".into();
        assert!(proxy.environment(&[], Some("10.233.0.1")).is_ok());
        proxy.no_proxy = ".svc,.control,233.0.1".into();
        assert!(proxy.environment(&["https://control"], None).is_err());
        assert!(proxy.environment(&[], Some("10.233.0.1")).is_err());
        proxy.no_proxy = ".svc,control,10.233.0.1".into();
        assert!(
            proxy
                .environment(&["https://control"], Some("10.233.0.1"))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn external_registry_uses_proxy_while_internal_control_stays_direct() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        const CHILD: &str = "ARTIFACTORY_TEST_PROXY_CHILD";
        if std::env::var(CHILD).as_deref() == Ok("1") {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            let control = std::env::var("ARTIFACTORY_TEST_PROXY_CONTROL").unwrap();
            assert_eq!(
                client
                    .get(control)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "{}"
            );
            assert_eq!(
                client
                    .get("http://external-registry.invalid/v2/")
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "{}"
            );
            assert!(
                client
                    .get("https://external-registry.invalid/v2/")
                    .send()
                    .await
                    .is_err()
            );
            return;
        }
        let direct = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control = format!("http://{}/internal/health", direct.local_addr().unwrap());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let direct_request = tokio::spawn(async move {
            let (mut stream, _) = direct.accept().await.unwrap();
            let mut bytes = [0; 2048];
            let size = stream.read(&mut bytes).await.unwrap();
            assert!(
                String::from_utf8_lossy(&bytes[..size])
                    .starts_with("GET /internal/health HTTP/1.1")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .unwrap();
        });
        let proxy_requests = tokio::spawn(async move {
            for expected in [
                "GET http://external-registry.invalid/v2/ HTTP/1.1",
                "CONNECT external-registry.invalid:443 HTTP/1.1",
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = [0; 2048];
                let size = stream.read(&mut bytes).await.unwrap();
                assert!(String::from_utf8_lossy(&bytes[..size]).starts_with(expected));
                let response = if expected.starts_with("GET") {
                    &b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"[..]
                } else {
                    &b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"[..]
                };
                stream.write_all(response).await.unwrap();
            }
        });
        let proxy = ProxyConfig {
            http_proxy: Some(format!("http://{address}")),
            https_proxy: Some(format!("http://{address}")),
            ..proxy()
        };
        let output = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "proxy::tests::external_registry_uses_proxy_while_internal_control_stays_direct",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("ARTIFACTORY_TEST_PROXY_CONTROL", control)
            .env_remove("ALL_PROXY")
            .env_remove("all_proxy")
            .envs(proxy.environment(&["https://control"], None).unwrap())
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "proxy transport child failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            direct_request.await.unwrap();
            proxy_requests.await.unwrap();
        })
        .await
        .unwrap();
    }
}
