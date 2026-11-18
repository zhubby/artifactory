//! ```text
//! [INPUT]: 依赖容器 PID namespace、fsGroup 卷、RootlessKit 和本地连接。
//! [OUTPUT]: 监督一次性 daemon，限定就绪、租约、停机和进程回收时间。
//! [POS]: BuildKit 容器 PID 1；不链接任务、仓库认证或控制平面能力。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use crate::lifecycle::{self, Frame};
use anyhow::{Context, Result, bail, ensure};
use std::sync::{Arc, Mutex};
use std::{
    collections::{BTreeMap, VecDeque},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    process::{Child, Command},
    signal::unix::{SignalKind, signal},
    task::JoinHandle,
    time::{Instant, timeout_at},
};

struct Config {
    socket: PathBuf,
    deadline: Instant,
    daemon: Command,
    probe: Vec<String>,
    verify_namespace: bool,
}
impl Config {
    fn environment() -> Result<Self> {
        ensure!(
            std::process::id() == 1,
            "BuildKit supervisor must be container PID 1"
        );
        ensure!(
            !std::env::var("POD_UID")?.is_empty(),
            "Pod identity required"
        );
        let seconds: u64 = std::env::var("ARTIFACTORY_BUILDKIT_TIMEOUT_SECONDS")?.parse()?;
        ensure!(
            (1..=604800).contains(&seconds),
            "invalid BuildKit task deadline"
        );
        let mut daemon = Command::new("rootlesskit");
        daemon.args([
            "buildkitd",
            "--addr",
            lifecycle::DAEMON_ADDRESS,
            "--oci-worker-no-process-sandbox",
        ]);
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        ensure!(
            args.is_empty()
                || (args.len() == 2
                    && args[0] == "--config"
                    && args[1] == "/etc/buildkit/buildkitd.toml"),
            "unsupported BuildKit supervisor arguments"
        );
        daemon.args(args);
        Ok(Self {
            socket: PathBuf::from(lifecycle::SOCKET),
            deadline: Instant::now() + Duration::from_secs(seconds),
            daemon,
            probe: [
                "buildctl",
                "--addr",
                lifecycle::DAEMON_ADDRESS,
                "debug",
                "workers",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            verify_namespace: true,
        })
    }
}

#[derive(Clone, Copy)]
struct Identity {
    parent: u32,
    started: u64,
}
fn identity(pid: u32) -> Option<Identity> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = stat
        .get(stat.rfind(')')? + 2..)?
        .split_whitespace()
        .collect();
    Some(Identity {
        parent: fields.get(1)?.parse().ok()?,
        started: fields.get(19)?.parse().ok()?,
    })
}
fn processes() -> BTreeMap<u32, Identity> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|entry| {
            let pid = entry.ok()?.file_name().to_str()?.parse().ok()?;
            Some((pid, identity(pid)?))
        })
        .collect()
}
struct Tree {
    root: u32,
    members: BTreeMap<u32, u64>,
}
impl Tree {
    fn new(root: u32) -> Self {
        let mut members = BTreeMap::new();
        if let Some(found) = identity(root) {
            members.insert(root, found.started);
        }
        Self { root, members }
    }
    fn refresh(&mut self) {
        let all = processes();
        loop {
            let before = self.members.len();
            for (&pid, found) in &all {
                let known_parent = self.members.get(&found.parent).is_some_and(|started| {
                    all.get(&found.parent)
                        .is_some_and(|parent| parent.started == *started)
                });
                let adopted = std::process::id() == 1 && found.parent == 1;
                if known_parent || adopted {
                    self.members.entry(pid).or_insert(found.started);
                }
            }
            if before == self.members.len() {
                break;
            }
        }
    }
    fn alive(&self, pid: u32, started: u64) -> bool {
        identity(pid).is_some_and(|value| value.started == started)
    }
    fn signal(&self, number: i32) -> Result<()> {
        for (&pid, &started) in &self.members {
            if !self.alive(pid, started) {
                continue;
            }
            // SAFETY: 身份由 PID 和启动时间共同验证，不向进程组或宿主发送信号。
            if unsafe { libc::kill(i32::try_from(pid)?, number) } != 0 {
                let error = std::io::Error::last_os_error();
                if !matches!(error.raw_os_error(), Some(libc::ESRCH | libc::EPERM)) {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }
    fn reap_adopted(&self) {
        for (&pid, &started) in &self.members {
            if pid == self.root || !self.alive(pid, started) {
                continue;
            }
            if identity(pid).is_some_and(|value| value.parent == std::process::id()) {
                // SAFETY: 只回收已记录的直系后代，root 仍交由 Tokio Child 回收。
                unsafe {
                    libc::waitpid(pid as libc::pid_t, std::ptr::null_mut(), libc::WNOHANG);
                }
            }
        }
    }
    fn empty(&self) -> bool {
        self.members
            .iter()
            .all(|(&pid, &started)| !self.alive(pid, started))
    }
    fn container_namespace(&self) -> Result<()> {
        let own = std::fs::read_link("/proc/self/ns/pid")?;
        ensure!(
            std::fs::read_link("/proc/1/ns/pid")? == own,
            "container procfs exposes another PID namespace"
        );
        for (&pid, &started) in &self.members {
            if !self.alive(pid, started) {
                continue;
            }
            if std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .unwrap_or_default()
                .trim()
                != "buildkitd"
            {
                continue;
            }
            let namespace = std::fs::read_link(format!("/proc/{pid}/ns/pid"))?;
            ensure!(
                namespace == own,
                "BuildKit escaped its container PID namespace"
            );
            return Ok(());
        }
        bail!("BuildKit daemon identity unavailable")
    }
}

#[derive(Default)]
struct Tail {
    bytes: VecDeque<u8>,
    truncated: bool,
}
impl Tail {
    fn push(&mut self, bytes: &[u8]) {
        let excess = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(lifecycle::DIAGNOSTIC_LIMIT);
        if excess > 0 {
            self.truncated = true;
            self.bytes.drain(..excess.min(self.bytes.len()));
        }
        let keep = bytes.len().min(lifecycle::DIAGNOSTIC_LIMIT);
        self.bytes
            .extend(bytes[bytes.len() - keep..].iter().copied());
    }
}
struct Log {
    task: JoinHandle<Result<()>>,
    tail: Arc<Mutex<Tail>>,
    channel: u8,
}
impl Log {
    fn read(mut pipe: impl tokio::io::AsyncRead + Unpin + Send + 'static, channel: u8) -> Self {
        let tail = Arc::new(Mutex::new(Tail::default()));
        let output = tail.clone();
        let task = tokio::spawn(async move {
            let mut buffer = [0; 8192];
            loop {
                let size = pipe.read(&mut buffer).await?;
                if size == 0 {
                    return Ok(());
                }
                output
                    .lock()
                    .map_err(|_| anyhow::anyhow!("daemon diagnostic buffer poisoned"))?
                    .push(&buffer[..size]);
            }
        });
        Self {
            task,
            tail,
            channel,
        }
    }
    fn diagnostic(&self) -> Result<lifecycle::Diagnostic> {
        let tail = self
            .tail
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon diagnostic buffer poisoned"))?;
        Ok(lifecycle::Diagnostic {
            channel: self.channel,
            truncated: tail.truncated,
            bytes: tail.bytes.iter().copied().collect(),
        })
    }
}
struct Daemon {
    child: Child,
    tree: Tree,
    logs: Vec<Log>,
}
impl Daemon {
    fn spawn(mut command: Command) -> Result<Self> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let parent = std::process::id();
        // SAFETY: pre_exec 只调用异步信号安全的 prctl/getppid，不分配内存或获取锁。
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent as libc::pid_t {
                    return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
                }
                Ok(())
            });
        }
        let mut child = command.spawn().context("cannot start rootless BuildKit")?;
        let tree = Tree::new(child.id().context("daemon PID missing")?);
        let stdout = child.stdout.take().context("daemon stdout missing")?;
        let stderr = child.stderr.take().context("daemon stderr missing")?;
        // 只保存有界尾部；失败时交给 Runner 脱敏，禁止写入原文 Pod 日志。
        let logs = vec![Log::read(stdout, 0), Log::read(stderr, 1)];
        Ok(Self { child, tree, logs })
    }
    fn live(&mut self) -> Result<()> {
        self.tree.refresh();
        ensure!(
            self.child.try_wait()?.is_none(),
            "BuildKit exited before requested stop"
        );
        ensure!(
            self.logs.iter().all(|log| !log.task.is_finished()),
            "BuildKit log stream closed unexpectedly"
        );
        Ok(())
    }
    async fn stop(&mut self) -> Result<()> {
        let end = Instant::now() + lifecycle::STOP_TIMEOUT - Duration::from_millis(200);
        let force = Instant::now() + Duration::from_secs(5);
        loop {
            self.tree.refresh();
            self.tree.signal(if Instant::now() < force {
                libc::SIGTERM
            } else {
                libc::SIGKILL
            })?;
            let _ = self.child.try_wait()?;
            self.tree.reap_adopted();
            if self.tree.empty() {
                break;
            }
            ensure!(
                Instant::now() < end,
                "BuildKit descendants remain after stop deadline"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.child.wait().await?;
        for log in &mut self.logs {
            timeout_at(end, &mut log.task)
                .await
                .context("BuildKit log drain timed out")???;
        }
        Ok(())
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        self.tree.refresh();
        let _ = self.tree.signal(libc::SIGKILL);
        for log in &self.logs {
            log.task.abort();
        }
    }
}

async fn probe(args: &[String]) -> Result<bool> {
    let (program, args) = args.split_first().context("readiness command missing")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(status) => Ok(status?.success()),
        Err(_) => {
            child.kill().await?;
            Ok(false)
        }
    }
}

async fn serve(config: Config) -> Result<()> {
    // SAFETY: subreaper 只改变本进程的孤儿后代归属，确保独立 session 仍可回收。
    ensure!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) } == 0,
        "cannot enable descendant reaper"
    );
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let startup = config
        .deadline
        .min(Instant::now() + lifecycle::START_TIMEOUT);
    let parent = config.socket.parent().context("lifecycle parent missing")?;
    std::fs::create_dir_all(parent)?;
    // emptyDir 的属主由 kubelet 保持为 root；fsGroup 只授予写权，不能 chmod 挂载根。
    let listener =
        UnixListener::bind(&config.socket).context("cannot bind task lifecycle socket")?;
    std::fs::set_permissions(&config.socket, std::fs::Permissions::from_mode(0o660))?;
    let accepted = tokio::select! {
        result = timeout_at(startup, listener.accept()) => result.context("Runner connection timed out")??,
        _ = term.recv() => bail!("BuildKit interrupted before Runner connected"),
        _ = interrupt.recv() => bail!("BuildKit interrupted before Runner connected"),
    };
    drop(listener);
    std::fs::remove_file(&config.socket)?;
    let mut stream = accepted.0;
    lifecycle::close_on_exec(&stream)?;
    let mut daemon = Daemon::spawn(config.daemon)?;
    let outcome = async {
        let mut lease = Instant::now() + lifecycle::HEARTBEAT_LEASE;
        let mut ready = false;
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = term.recv() => bail!("BuildKit interrupted"),
                _ = interrupt.recv() => bail!("BuildKit interrupted"),
                _ = tokio::time::sleep_until(config.deadline) => bail!("BuildKit task deadline exceeded"),
                _ = tokio::time::sleep_until(lease) => bail!("Runner lifecycle lease expired"),
                frame = lifecycle::read(&mut stream) => match frame? {
                    Frame::Heartbeat => lease = Instant::now() + lifecycle::HEARTBEAT_LEASE,
                    Frame::Stop if ready => { daemon.live()?; return Ok(()); }
                    _ => bail!("invalid Runner lifecycle state"),
                },
                _ = tick.tick() => {
                    daemon.live()?;
                    if !ready {
                        ensure!(Instant::now() < startup, "BuildKit readiness timed out");
                        if probe(&config.probe).await? {
                            daemon.tree.refresh();
                            if config.verify_namespace { daemon.tree.container_namespace()?; }
                            lifecycle::write(&mut stream, Frame::Ready).await?;
                            ready = true;
                        }
                    }
                }
            }
        }
    }.await;
    let stopped = daemon.stop().await;
    let terminal = match (outcome, stopped) {
        (Ok(()), Ok(())) => lifecycle::write(&mut stream, Frame::Stopped).await,
        (Err(error), _) | (_, Err(error)) => {
            for log in &daemon.logs {
                if let Ok(diagnostic) = log.diagnostic()
                    && !diagnostic.bytes.is_empty()
                {
                    let _ = lifecycle::write_diagnostic(&mut stream, &diagnostic).await;
                }
            }
            let _ = lifecycle::write(&mut stream, Frame::Failed).await;
            Err(error)
        }
    };
    // Unix socket 带未读心跳直接 close 能触发 reset，丢失已写出的诊断或确认。
    let _ = stream.shutdown().await;
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let mut remaining = [0; 256];
        while stream.read(&mut remaining).await? != 0 {}
        Ok::<_, std::io::Error>(())
    })
    .await;
    terminal
}

pub async fn run() -> Result<()> {
    serve(Config::environment()?).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::Client;
    use tokio_util::sync::CancellationToken;

    fn test_config(path: PathBuf, script: &str) -> Config {
        let mut daemon = Command::new("/bin/sh");
        daemon.args(["-c", script]);
        Config {
            socket: path,
            deadline: Instant::now() + Duration::from_secs(20),
            daemon,
            probe: vec!["/bin/true".into()],
            verify_namespace: false,
        }
    }
    #[test]
    fn diagnostic_tail_has_a_fixed_memory_bound() {
        let mut tail = Tail::default();
        tail.push(&vec![b'a'; lifecycle::DIAGNOSTIC_LIMIT * 3]);
        tail.push(b"final failure");
        assert!(tail.truncated);
        assert_eq!(tail.bytes.len(), lifecycle::DIAGNOSTIC_LIMIT);
        assert!(
            tail.bytes
                .iter()
                .copied()
                .collect::<Vec<_>>()
                .ends_with(b"final failure")
        );
    }
    #[tokio::test]
    async fn failed_daemon_delivers_diagnostics_only_to_its_runner() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(
            socket.clone(),
            "printf 'daemon-secret' >&2; exit 1",
        )));
        let diagnostics = lifecycle::Diagnostics::default();
        let cancel = CancellationToken::new();
        if let Ok(client) = Client::connect_recording(
            &socket,
            Instant::now() + Duration::from_secs(3),
            cancel.clone(),
            diagnostics.clone(),
        )
        .await
        {
            tokio::time::timeout(Duration::from_secs(2), cancel.cancelled())
                .await
                .unwrap();
            drop(client);
        }
        assert!(server.await.unwrap().is_err());
        let stored = diagnostics.snapshot().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].channel, 1);
        assert_eq!(stored[0].bytes, b"daemon-secret");
    }
    #[tokio::test]
    async fn stop_reaps_real_descendants_and_escaped_session() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let pids = directory.path().join("pids");
        let script = format!(
            "setsid /bin/sh -c 'trap \"\" TERM; echo $$ > {}; exec sleep 90' & wait",
            pids.display()
        );
        let mut config = test_config(socket.clone(), &script);
        config.probe = vec![
            "/bin/sh".into(),
            "-c".into(),
            format!("test -s {}", pids.display()),
        ];
        let server = tokio::spawn(serve(config));
        let client = Client::connect(
            &socket,
            Instant::now() + Duration::from_secs(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let pid: u32 = tokio::fs::read_to_string(&pids)
            .await
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        client.stop().await.unwrap();
        server.await.unwrap().unwrap();
        assert!(identity(pid).is_none(), "escaped descendant must be reaped");
    }
    #[tokio::test]
    async fn runner_eof_fails_supervisor_and_kills_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(socket.clone(), "exec sleep 90")));
        let client = Client::connect(
            &socket,
            Instant::now() + Duration::from_secs(20),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        drop(client);
        assert!(
            tokio::time::timeout(Duration::from_secs(16), server)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }
    #[tokio::test]
    async fn hard_deadline_stops_daemon_even_with_live_runner_connection() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let mut config = test_config(socket.clone(), "exec sleep 90");
        config.deadline = Instant::now() + Duration::from_secs(1);
        let server = tokio::spawn(serve(config));
        let cancel = CancellationToken::new();
        let client = Client::connect(
            &socket,
            Instant::now() + Duration::from_secs(5),
            cancel.clone(),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), cancel.cancelled())
            .await
            .unwrap();
        assert!(server.await.unwrap().is_err());
        assert!(client.stop().await.is_err());
    }
    #[tokio::test]
    async fn invalid_protocol_state_fails_and_reaps_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(socket.clone(), "exec sleep 90")));
        let mut stream = loop {
            if let Ok(stream) = tokio::net::UnixStream::connect(&socket).await {
                break stream;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        while lifecycle::read(&mut stream).await.unwrap() != Frame::Ready {}
        lifecycle::write(&mut stream, Frame::Stopped).await.unwrap();
        assert_eq!(lifecycle::read(&mut stream).await.unwrap(), Frame::Failed);
        assert!(server.await.unwrap().is_err());
    }
    #[tokio::test]
    async fn open_connection_without_heartbeats_expires_and_reaps_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(socket.clone(), "exec sleep 90")));
        let mut stream = loop {
            if let Ok(stream) = tokio::net::UnixStream::connect(&socket).await {
                break stream;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(lifecycle::read(&mut stream).await.unwrap(), Frame::Ready);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(12), lifecycle::read(&mut stream))
                .await
                .unwrap()
                .unwrap(),
            Frame::Failed
        );
        assert!(server.await.unwrap().is_err());
    }
    #[tokio::test]
    async fn duplicate_stop_cannot_restart_daemon_or_open_another_connection() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(socket.clone(), "exec sleep 90")));
        let mut stream = loop {
            if let Ok(stream) = tokio::net::UnixStream::connect(&socket).await {
                break stream;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        assert_eq!(lifecycle::read(&mut stream).await.unwrap(), Frame::Ready);
        assert!(tokio::net::UnixStream::connect(&socket).await.is_err());
        lifecycle::write(&mut stream, Frame::Stop).await.unwrap();
        lifecycle::write(&mut stream, Frame::Stop).await.unwrap();
        assert_eq!(lifecycle::read(&mut stream).await.unwrap(), Frame::Stopped);
        server.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn daemon_clean_early_exit_is_failure() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("lifecycle.sock");
        let server = tokio::spawn(serve(test_config(socket.clone(), "sleep 0.1; exit 0")));
        let cancel = CancellationToken::new();
        if let Ok(client) = Client::connect(
            &socket,
            Instant::now() + Duration::from_secs(3),
            cancel.clone(),
        )
        .await
        {
            tokio::time::timeout(Duration::from_secs(2), cancel.cancelled())
                .await
                .unwrap();
            assert!(client.stop().await.is_err());
        }
        assert!(server.await.unwrap().is_err());
    }
}
