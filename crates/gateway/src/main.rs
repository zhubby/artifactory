//! [INPUT]: 依赖 Pingora、必要监听监督、固定后端与对象票据。
//! [OUTPUT]: 同进程提供四监听，全部启动后才报告就绪。
//! [POS]: 监听失败使进程退出，各入口保留独立连接预算。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod config;
mod http;
mod listener;
mod registry;
mod tcp;
fn add_http(
    server: &mut pingora::server::Server,
    config: &config::Config,
    readiness: listener::Readiness,
) -> anyhow::Result<()> {
    let mut http = pingora::proxy::http_proxy_service_with_name(
        &server.configuration,
        http::Gateway::new(config.clone(), readiness.clone())?,
        &format!("{} HTTP gateway", config.profile),
    );
    if let (Some(cert), Some(key)) = (&config.tls_cert, &config.tls_key) {
        http.add_tls(&config.http_bind, cert, key)?;
    } else {
        http.add_tcp(&config.http_bind);
    }
    server.add_service(listener::RequiredListener::new(
        http,
        &config.http_bind,
        readiness,
    )?);
    Ok(())
}
fn main() -> anyhow::Result<()> {
    // Pingora 的 debug 日志包含请求头，网关固定日志上限为 info。
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let config = config::Config::load()?;
    let unified = config.profile == "unified";
    let readiness = listener::Readiness::new(if unified { 4 } else { 1 });
    let mut server = pingora::server::Server::new(None)?;
    server.bootstrap();
    if config.profile != "device" {
        let mut public = config.clone();
        if unified {
            public.profile = "all".into();
        }
        add_http(&mut server, &public, readiness.clone())?;
    }
    if unified {
        let mut internal = config.clone();
        internal.profile = "internal".into();
        internal.http_bind = config.internal_bind.clone();
        internal.tls_cert = config.internal_tls_cert.clone();
        internal.tls_key = config.internal_tls_key.clone();
        internal.max_http = config.max_internal;
        add_http(&mut server, &internal, readiness.clone())?;
    }
    if config.profile == "device" || unified {
        let mut service = pingora::services::listening::Service::new(
            "device TLS passthrough".into(),
            tcp::Passthrough::new(config.device.clone(), config.max_tcp),
        );
        service.add_tcp(&config.tcp_bind);
        server.add_service(listener::RequiredListener::new(
            service,
            &config.tcp_bind,
            readiness.clone(),
        )?);
    }
    if unified {
        let mut service = pingora::services::listening::Service::new(
            "telemetry TLS passthrough".into(),
            tcp::Passthrough::new(config.telemetry, config.max_telemetry),
        );
        service.add_tcp(&config.telemetry_bind);
        server.add_service(listener::RequiredListener::new(
            service,
            &config.telemetry_bind,
            readiness,
        )?);
    }
    server.run_forever()
}
