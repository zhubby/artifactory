//! [INPUT]: 依赖原生 PTY、平台进程树和调用方有界通道。
//! [OUTPUT]: 驱动注册、有界输入与窗口；取消结束Unix读写，失败拒绝正常退出。
//! [POS]: 终端运行循环；Unix非阻塞读写可取消，ConPTY关闭与排空并行。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
#[cfg(unix)]
use super::unix as platform;
#[cfg(windows)]
use super::windows as platform;
use super::{
    MAX_FRAME_BYTES, TerminalConfig, TerminalError, TerminalEvent, TerminalInput, validate_size,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{self, Read, Write},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// PTY 私有队列最多四帧；消费端停顿不会无限积累输出。
const READ_QUEUE_FRAMES: usize = 4;
/// 写线程最多十六帧，主循环额外保留一帧并继续监督取消和退出。
const WRITE_QUEUE_FRAMES: usize = 16;
const INPUT_STALL_TIMEOUT: Duration = Duration::from_secs(3);
/// 退出后只等待有界排空，不让慢消费永久保留已结束终端。
const FINISH_TIMEOUT: Duration = Duration::from_secs(3);
const REGISTRATION_TIMEOUT: Duration = Duration::from_secs(5);

struct PtySession {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    tree: platform::ProcessTree,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    process_id: u32,
}

impl PtySession {
    fn spawn(config: TerminalConfig) -> Result<Self, TerminalError> {
        validate(&config)?;
        let pair = native_pty_system()
            .openpty(size(config.columns, config.rows))
            .map_err(|error| TerminalError::caused("could not open terminal PTY", error))?;
        let mut command = CommandBuilder::new(&config.shell);
        command.args(&config.args);
        command.cwd(&config.cwd);
        command.env_clear();
        for (key, value) in &config.environment {
            command.env(key, value);
        }
        command.env("TERM", &config.term);
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| TerminalError::caused("could not spawn terminal shell", error))?;
        drop(pair.slave);
        let tree = match platform::ProcessTree::attach(&*child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let process_id = child
            .process_id()
            .ok_or_else(|| TerminalError::invalid("terminal process ID is unavailable"))?;
        #[cfg(unix)]
        let (reader, writer) = platform::io_handles(&*pair.master)?;
        #[cfg(windows)]
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TerminalError::caused("could not clone terminal reader", error))?;
        #[cfg(windows)]
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TerminalError::caused("could not open terminal writer", error))?;
        Ok(Self {
            child,
            master: pair.master,
            tree,
            reader,
            writer,
            process_id,
        })
    }
}

fn validate(config: &TerminalConfig) -> Result<(), TerminalError> {
    validate_size(config.columns, config.rows)?;
    if !config.shell.is_absolute() || !config.shell.is_file() {
        return Err(TerminalError::invalid(
            "terminal shell must be an existing absolute file",
        ));
    }
    if !config.cwd.is_absolute() || !config.cwd.is_dir() {
        return Err(TerminalError::invalid(
            "terminal directory must be an existing absolute directory",
        ));
    }
    if config.term.is_empty()
        || config.term.len() > 64
        || !config
            .term
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"-_.".contains(&v))
    {
        return Err(TerminalError::invalid("invalid terminal type"));
    }
    if config.args.len() > 64
        || config
            .args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\0'))
    {
        return Err(TerminalError::invalid("invalid configured shell arguments"));
    }
    if config.environment.len() > 64
        || config.environment.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 128
                || key.bytes().any(|c| !c.is_ascii_alphanumeric() && c != b'_')
                || value.contains('\0')
        })
        || config
            .environment
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > MAX_FRAME_BYTES
    {
        return Err(TerminalError::invalid(
            "invalid configured terminal environment",
        ));
    }
    Ok(())
}

fn size(columns: u16, rows: u16) -> PtySize {
    PtySize {
        cols: columns,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}

enum ReadResult {
    Data(Vec<u8>),
    End,
    Failed(io::Error),
}

fn read_loop(
    mut reader: Box<dyn Read + Send>,
    tx: mpsc::Sender<ReadResult>,
    discard: CancellationToken,
    stop: CancellationToken,
) {
    let mut buffer = vec![0; MAX_FRAME_BYTES];
    loop {
        if cfg!(unix) && stop.is_cancelled() {
            return;
        }
        let mut result = match reader.read(&mut buffer) {
            Ok(0) => ReadResult::End,
            Ok(count) => ReadResult::Data(buffer[..count].to_vec()),
            // Unix PTY 在最后一个 slave 关闭后可用 EIO 表示 EOF。
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(libc::EIO) => ReadResult::End,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            #[cfg(unix)]
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            Err(error) => ReadResult::Failed(error),
        };
        let end = !matches!(result, ReadResult::Data(_));
        loop {
            match tx.try_send(result) {
                Ok(()) => break,
                Err(mpsc::error::TrySendError::Closed(_)) => return,
                Err(mpsc::error::TrySendError::Full(value)) => {
                    if discard.is_cancelled() {
                        break;
                    }
                    result = value;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        if end {
            return;
        }
    }
}

/// 执行单个真实终端；调用方关闭输入或取消时回收进程树。
///
/// # Errors
/// 拒绝无效配置、超额输入及窗口，并报告 PTY、写入或消费端失败。
/// Ready 只在真实 PTY 与进程监督均成功后产生。
pub async fn run(
    config: TerminalConfig,
    mut input: mpsc::Receiver<TerminalInput>,
    events: mpsc::Sender<TerminalEvent>,
    cancel: CancellationToken,
) -> Result<(), TerminalError> {
    let mut registered = !config.require_registration;
    let setup = tokio::task::spawn_blocking(move || PtySession::spawn(config))
        .await
        .map_err(|error| TerminalError::caused("terminal setup task failed", error))??;
    let PtySession {
        mut child,
        master,
        tree,
        reader,
        mut writer,
        process_id,
    } = setup;
    let discard = CancellationToken::new();
    let (read_tx, mut read_rx) = mpsc::channel(READ_QUEUE_FRAMES);
    let reader_discard = discard.clone();
    let io_stop = CancellationToken::new();
    let _writer_stop_on_drop = io_stop.clone().drop_guard();
    let reader_stop = CancellationToken::new();
    let _reader_stop_on_drop = reader_stop.clone().drop_guard();
    let reader_cancel = reader_stop.clone();
    let reader_task = tokio::task::spawn_blocking(move || {
        read_loop(reader, read_tx, reader_discard, reader_stop)
    });
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(WRITE_QUEUE_FRAMES);
    let (writer_error, mut writer_errors) = mpsc::channel(1);
    #[cfg(unix)]
    let writer_stop = io_stop.clone();
    let writer_task = tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        while let Some(data) = write_rx.blocking_recv() {
            if let Err(error) = writer.write_all(&data) {
                let _ = writer_error.try_send(error);
                break;
            }
        }
        #[cfg(unix)]
        while let Some(data) = write_rx.blocking_recv() {
            let mut remaining = data.as_slice();
            while !remaining.is_empty() && !writer_stop.is_cancelled() {
                match writer.write(remaining) {
                    Ok(0) => {
                        let _ = writer_error.try_send(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "terminal writer made no progress",
                        ));
                        return;
                    }
                    Ok(count) => remaining = &remaining[count..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => {
                        let _ = writer_error.try_send(error);
                        return;
                    }
                }
            }
            if writer_stop.is_cancelled() {
                return;
            }
        }
    });
    let mut write_tx = Some(write_tx);
    let mut master = Some(master);
    let mut master_close = None;
    let mut pending_write = None;
    let mut write_stalled_at = None;
    let mut pending = Some(TerminalEvent::Ready { process_id });
    let mut status = None;
    let mut read_ended = false;
    let mut closing = false;
    let mut finish_at = None;
    let mut failure = None;
    let mut cleanup = None;
    let mut cleanup_done = false;
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    let register_by = tokio::time::Instant::now() + REGISTRATION_TIMEOUT;

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled(), if !closing => { closing = true; discard.cancel(); pending = None; }
            _ = events.closed(), if !closing => {
                failure = Some(TerminalError::invalid("terminal event consumer closed")); closing = true; discard.cancel(); pending = None;
            }
            _ = tick.tick() => {
                if !closing && write_stalled_at.is_some_and(|at| tokio::time::Instant::now() >= at) {
                    failure = Some(TerminalError::invalid("terminal input writer stalled")); closing = true;
                }
                if !registered && !closing && tokio::time::Instant::now() >= register_by {
                    failure = Some(TerminalError::invalid("terminal parent registration timed out")); closing = true;
                }
                // 注册前不 wait/reap。即刻退出的 Shell 也须保留 SID 与出生时间。
                if status.is_none() && (registered || closing) {
                    match child.try_wait() {
                        Ok(Some(exit)) => { status = Some(exit); closing = true; }
                        Ok(None) => {},
                        Err(error) => { failure = Some(TerminalError::caused("could not wait for terminal", error)); closing = true; }
                    }
                }
                if finish_at.is_some_and(|at| tokio::time::Instant::now() >= at) {
                    if !read_ended || pending.is_some() { failure.get_or_insert_with(|| TerminalError::invalid("terminal output did not drain before shutdown")); }
                    discard.cancel(); break;
                }
            }
            message = input.recv(), if !closing && pending_write.is_none() => {
                match message {
                    Some(TerminalInput::Registered) => {
                        if registered { failure = Some(TerminalError::invalid("duplicate terminal registration")); closing = true; }
                        else { registered = true; }
                    }
                    Some(TerminalInput::Input { data }) => {
                        if !registered { failure = Some(TerminalError::invalid("terminal input before parent registration")); closing = true; }
                        else if data.len() > MAX_FRAME_BYTES { failure = Some(TerminalError::invalid("terminal input exceeds frame limit")); closing = true; }
                        else if !data.is_empty() {
                            pending_write = Some(data);
                            write_stalled_at = Some(tokio::time::Instant::now() + INPUT_STALL_TIMEOUT);
                        }
                    }
                    Some(TerminalInput::Resize { columns, rows }) => {
                        if let Err(error) = validate_size(columns, rows).and_then(|()| master.as_ref().expect("open terminal master").resize(size(columns, rows)).map_err(|error| TerminalError::caused("could not resize terminal", error))) {
                            failure = Some(error); closing = true;
                        }
                    }
                    Some(TerminalInput::Close) | None => { closing = true; discard.cancel(); pending = None; }
                }
            }
            permit = async { write_tx.as_ref().expect("open terminal writer").reserve().await }, if !closing && pending_write.is_some() => {
                match permit {
                    Ok(permit) => {
                        permit.send(pending_write.take().expect("pending terminal input"));
                        write_stalled_at = None;
                    }
                    Err(_) => { failure = Some(TerminalError::invalid("terminal input writer closed")); closing = true; }
                }
            }
            permit = events.reserve(), if pending.is_some() && !events.is_closed() => {
                if let Ok(permit) = permit && let Some(event) = pending.take() { permit.send(event); }
            }
            result = read_rx.recv(), if pending.is_none() && !read_ended => {
                match result {
                    Some(ReadResult::Data(data)) if !discard.is_cancelled() => pending = Some(TerminalEvent::Output { data }),
                    Some(ReadResult::Data(_)) => {},
                    Some(ReadResult::End) | None => read_ended = true,
                    Some(ReadResult::Failed(error)) => { read_ended = true; failure = Some(TerminalError::caused("could not read terminal", error)); closing = true; }
                }
            }
            Some(error) = writer_errors.recv(), if !closing => {
                failure = Some(TerminalError::caused("could not write terminal", error)); closing = true;
            }
        }
        if closing && cleanup.is_none() && !cleanup_done {
            pending_write = None;
            write_stalled_at = None;
            write_tx.take();
            io_stop.cancel();
            finish_at = Some(tokio::time::Instant::now() + FINISH_TIMEOUT);
            let cleanup_tree = tree.clone();
            cleanup = Some(tokio::task::spawn_blocking(move || {
                cleanup_tree.terminate()
            }));
            // ConPTY 只有关闭 HPCON 才结束输出管道；关闭必须与读线程排空并行。
            #[cfg(windows)]
            if let Some(master) = master.take() {
                master_close = Some(tokio::task::spawn_blocking(move || drop(master)));
            }
        }
        if cleanup
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
            && let Some(task) = cleanup.take()
        {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(error) => {
                    failure.get_or_insert_with(|| {
                        TerminalError::caused("terminal cleanup task failed", error)
                    });
                }
            }
            cleanup_done = true;
        }
        if closing && status.is_some() && read_ended && pending.is_none() && cleanup_done {
            break;
        }
    }
    discard.cancel();
    io_stop.cancel();
    reader_cancel.cancel();
    drop(read_rx);
    drop(write_tx);
    // Unix 在读线程排空后关闭 master；ConPTY 已在关闭阶段并行排空。
    if let Some(task) = cleanup {
        let result = match tokio::time::timeout(FINISH_TIMEOUT, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(TerminalError::caused("terminal cleanup task failed", error)),
            Err(_) => Err(TerminalError::invalid("terminal cleanup task timed out")),
        };
        if let Err(error) = result {
            failure.get_or_insert(error);
        }
    }
    let master_close = master_close
        .take()
        .unwrap_or_else(|| tokio::task::spawn_blocking(move || drop(master.take())));
    for (task, operation) in [
        (master_close, "terminal master close task did not finish"),
        (reader_task, "terminal reader task did not finish"),
        (writer_task, "terminal writer task did not finish"),
    ] {
        match tokio::time::timeout(FINISH_TIMEOUT, task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                failure.get_or_insert_with(|| TerminalError::caused(operation, error));
            }
            Err(_) => {
                failure.get_or_insert_with(|| TerminalError::invalid(operation));
            }
        }
    }
    if let Ok(error) = writer_errors.try_recv() {
        failure.get_or_insert_with(|| TerminalError::caused("could not write terminal", error));
    }
    if status.is_none() {
        status = child
            .try_wait()
            .map_err(|error| TerminalError::caused("could not reap terminal", error))?;
    }
    if status.is_none() {
        failure.get_or_insert_with(|| TerminalError::invalid("terminal process did not exit"));
    }
    // 已知失败必须先交由 Worker 报错。Exited 表示字节与回收均已成功。
    if let Some(error) = failure {
        return Err(error);
    }
    if let Some(exit) = status {
        let signal = exit.signal().map(str::to_owned);
        // Windows 原始退出码为 u32，按位保留 NTSTATUS 高位。
        let code = if signal.is_some() {
            None
        } else {
            Some(i32::from_ne_bytes(exit.exit_code().to_ne_bytes()))
        };
        if !events.is_closed() {
            tokio::time::timeout(
                FINISH_TIMEOUT,
                events.send(TerminalEvent::Exited { code, signal }),
            )
            .await
            .map_err(|_| TerminalError::invalid("terminal exit event consumer stalled"))?
            .map_err(|_| TerminalError::invalid("terminal event consumer closed"))?;
        }
    }
    Ok(())
}
