//! [INPUT]: 依赖固定环境配置与受限源地址。
//! [OUTPUT]: 提供统一监听、独立连接预算和固定后端。
//! [POS]: 统一模式隔离公开、内部、设备和遥测端口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Result, ensure};
#[derive(Clone)]
pub struct Config {
    pub profile: String,
    pub http_bind: String,
    pub tcp_bind: String,
    pub internal_bind: String,
    pub telemetry_bind: String,
    pub control: Vec<url::Url>,
    pub registry: Vec<url::Url>,
    pub web: Vec<url::Url>,
    pub device: Vec<String>,
    pub telemetry: Vec<String>,
    pub object_origin: url::Url,
    pub token: String,
    pub max_http: usize,
    pub max_terminal: usize,
    pub max_tcp: usize,
    pub max_internal: usize,
    pub max_telemetry: usize,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub internal_tls_cert: Option<String>,
    pub internal_tls_key: Option<String>,
}
impl Config {
    pub fn load() -> Result<Self> {
        let env =
            |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.into());
        let insecure = env("ARTIFACTORY_INSECURE_DEV", "false") == "true";
        let upstream_http =
            env("ARTIFACTORY_GATEWAY_ALLOW_HTTP_UPSTREAMS", "false") == "true" || insecure;
        let origins = |name: &str, default: &str| -> Result<Vec<url::Url>> {
            env(name, default)
                .split(',')
                .map(|s| origin(s, upstream_http))
                .collect()
        };
        let profile = env("ARTIFACTORY_GATEWAY_PROFILE", "all");
        ensure!(
            ["all", "control", "data", "device", "internal", "unified"].contains(&profile.as_str()),
            "invalid gateway profile"
        );
        let token = std::env::var("ARTIFACTORY_GATEWAY_TOKEN")?;
        ensure!(token.len() >= 32, "gateway token requires 32 characters");
        let tls_cert = std::env::var("ARTIFACTORY_GATEWAY_TLS_CERT").ok();
        let tls_key = std::env::var("ARTIFACTORY_GATEWAY_TLS_KEY").ok();
        let internal_tls_cert = std::env::var("ARTIFACTORY_GATEWAY_INTERNAL_TLS_CERT").ok();
        let internal_tls_key = std::env::var("ARTIFACTORY_GATEWAY_INTERNAL_TLS_KEY").ok();
        ensure!(
            tls_cert.is_some() == tls_key.is_some(),
            "gateway requires both certificate and key"
        );
        ensure!(
            profile != "internal" || tls_cert.is_some(),
            "internal gateway requires TLS"
        );
        ensure!(
            internal_tls_cert.is_some() == internal_tls_key.is_some(),
            "internal listener requires both certificate and key"
        );
        ensure!(
            profile != "unified" || internal_tls_cert.is_some(),
            "unified gateway requires internal TLS"
        );
        let http_bind = env("ARTIFACTORY_GATEWAY_HTTP_BIND", "0.0.0.0:8080");
        let tcp_bind = env("ARTIFACTORY_GATEWAY_TCP_BIND", "0.0.0.0:8443");
        let internal_bind = env("ARTIFACTORY_GATEWAY_INTERNAL_BIND", "0.0.0.0:8445");
        let telemetry_bind = env("ARTIFACTORY_GATEWAY_TELEMETRY_BIND", "0.0.0.0:8444");
        if profile == "unified" {
            let mut ports = std::collections::HashSet::new();
            for bind in [&http_bind, &tcp_bind, &internal_bind, &telemetry_bind] {
                let address: std::net::SocketAddr = bind.parse()?;
                ensure!(
                    ports.insert(address.port()),
                    "unified gateway listeners require distinct ports"
                );
            }
        }
        let count = |name: &str, default: &str| -> Result<usize> {
            let value = env(name, default).parse()?;
            ensure!((1..=100000).contains(&value), "invalid gateway concurrency");
            Ok(value)
        };
        Ok(Self {
            profile,
            http_bind,
            tcp_bind,
            internal_bind,
            telemetry_bind,
            control: origins("ARTIFACTORY_GATEWAY_CONTROL", "http://control-plane:8080")?,
            registry: origins("ARTIFACTORY_GATEWAY_REGISTRY", "http://registry:5000")?,
            web: origins("ARTIFACTORY_GATEWAY_WEB", "http://web:3000")?,
            device: env("ARTIFACTORY_GATEWAY_DEVICE", "control-plane:8443")
                .split(',')
                .map(str::to_owned)
                .collect(),
            telemetry: env("ARTIFACTORY_GATEWAY_TELEMETRY", "telemetry:8444")
                .split(',')
                .map(str::to_owned)
                .collect(),
            object_origin: origin(&std::env::var("ARTIFACTORY_S3_ENDPOINT")?, upstream_http)?,
            token,
            max_http: count("ARTIFACTORY_GATEWAY_HTTP_CONNECTIONS", "1024")?,
            max_terminal: count("ARTIFACTORY_GATEWAY_TERMINAL_CONNECTIONS", "128")?,
            max_tcp: count("ARTIFACTORY_GATEWAY_DEVICE_CONNECTIONS", "4096")?,
            max_internal: count("ARTIFACTORY_GATEWAY_INTERNAL_CONNECTIONS", "1024")?,
            max_telemetry: count("ARTIFACTORY_GATEWAY_TELEMETRY_CONNECTIONS", "4096")?,
            tls_cert,
            tls_key,
            internal_tls_cert,
            internal_tls_key,
        })
    }
}
pub fn origin(value: &str, insecure: bool) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    ensure!(
        url.host_str().is_some()
            && (url.scheme() == "https" || (insecure && url.scheme() == "http"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "backend must be a fixed HTTPS origin"
    );
    Ok(url)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origin_rejects_credentials_path_and_plaintext() {
        for s in [
            "http://store/",
            "https://user:secret@store/",
            "https://store/path",
            "https://store/?u=other",
        ] {
            assert!(origin(s, false).is_err());
        }
        assert!(origin("https://store:9000/", false).is_ok());
    }
}
