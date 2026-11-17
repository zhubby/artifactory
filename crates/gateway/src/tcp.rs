//! [INPUT]: 依赖原始 TCP 流、固定后端和独立连接预算。
//! [OUTPUT]: 将设备和遥测 TLS 字节透传给对应服务。
//! [POS]: 每个监听保留客户端证书到 tonic 的端到端认证。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use async_trait::async_trait;
use pingora::{apps::ServerApp, protocols::Stream, server::ShutdownWatch};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
pub struct Passthrough {
    destinations: Vec<String>,
    next: AtomicUsize,
    connections: tokio::sync::Semaphore,
}
impl Passthrough {
    pub fn new(destinations: Vec<String>, max_connections: usize) -> Self {
        Self {
            connections: tokio::sync::Semaphore::new(max_connections),
            destinations,
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
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.destinations.len();
        let destination = &self.destinations[index];
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
