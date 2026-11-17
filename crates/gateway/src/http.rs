//! [INPUT]: 依赖固定 HTTP 后端、加密对象票据、请求预算与结构化日志。
//! [OUTPUT]: 流式代理 HTTP，限制上游空闲复用并记录脱敏错误。
//! [POS]: 不缓存认证结果，不缓冲镜像层，不重放写请求。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::config::Config;
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

#[derive(Default)]
pub struct Context {
    upstream: Option<url::Url>,
    object: bool,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    upstream_reused: Option<bool>,
}
pub struct Gateway {
    config: Config,
    next: AtomicUsize,
    http: reqwest::Client,
    requests: Arc<tokio::sync::Semaphore>,
    ca: Option<Arc<pingora::protocols::tls::CaType>>,
}
impl Gateway {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let mut http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
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
            config,
            next: AtomicUsize::new(0),
            http: http.build()?,
            ca,
        })
    }
    fn select(&self, urls: &[url::Url]) -> url::Url {
        urls[self.next.fetch_add(1, Ordering::Relaxed) % urls.len()].clone()
    }
    async fn object(&self, session: &Session) -> anyhow::Result<url::Url> {
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
        Ok(url)
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
            session.respond_error(200).await?;
            return Ok(true);
        }
        let Ok(permit) = self.requests.clone().try_acquire_owned() else {
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
        if path == "/artifacts/object" {
            match self.object(session).await {
                Ok(url) => {
                    ctx.upstream = Some(url);
                    ctx.object = true
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
            ctx.upstream = Some(self.select(&self.config.registry));
        } else if path.starts_with("/api/v1/source/") && self.config.source.is_some() {
            ctx.upstream = self.config.source.clone();
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
