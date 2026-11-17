//! [INPUT]: 依赖 Pingora、固定后端和网关对象票据。
//! [OUTPUT]: 同进程提供公开 HTTP、内部 HTTPS 与设备和遥测 TLS 透传。
//! [POS]: 各监听保留路由边界与独立连接预算。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod config;
mod http;
mod tcp;
fn add_http(server: &mut pingora::server::Server, config: config::Config) -> anyhow::Result<()> {
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
    Ok(())
}
fn main() -> anyhow::Result<()> {
    let config = config::Config::load()?;
    let unified = config.profile == "unified";
    let mut server = pingora::server::Server::new(None)?;
    server.bootstrap();
    if config.profile != "device" {
        let mut public = config.clone();
        if unified {
            public.profile = "all".into();
        }
        add_http(&mut server, public)?;
    }
    if unified {
        let mut internal = config.clone();
        internal.profile = "internal".into();
        internal.http_bind = config.internal_bind.clone();
        internal.tls_cert = config.internal_tls_cert.clone();
        internal.tls_key = config.internal_tls_key.clone();
        internal.max_http = config.max_internal;
        add_http(&mut server, internal)?;
    }
    if config.profile == "device" || unified {
        let mut service = pingora::services::listening::Service::new(
            "device TLS passthrough".into(),
            tcp::Passthrough::new(config.device.clone(), config.max_tcp),
        );
        service.add_tcp(&config.tcp_bind);
        server.add_service(service);
    }
    if unified {
        let mut service = pingora::services::listening::Service::new(
            "telemetry TLS passthrough".into(),
            tcp::Passthrough::new(config.telemetry, config.max_telemetry),
        );
        service.add_tcp(&config.telemetry_bind);
        server.add_service(service);
    }
    server.run_forever()
}
