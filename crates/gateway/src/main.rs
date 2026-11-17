//! [INPUT]: 依赖 Pingora、固定后端和网关对象票据。
//! [OUTPUT]: 提供 HTTP 反向代理与设备 TLS 字节透传。
//! [POS]: 平台公共入口，私有 S3 URL 不返回客户端。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod config;
mod http;
mod tcp;
fn main() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    let mut server = pingora::server::Server::new(None)?;
    server.bootstrap();
    if config.profile != "device" {
        let mut http = pingora::proxy::http_proxy_service(
            &server.configuration,
            http::Gateway::new(config.clone())?,
        );
        if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
            http.add_tls(&config.http_bind, cert, key)?;
        } else {
            http.add_tcp(&config.http_bind);
        }
        server.add_service(http);
    }
    if config.profile == "device" {
        let mut service = pingora::services::listening::Service::new(
            "device TLS passthrough".into(),
            tcp::Passthrough::new(config.clone()),
        );
        service.add_tcp(&config.tcp_bind);
        server.add_service(service);
    }
    server.run_forever()
}
