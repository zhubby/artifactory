//! [INPUT]: 依赖 Pingora 监听服务、接受回调和关闭信号。
//! [OUTPUT]: 监督必要监听，并汇总实际接受连接后的就绪状态。
//! [POS]: 启动探测只连接本机；监听失败使整个网关退出。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use pingora::{
    apps::ServerApp,
    listeners::ConnectionFilter,
    server::ShutdownWatch,
    services::{Service, listening},
};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

// Pingora 在端口占用时最多重试三十次，给重试保留五秒余量。
const STARTUP_TIMEOUT: Duration = Duration::from_secs(35);

#[derive(Clone)]
pub struct Readiness(Arc<Status>);

#[derive(Debug)]
struct Status {
    required: usize,
    accepted: AtomicUsize,
    stopping: AtomicBool,
}

impl Readiness {
    pub fn new(required: usize) -> Self {
        Self(Arc::new(Status {
            required,
            accepted: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
        }))
    }

    pub fn is_ready(&self) -> bool {
        !self.0.stopping.load(Ordering::Acquire)
            && self.0.accepted.load(Ordering::Acquire) == self.0.required
    }

    fn stop(&self) {
        self.0.stopping.store(true, Ordering::Release);
    }
}

#[derive(Debug)]
struct Accepted {
    observed: AtomicBool,
    status: Arc<Status>,
}

#[async_trait]
impl ConnectionFilter for Accepted {
    async fn should_accept(&self, _peer: Option<&SocketAddr>) -> bool {
        // 回调只在端口和 TLS 构建完成后运行，连接到其他进程不能伪造就绪。
        if !self.observed.swap(true, Ordering::AcqRel) {
            self.status.accepted.fetch_add(1, Ordering::Release);
        }
        true
    }
}

pub struct RequiredListener<A> {
    service: Option<listening::Service<A>>,
    name: String,
    threads: Option<usize>,
    addresses: Option<Vec<String>>,
    probe: SocketAddr,
    accepted: Arc<Accepted>,
    readiness: Readiness,
}

impl<A: ServerApp + Send + Sync + 'static> RequiredListener<A> {
    pub fn new(
        mut service: listening::Service<A>,
        bind: &str,
        readiness: Readiness,
    ) -> Result<Self> {
        let mut probe = bind
            .to_socket_addrs()
            .context("invalid gateway listener address")?
            .next()
            .context("empty gateway listener address")?;
        ensure!(probe.port() != 0, "gateway listener requires a fixed port");
        if probe.ip().is_unspecified() {
            probe.set_ip(match probe.ip() {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
            });
        }
        let accepted = Arc::new(Accepted {
            observed: AtomicBool::new(false),
            status: readiness.0.clone(),
        });
        service.set_connection_filter(accepted.clone());
        Ok(Self {
            name: service.name().to_owned(),
            threads: service.threads(),
            addresses: service.listen_addresses(),
            service: Some(service),
            probe,
            accepted,
            readiness,
        })
    }

    async fn probe(&self) -> Result<()> {
        tokio::time::timeout(STARTUP_TIMEOUT, async {
            while !self.accepted.observed.load(Ordering::Acquire) {
                // TCP 握手成功不代表就绪，必须等自己的接受回调。
                let _ = tokio::time::timeout(
                    Duration::from_secs(1),
                    tokio::net::TcpStream::connect(self.probe),
                )
                .await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .context("gateway listener startup timed out")
    }
}

async fn shutdown_started(shutdown: &mut ShutdownWatch) {
    while !*shutdown.borrow() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}

fn completed(
    result: std::result::Result<(), tokio::task::JoinError>,
    shutdown: &ShutdownWatch,
) -> Result<()> {
    result.context("gateway listener task failed")?;
    ensure!(
        *shutdown.borrow(),
        "gateway listener stopped before shutdown"
    );
    Ok(())
}

#[async_trait]
impl<A: ServerApp + Send + Sync + 'static> Service for RequiredListener<A> {
    async fn start_service(
        &mut self,
        #[cfg(unix)] fds: Option<pingora::server::ListenFds>,
        mut shutdown: ShutdownWatch,
        listeners_per_fd: usize,
    ) {
        let Some(mut service) = self.service.take() else {
            std::process::exit(1);
        };
        let service_shutdown = shutdown.clone();
        let mut task = tokio::spawn(async move {
            service
                .start_service(
                    #[cfg(unix)]
                    fds,
                    service_shutdown,
                    listeners_per_fd,
                )
                .await;
        });
        let result: Result<()> = async {
            tokio::select! {
                result = &mut task => return completed(result, &shutdown),
                result = self.probe() => result?,
                () = shutdown_started(&mut shutdown) => self.readiness.stop(),
            }
            // 探测结束后只监督生命周期。
            tokio::select! {
                result = &mut task => completed(result, &shutdown),
                () = shutdown_started(&mut shutdown) => {
                    self.readiness.stop();
                    completed(task.await, &shutdown)
                }
            }
        }
        .await;
        self.readiness.stop();
        if result.is_err() {
            tracing::error!(listener = self.name, "必要网关监听失败，退出进程");
            std::process::exit(1);
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn threads(&self) -> Option<usize> {
        self.threads
    }

    fn listen_addresses(&self) -> Option<Vec<String>> {
        self.addresses.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(readiness: &Readiness) -> Accepted {
        Accepted {
            observed: AtomicBool::new(false),
            status: readiness.0.clone(),
        }
    }

    #[tokio::test]
    async fn readiness_requires_each_listener_not_repeated_connections() {
        let readiness = Readiness::new(2);
        let first = listener(&readiness);
        let second = listener(&readiness);
        assert!(!readiness.is_ready());
        assert!(first.should_accept(None).await);
        assert!(first.should_accept(None).await);
        assert!(!readiness.is_ready());
        assert!(second.should_accept(None).await);
        assert!(readiness.is_ready());
    }

    #[tokio::test]
    async fn late_accept_does_not_restore_readiness_after_shutdown() {
        let readiness = Readiness::new(2);
        assert!(listener(&readiness).should_accept(None).await);
        readiness.stop();
        assert!(listener(&readiness).should_accept(None).await);
        assert!(!readiness.is_ready());
    }
}
