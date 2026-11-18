//! ```text
//! [INPUT]: 依赖任务私有 Unix socket、单调时钟和取消令牌。
//! [OUTPUT]: 提供限长生命周期帧、独占 Runner 连接与停止确认。
//! [POS]: Runner 和最小 BuildKit 监督程序共用的本地协议。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use anyhow::{Context, Result, bail, ensure};
use std::{
    fmt,
    os::fd::AsRawFd,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, timeout, timeout_at},
};
use tokio_util::sync::CancellationToken;

pub const SOCKET: &str = "/run/buildkit/lifecycle.sock";
pub const DAEMON_ADDRESS: &str = "unix:///run/buildkit/buildkitd.sock";
pub const START_TIMEOUT: Duration = Duration::from_secs(60);
pub const STOP_TIMEOUT: Duration = Duration::from_secs(15);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);
pub const HEARTBEAT_LEASE: Duration = Duration::from_secs(10);
pub const DIAGNOSTIC_LIMIT: usize = 16384;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Frame {
    Heartbeat = 1,
    Ready = 2,
    Stop = 3,
    Stopped = 4,
    Failed = 5,
    Diagnostic = 6,
}
impl TryFrom<u8> for Frame {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Heartbeat),
            2 => Ok(Self::Ready),
            3 => Ok(Self::Stop),
            4 => Ok(Self::Stopped),
            5 => Ok(Self::Failed),
            6 => Ok(Self::Diagnostic),
            _ => bail!("invalid lifecycle frame"),
        }
    }
}

#[derive(Clone)]
pub struct Diagnostic {
    pub channel: u8,
    pub truncated: bool,
    pub bytes: Vec<u8>,
}
impl fmt::Debug for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Diagnostic")
            .field("channel", &self.channel)
            .field("truncated", &self.truncated)
            .field("length", &self.bytes.len())
            .finish()
    }
}
#[derive(Clone, Debug, Default)]
pub struct Diagnostics(Arc<Mutex<Vec<Diagnostic>>>);
impl Diagnostics {
    pub fn snapshot(&self) -> Result<Vec<Diagnostic>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("diagnostic buffer poisoned"))?
            .clone())
    }
    fn record(&self, diagnostic: Diagnostic) -> Result<()> {
        let mut stored = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("diagnostic buffer poisoned"))?;
        ensure!(
            stored.len() < 2
                && !stored
                    .iter()
                    .any(|value| value.channel == diagnostic.channel),
            "duplicate daemon diagnostic"
        );
        stored.push(diagnostic);
        Ok(())
    }
}
pub async fn write_diagnostic(stream: &mut UnixStream, diagnostic: &Diagnostic) -> Result<()> {
    ensure!(
        diagnostic.channel <= 1 && diagnostic.bytes.len() <= DIAGNOSTIC_LIMIT,
        "invalid daemon diagnostic"
    );
    timeout(IO_TIMEOUT, async {
        stream.write_u8(Frame::Diagnostic as u8).await?;
        stream.write_u8(diagnostic.channel).await?;
        stream.write_u8(u8::from(diagnostic.truncated)).await?;
        stream
            .write_u16(u16::try_from(diagnostic.bytes.len())?)
            .await?;
        stream.write_all(&diagnostic.bytes).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("daemon diagnostic write timed out")??;
    Ok(())
}
async fn read_diagnostic(stream: &mut UnixStream) -> Result<Diagnostic> {
    timeout(IO_TIMEOUT, async {
        let channel = stream.read_u8().await?;
        let truncated = stream.read_u8().await?;
        let length = usize::from(stream.read_u16().await?);
        ensure!(
            channel <= 1 && truncated <= 1 && length <= DIAGNOSTIC_LIMIT,
            "invalid daemon diagnostic"
        );
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await?;
        Ok::<_, anyhow::Error>(Diagnostic {
            channel,
            truncated: truncated != 0,
            bytes,
        })
    })
    .await
    .context("daemon diagnostic read timed out")?
}

pub async fn read(stream: &mut UnixStream) -> Result<Frame> {
    Frame::try_from(
        stream
            .read_u8()
            .await
            .context("lifecycle connection closed")?,
    )
}
pub async fn write(stream: &mut UnixStream, frame: Frame) -> Result<()> {
    timeout(IO_TIMEOUT, stream.write_u8(frame as u8))
        .await
        .context("lifecycle write timed out")??;
    Ok(())
}
pub fn close_on_exec(stream: &UnixStream) -> Result<()> {
    // SAFETY: fcntl 只修改当前有效 socket 描述符的继承标志。
    let flags = unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) };
    ensure!(flags >= 0, "cannot inspect lifecycle descriptor");
    // SAFETY: 描述符仍由 stream 持有，设置 CLOEXEC 不改变其所有权。
    ensure!(
        unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) } == 0,
        "cannot protect lifecycle descriptor"
    );
    Ok(())
}

/// 连接只由后台协议任务持有；Drop 立即终止任务并关闭连接。
#[derive(Debug)]
pub struct Client {
    stop: mpsc::Sender<oneshot::Sender<Result<()>>>,
    task: JoinHandle<()>,
}
impl Client {
    pub async fn connect(
        path: &Path,
        deadline: Instant,
        cancel: CancellationToken,
    ) -> Result<Self> {
        Self::connect_recording(path, deadline, cancel, Diagnostics::default()).await
    }
    pub async fn connect_recording(
        path: &Path,
        deadline: Instant,
        cancel: CancellationToken,
        diagnostics: Diagnostics,
    ) -> Result<Self> {
        let startup = deadline.min(Instant::now() + START_TIMEOUT);
        let mut stream = loop {
            let attempt = timeout_at(startup, UnixStream::connect(path))
                .await
                .context("lifecycle connection timed out")?;
            match attempt {
                Ok(stream) => break stream,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    tokio::select! {
                        _ = cancel.cancelled() => bail!("image execution canceled"),
                        _ = tokio::time::sleep_until(startup) => bail!("lifecycle connection timed out"),
                        _ = tokio::time::sleep(Duration::from_millis(50)) => (),
                    }
                }
                Err(error) => return Err(error.into()),
            }
        };
        close_on_exec(&stream)?;
        let (ready, waiting) = oneshot::channel();
        let (stop, mut requests) = mpsc::channel::<oneshot::Sender<Result<()>>>(1);
        let task = tokio::spawn(async move {
            let mut ready = Some(ready);
            let result = async {
                let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(deadline) => bail!("buildkit lifecycle deadline exceeded"),
                        _ = ticker.tick() => write(&mut stream, Frame::Heartbeat).await?,
                        request = requests.recv() => {
                            let Some(reply) = request else { return Ok(()); };
                            let stopped = async {
                                write(&mut stream, Frame::Stop).await?;
                                loop {
                                    match read(&mut stream).await? {
                                        Frame::Stopped => return Ok(()),
                                        Frame::Diagnostic => diagnostics.record(read_diagnostic(&mut stream).await?)?,
                                        _ => bail!("BuildKit stop was not confirmed"),
                                    }
                                }
                            };
                            let result = timeout(STOP_TIMEOUT, stopped).await
                                .context("BuildKit stop timed out").and_then(|value| value);
                            let failed = result.is_err();
                            let _ = reply.send(result);
                            ensure!(!failed, "BuildKit stop failed");
                            return Ok(());
                        }
                        frame = read(&mut stream) => {
                            match frame? {
                                Frame::Ready => {
                                    let sender = ready.take().context("duplicate BuildKit readiness")?;
                                    let _ = sender.send(());
                                }
                                Frame::Diagnostic => diagnostics.record(read_diagnostic(&mut stream).await?)?,
                                _ => bail!("BuildKit exited or invalid lifecycle state"),
                            }
                        }
                    }
                }
            }.await;
            if result.is_err() {
                cancel.cancel();
            }
        });
        let client = Self { stop, task };
        timeout_at(startup, waiting)
            .await
            .context("BuildKit readiness timed out")?
            .context("BuildKit failed before readiness")?;
        Ok(client)
    }

    /// 成功返回只证明本地进程收尾，不表示镜像或业务执行成功。
    pub async fn stop(self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.stop
            .send(reply)
            .await
            .context("BuildKit lifecycle unavailable")?;
        timeout(STOP_TIMEOUT, result)
            .await
            .context("BuildKit stop confirmation timed out")?
            .context("BuildKit lifecycle stopped without confirmation")?
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn stop_requires_confirmation_and_expected_eof_does_not_cancel() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            close_on_exec(&stream).unwrap();
            write(&mut stream, Frame::Ready).await.unwrap();
            loop {
                match read(&mut stream).await.unwrap() {
                    Frame::Heartbeat => (),
                    Frame::Stop => {
                        write(&mut stream, Frame::Stopped).await.unwrap();
                        break;
                    }
                    frame => panic!("unexpected frame {frame:?}"),
                }
            }
        });
        let cancel = CancellationToken::new();
        let client = Client::connect(
            &path,
            Instant::now() + Duration::from_secs(3),
            cancel.clone(),
        )
        .await
        .unwrap();
        client.stop().await.unwrap();
        server.await.unwrap();
        assert!(!cancel.is_cancelled());
    }

    #[tokio::test]
    async fn daemon_eof_cancels_runner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write(&mut stream, Frame::Ready).await.unwrap();
        });
        let cancel = CancellationToken::new();
        let client = Client::connect(
            &path,
            Instant::now() + Duration::from_secs(3),
            cancel.clone(),
        )
        .await
        .unwrap();
        timeout(Duration::from_secs(1), cancel.cancelled())
            .await
            .unwrap();
        assert!(client.stop().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn dropped_client_closes_socket_without_heartbeat_references() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write(&mut stream, Frame::Ready).await.unwrap();
            while let Ok(frame) = read(&mut stream).await {
                assert_eq!(frame, Frame::Heartbeat);
            }
        });
        let client = Client::connect(
            &path,
            Instant::now() + Duration::from_secs(3),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        drop(client);
        timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn missing_supervisor_connection_has_a_hard_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let result = Client::connect(
            &directory.path().join("missing.sock"),
            Instant::now() + Duration::from_millis(20),
            CancellationToken::new(),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn eof_after_stop_without_confirmation_is_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write(&mut stream, Frame::Ready).await.unwrap();
            while read(&mut stream).await.unwrap() != Frame::Stop {}
        });
        let cancel = CancellationToken::new();
        let client = Client::connect(
            &path,
            Instant::now() + Duration::from_secs(3),
            cancel.clone(),
        )
        .await
        .unwrap();
        assert!(client.stop().await.is_err());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn startup_failure_keeps_bounded_diagnostics_without_printing_raw_debug() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            write_diagnostic(
                &mut stream,
                &Diagnostic {
                    channel: 1,
                    truncated: false,
                    bytes: b"private-password".to_vec(),
                },
            )
            .await
            .unwrap();
            write(&mut stream, Frame::Failed).await.unwrap();
        });
        let diagnostics = Diagnostics::default();
        assert!(
            Client::connect_recording(
                &path,
                Instant::now() + Duration::from_secs(3),
                CancellationToken::new(),
                diagnostics.clone()
            )
            .await
            .is_err()
        );
        let stored = diagnostics.snapshot().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].bytes, b"private-password");
        assert!(!format!("{diagnostics:?}").contains("private-password"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_diagnostic_is_rejected_before_allocating_payload() {
        let (mut writer, mut reader) = UnixStream::pair().unwrap();
        writer.write_u8(1).await.unwrap();
        writer.write_u8(0).await.unwrap();
        writer
            .write_u16(u16::try_from(DIAGNOSTIC_LIMIT + 1).unwrap())
            .await
            .unwrap();
        assert!(read_diagnostic(&mut reader).await.is_err());
        assert!(
            write_diagnostic(
                &mut writer,
                &Diagnostic {
                    channel: 0,
                    truncated: false,
                    bytes: vec![0; DIAGNOSTIC_LIMIT + 1]
                }
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn unknown_frames_are_rejected_and_descriptor_is_not_inherited() {
        assert!(Frame::try_from(0).is_err());
        let (stream, _) = std::os::unix::net::UnixStream::pair().unwrap();
        stream.set_nonblocking(true).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();
        let stream = UnixStream::from_std(stream).unwrap();
        close_on_exec(&stream).unwrap();
        // SAFETY: 只读取当前有效 socket 的描述符标志。
        assert_ne!(
            unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
            0
        );
    }
}
