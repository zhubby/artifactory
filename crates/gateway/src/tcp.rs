//! [INPUT]: 依赖原始 TCP 流、固定设备后端和连接预算。
//! [OUTPUT]: 将设备 TLS 字节透传给控制平面。
//! [POS]: 保留客户端证书到 tonic 的端到端认证。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::config::Config;
use async_trait::async_trait;
use pingora::{apps::ServerApp, protocols::Stream, server::ShutdownWatch};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
pub struct Passthrough {
    config: Config,
    next: AtomicUsize,
    connections: tokio::sync::Semaphore,
}
impl Passthrough {
    pub fn new(config: Config) -> Self {
        Self {
            connections: tokio::sync::Semaphore::new(config.max_tcp),
            config,
            next: AtomicUsize::new(0),
        }
    }
}
#[async_trait]
impl ServerApp for Passthrough {
    async fn process_new(
        self: &Arc<Self>,
        mut downstream: Stream,
        _shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let Ok(_permit) = self.connections.try_acquire() else {
            return None;
        };
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.config.device.len();
        let destination = &self.config.device[index];
        if let Ok(Ok(mut upstream)) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::TcpStream::connect(destination),
        )
        .await
        {
            let _ = upstream.set_nodelay(true);
            // 建立后不切换后端，避免重复命令与 TLS 会话损坏。
            let _ = tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await;
        }
        None
    }
}
