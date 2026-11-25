//! [INPUT]: 依赖固定后端、接单开关、交付票据和请求预算。
//! [OUTPUT]: 暂停新规划请求，隔离终端预算并代理 HTTP。
//! [POS]: 不缓存认证结果，不缓冲镜像层，不重放写请求。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::config::Config;
use crate::listener::Readiness;
use async_trait::async_trait;
use pingora::{
    Error, ErrorType, Result,
    http::{RequestHeader, ResponseHeader},
    proxy::{ProxyHttp, Session},
    upstreams::peer::HttpPeer,
};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

/// 只限制回池连接的空闲时间，保留流式传输的独立读写时限。
const UPSTREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn delivery_disposition(filename: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !filename.is_empty()
            && filename.len() <= 255
            && filename.as_bytes()[0].is_ascii_alphanumeric()
            && filename
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)),
        "invalid delivery filename"
    );
    Ok(format!(
        "attachment; filename=\"{filename}\"; filename*=UTF-8''{filename}"
    ))
}

#[derive(Default)]
pub struct Context {
    upstream: Option<url::Url>,
    object: bool,
    disposition: Option<String>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    upstream_reused: Option<bool>,
}

pub struct Gateway {
    config: Config,
    readiness: Readiness,
    next: AtomicUsize,
    http: reqwest::Client,
    requests: Arc<tokio::sync::Semaphore>,
    terminals: Arc<tokio::sync::Semaphore>,
    manifests: tokio::sync::Semaphore,
    ca: Option<Arc<pingora::protocols::tls::CaType>>,
}
impl Gateway {
    pub fn new(config: Config, readiness: Readiness) -> anyhow::Result<Self> {
        let mut http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(std::time::Duration::from_secs(10));
        let mut ca = None;
        if let Ok(path) = std::env::var("ARTIFACTORY_SERVICE_CA") {
            let bytes = std::fs::read(path)?;
            http = http.add_root_certificate(reqwest::Certificate::from_pem(&bytes)?);
            let mut certs = x509_parser::pem::Pem::iter_from_buffer(&bytes)
                .map(|pem| {
                    let pem = pem.map_err(|_| anyhow::anyhow!("invalid gateway CA"))?;
                    pem.parse_x509()
                        .map_err(|_| anyhow::anyhow!("invalid gateway CA"))?;
                    Ok(pingora::utils::tls::WrappedX509::new(
                        pem.contents,
                        pingora::utils::tls::parse_x509,
                    ))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            anyhow::ensure!(!certs.is_empty(), "empty gateway CA");
            // Pingora 的 per-peer CA 替换默认根，必须合入系统根。
            certs.extend(
                rustls_native_certs::load_native_certs()
                    .certs
                    .into_iter()
                    .map(|cert| {
                        pingora::utils::tls::WrappedX509::new(
                            cert.as_ref().to_vec(),
                            pingora::utils::tls::parse_x509,
                        )
                    }),
            );
            ca = Some(Arc::from(certs));
        }
        Ok(Self {
            requests: Arc::new(tokio::sync::Semaphore::new(config.max_http)),
            terminals: Arc::new(tokio::sync::Semaphore::new(config.max_terminal)),
            // 八个四 MiB 正文上限，避免普通 HTTP 预算放大缓冲内存。
            manifests: tokio::sync::Semaphore::new(8),
            config,
            readiness,
            next: AtomicUsize::new(0),
            http: http.build()?,
            ca,
        })
    }
    fn select(&self, urls: &[url::Url]) -> url::Url {
        urls[self.next.fetch_add(1, Ordering::Relaxed) % urls.len()].clone()
    }
    async fn object(&self, session: &Session) -> anyhow::Result<(url::Url, Option<String>)> {
        let method = session.req_header().method.as_str();
        anyhow::ensure!(
            ["GET", "HEAD", "PUT"].contains(&method),
            "invalid object method"
        );
        let query = session.req_header().uri.query().unwrap_or("");
        anyhow::ensure!(query.len() <= 12288, "oversized object ticket");
        let mut tickets = url::form_urlencoded::parse(query.as_bytes())
            .filter(|(k, _)| k == "ticket")
            .map(|(_, v)| v.into_owned());
        let ticket = tickets
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing object ticket"))?;
        anyhow::ensure!(tickets.next().is_none(), "duplicate object ticket");
        let resolver = self
            .select(&self.config.control)
            .join("/internal/artifact-resolve")?;
        let mut response = self
            .http
            .post(resolver)
            .bearer_auth(&self.config.token)
            .json(&serde_json::json!({"ticket":ticket,"method":method}))
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        anyhow::ensure!(
            response.status().is_success() && response.content_length().is_none_or(|n| n <= 16384),
            "object ticket rejected"
        );
        #[derive(Deserialize)]
        struct Resolved {
            url: String,
            filename: Option<String>,
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 16384,
                "oversized object resolution"
            );
            bytes.extend_from_slice(&chunk);
        }
        let resolved: Resolved = serde_json::from_slice(&bytes)?;
        let url = url::Url::parse(&resolved.url)?;
        anyhow::ensure!(
            url.origin() == self.config.object_origin.origin()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none(),
            "object origin rejected"
        );
        let disposition = resolved
            .filename
            .as_deref()
            .map(delivery_disposition)
            .transpose()?;
        Ok((url, disposition))
    }
}
fn failure(message: &'static str) -> Box<Error> {
    Error::explain(ErrorType::HTTPStatus(502), message)
}
#[async_trait]
impl ProxyHttp for Gateway {
    type CTX = Context;
    fn new_ctx(&self) -> Context {
        Context::default()
    }
    async fn request_filter(&self, session: &mut Session, ctx: &mut Context) -> Result<bool> {
        let path = session.req_header().uri.path();
        if path == "/healthz" {
            session
                .respond_error(if self.readiness.is_ready() { 200 } else { 503 })
                .await?;
            return Ok(true);
        }
        let budget = if terminal_stream(path) {
            &self.terminals
        } else {
            &self.requests
        };
        let Ok(permit) = budget.clone().try_acquire_owned() else {
            session.respond_error(503).await?;
            return Ok(true);
        };
        ctx.permit = Some(permit);
        let internal = path.starts_with("/internal/");
        if internal && self.config.profile != "internal"
            || session.req_header().method == "CONNECT"
            || session.req_header().uri.scheme().is_some()
        {
            session.respond_error(403).await?;
            return Ok(true);
        }
        if self.config.profile == "internal" && !internal {
            session.respond_error(404).await?;
            return Ok(true);
        }
        let data = path == "/v2" || path.starts_with("/v2/") || path.starts_with("/artifacts/");
        if self.config.profile == "data" && !data && path != "/api/v1/registry/token"
            || self.config.profile == "control" && data
        {
            session.respond_error(404).await?;
            return Ok(true);
        }
        if self.config.build_admission_paused
            && build_admission_request(session.req_header().method.as_str(), path)
        {
            let body = br#"{"error":"build admission is paused for maintenance"}"#;
            let mut response = ResponseHeader::build(503, Some(4))?;
            response.insert_header("Content-Type", "application/json")?;
            response.insert_header("Content-Length", body.len().to_string())?;
            response.insert_header("Cache-Control", "no-store")?;
            response.insert_header("Retry-After", "60")?;
            session
                .write_error_response(response, body.as_slice().into())
                .await?;
            return Ok(true);
        }
        if path == "/artifacts/object" {
            match self.object(session).await {
                Ok((url, disposition)) => {
                    ctx.upstream = Some(url);
                    ctx.object = true;
                    ctx.disposition = disposition;
                }
                Err(_) => {
                    session.respond_error(403).await?;
                    return Ok(true);
                }
            }
        } else if path.starts_with("/artifacts/") {
            session.respond_error(404).await?;
            return Ok(true);
        } else if path == "/v2" || path.starts_with("/v2/") {
            if !["GET", "HEAD", "OPTIONS"].contains(&session.req_header().method.as_str())
                && (path.contains('%')
                    || path.contains('\\')
                    || path.contains("//")
                    || path.split('/').any(|part| part == "." || part == ".."))
            {
                session.respond_error(400).await?;
                return Ok(true);
            }
            if session.req_header().method == "PUT"
                && crate::registry::formal_manifest(path).is_some()
            {
                let Ok(_manifest) = self.manifests.try_acquire() else {
                    session.respond_error(503).await?;
                    return Ok(true);
                };
                crate::registry::write_manifest(
                    session,
                    &self.http,
                    self.select(&self.config.control),
                    self.select(&self.config.registry),
                    &self.config.token,
                )
                .await?;
                return Ok(true);
            }
            ctx.upstream = Some(self.select(&self.config.registry));
        } else if internal
            || path.starts_with("/api/")
            || path == "/health"
            || path.starts_with("/health/")
        {
            ctx.upstream = Some(self.select(&self.config.control));
        } else {
            ctx.upstream = Some(self.select(&self.config.web));
        }
        Ok(false)
    }
    async fn upstream_peer(
        &self,
        _session: &mut Session,
        ctx: &mut Context,
    ) -> Result<Box<HttpPeer>> {
        let url = ctx
            .upstream
            .as_ref()
            .ok_or_else(|| failure("missing backend"))?;
        let host = url
            .host_str()
            .ok_or_else(|| failure("missing backend host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| failure("missing backend port"))?;
        let mut addresses = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| failure("backend DNS unavailable"))?;
        let address = addresses
            .next()
            .ok_or_else(|| failure("backend DNS empty"))?;
        let mut peer = HttpPeer::new(address, url.scheme() == "https", host.into());
        peer.options.ca = self.ca.clone();
        peer.options.connection_timeout = Some(std::time::Duration::from_secs(5));
        peer.options.idle_timeout = Some(UPSTREAM_IDLE_TIMEOUT);
        peer.options.read_timeout = Some(std::time::Duration::from_secs(1800));
        peer.options.write_timeout = Some(std::time::Duration::from_secs(1800));
        Ok(Box::new(peer))
    }
    async fn upstream_request_filter(
        &self,
        _session: &mut Session,
        request: &mut RequestHeader,
        ctx: &mut Context,
    ) -> Result<()> {
        let url = ctx
            .upstream
            .as_ref()
            .ok_or_else(|| failure("missing backend"))?;
        for name in [
            "Forwarded",
            "X-Forwarded-For",
            "X-Forwarded-Host",
            "X-Forwarded-Proto",
            "X-Client-Cert",
            "X-SSL-Client-Cert",
            "X-Artifactory-Identity",
        ] {
            request.remove_header(name);
        }
        if ctx.object {
            let retain = [
                "range",
                "if-range",
                "if-match",
                "if-none-match",
                "if-modified-since",
                "if-unmodified-since",
                "content-length",
                "content-type",
                "transfer-encoding",
                "expect",
            ];
            let names: Vec<_> = request
                .headers
                .keys()
                .filter(|k| !retain.contains(&k.as_str()))
                .cloned()
                .collect();
            for name in names {
                request.remove_header(&name);
            }
            request.set_raw_path(url[url::Position::BeforePath..].as_bytes())?;
        }
        request.insert_header("Host", url.authority())?;
        Ok(())
    }
    async fn response_filter(
        &self,
        _session: &mut Session,
        response: &mut ResponseHeader,
        ctx: &mut Context,
    ) -> Result<()> {
        if ctx.object {
            if response.status.is_redirection() {
                return Err(failure("object store redirect rejected"));
            }
            response.remove_header("location");
            response.insert_header("Cache-Control", "private, no-store")?;
            if let Some(disposition) = &ctx.disposition
                && response.status.is_success()
            {
                response.insert_header("Content-Disposition", disposition)?;
            }
        }
        Ok(())
    }
    fn request_summary(&self, session: &Session, _ctx: &Context) -> String {
        format!(
            "{} {}",
            session.req_header().method,
            session.req_header().uri.path()
        )
    }
    fn suppress_error_log(&self, _session: &Session, _ctx: &Context, _error: &Error) -> bool {
        // Pingora 的完整错误上下文改由 logging 输出固定字段。
        true
    }
    async fn logging(&self, _session: &mut Session, error: Option<&Error>, ctx: &mut Context) {
        if let Some(error) = error {
            tracing::warn!(
                error_type = error.etype().as_str(),
                error_source = ?error.esource(),
                upstream_reused = ctx.upstream_reused,
                profile = self.config.profile.as_str(),
                "网关请求失败"
            );
        }
    }
    fn error_while_proxy(
        &self,
        _peer: &HttpPeer,
        _session: &mut Session,
        mut error: Box<Error>,
        ctx: &mut Context,
        reused: bool,
    ) -> Box<Error> {
        ctx.upstream_reused = Some(reused);
        error.set_retry(false);
        error
    }
    fn fail_to_connect(
        &self,
        _session: &mut Session,
        _peer: &HttpPeer,
        ctx: &mut Context,
        mut error: Box<Error>,
    ) -> Box<Error> {
        ctx.upstream_reused = Some(false);
        error.set_retry(false);
        error
    }
}

fn terminal_stream(path: &str) -> bool {
    path.strip_prefix("/api/v1/terminal-sessions/")
        .and_then(|tail| tail.strip_suffix("/stream"))
        .is_some_and(|id| !id.is_empty() && !id.contains('/'))
}

fn build_admission_request(method: &str, path: &str) -> bool {
    if method != "POST" {
        return false;
    }
    let parts: Vec<_> = path.split('/').collect();
    matches!(
        parts.as_slice(),
        ["", "api", "v1", "releases", id, "builds"]
            | ["", "api", "v1", "jobs", id, "retry"]
            | ["", "api", "v1", "builds", id, "clarifications"]
            if !id.is_empty()
    )
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn gateway(paused: bool, internal: bool) -> Gateway {
        let origin = url::Url::parse("https://upstream.invalid/").unwrap();
        let config = Config {
            profile: if internal { "internal" } else { "all" }.into(),
            http_bind: "127.0.0.1:8080".into(),
            tcp_bind: "127.0.0.1:8443".into(),
            internal_bind: "127.0.0.1:8445".into(),
            telemetry_bind: "127.0.0.1:8444".into(),
            control: vec![origin.clone()],
            registry: vec![origin.clone()],
            web: vec![origin.clone()],
            device: vec![],
            telemetry: vec![],
            object_origin: origin,
            token: "unused-in-local-filter-test".into(),
            max_http: 8,
            max_terminal: 8,
            max_tcp: 8,
            max_internal: 8,
            max_telemetry: 8,
            build_admission_paused: paused,
            tls_cert: None,
            tls_key: None,
            internal_tls_cert: None,
            internal_tls_key: None,
        };
        Gateway {
            config,
            readiness: Readiness::new(0),
            next: AtomicUsize::new(0),
            http: reqwest::Client::new(),
            requests: Arc::new(tokio::sync::Semaphore::new(8)),
            terminals: Arc::new(tokio::sync::Semaphore::new(8)),
            manifests: tokio::sync::Semaphore::new(8),
            ca: None,
        }
    }

    #[tokio::test]
    async fn paused_admission_returns_retryable_json_without_an_upstream() {
        let (mut client, server) = tokio::io::duplex(4096);
        client
            .write_all(b"POST /api/v1/releases/id/builds HTTP/1.1\r\nHost: local\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let mut session = Session::new_h1(Box::new(server));
        session.read_request().await.unwrap();
        let mut ctx = Context::default();
        assert!(
            gateway(true, false)
                .request_filter(&mut session, &mut ctx)
                .await
                .unwrap()
        );
        assert!(ctx.upstream.is_none());
        drop(session);
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        let normalized = response.to_ascii_lowercase();
        assert!(normalized.starts_with("http/1.1 503"));
        assert!(normalized.contains("retry-after: 60\r\n"));
        assert!(normalized.contains("cache-control: no-store\r\n"));
        assert!(normalized.contains("content-type: application/json\r\n"));
        assert!(response.ends_with("{\"error\":\"build admission is paused for maintenance\"}"));
    }

    #[tokio::test]
    async fn drain_preserves_reads_cancel_and_internal_receipts() {
        for (paused, internal, method, path) in [
            (false, false, "POST", "/api/v1/releases/id/builds"),
            (true, false, "GET", "/api/v1/releases/id/builds"),
            (true, false, "POST", "/api/v1/builds/id/cancel"),
            (true, false, "POST", "/api/v1/projects/id/refs"),
            (true, true, "POST", "/internal/planning/event"),
        ] {
            let (mut client, server) = tokio::io::duplex(4096);
            client
                .write_all(
                    format!("{method} {path} HTTP/1.1\r\nHost: local\r\nContent-Length: 0\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            let mut session = Session::new_h1(Box::new(server));
            session.read_request().await.unwrap();
            let mut ctx = Context::default();
            assert!(
                !gateway(paused, internal)
                    .request_filter(&mut session, &mut ctx)
                    .await
                    .unwrap(),
                "{method} {path}"
            );
            assert_eq!(ctx.upstream.unwrap().host_str(), Some("upstream.invalid"));
        }
    }

    #[test]
    fn admission_pause_matches_only_planning_creating_requests() {
        for path in [
            "/api/v1/releases/release-id/builds",
            "/api/v1/jobs/job-id/retry",
            "/api/v1/builds/build-id/clarifications",
        ] {
            assert!(build_admission_request("POST", path), "{path}");
            for method in ["GET", "HEAD", "PUT", "PATCH", "DELETE", "OPTIONS"] {
                assert!(!build_admission_request(method, path), "{method} {path}");
            }
        }
        for path in [
            "/api/v1/builds/build-id/cancel",
            "/api/v1/builds/build-id/activity",
            "/api/v1/builds",
            "/api/v1/projects/project-id/releases",
            "/api/v1/projects/project-id/refs",
            "/api/v1/operations/operation-id/retry",
            "/api/v1/emissary-updates/update-id/retry",
            "/api/v1/projects/project-id/notification-webhooks/hook/deliveries/id/retry",
            "/internal/planning/event",
            "/internal/planning/v1/prepare/ready",
            "/healthz",
            "/api/v1/releases//builds",
            "/api/v1/releases/release-id/builds/other",
        ] {
            assert!(!build_admission_request("POST", path), "{path}");
        }
    }
    #[test]
    fn content_disposition_accepts_only_bounded_safe_delivery_names() {
        assert_eq!(
            delivery_disposition("app-1.2.3-linux-amd64.tar.gz").unwrap(),
            "attachment; filename=\"app-1.2.3-linux-amd64.tar.gz\"; filename*=UTF-8''app-1.2.3-linux-amd64.tar.gz"
        );
        for invalid in [
            "",
            "../app",
            "a/b",
            "a\\b",
            "a\r\nX:1",
            "a\"b",
            "a;b",
            "中文.zip",
        ] {
            assert!(delivery_disposition(invalid).is_err());
        }
        assert!(delivery_disposition(&"a".repeat(256)).is_err());
    }
}
