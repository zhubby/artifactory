//! [INPUT]: 依赖 PTY master 描述符、进程 SID、内核信号和进程清单。
//! [OUTPUT]: 提供非阻塞 PTY 读写并回收同一会话的进程。
//! [POS]: Unix 终端监督边界；PID 只来自受信 PTY 创建回执。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::TerminalError;
use std::{
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

type IoHandles = (Box<dyn Read + Send>, Box<dyn Write + Send>);

/// Unix PTY I/O 使用可取消的非阻塞副本，避免满输入永久占据阻塞线程。
pub(super) fn io_handles(master: &dyn portable_pty::MasterPty) -> Result<IoHandles, TerminalError> {
    let fd = master
        .as_raw_fd()
        .ok_or_else(|| TerminalError::invalid("PTY master descriptor is unavailable"))?;
    // SAFETY: F_DUPFD_CLOEXEC 复制尚在本函数中受借用的描述符。
    let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(TerminalError::caused(
            "could not duplicate PTY master",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: duplicate 是刚取得且尚未转移的独占文件描述符。
    let writer = unsafe { std::fs::File::from_raw_fd(duplicate) };
    // SAFETY: writer持有有效描述符，F_GETFL仅读取其状态。
    let flags = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
    // SAFETY: F_SETFL只增加非阻塞标志；reader共享同一打开文件状态。
    if flags < 0
        || unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(TerminalError::caused(
            "could not configure nonblocking PTY I/O",
            io::Error::last_os_error(),
        ));
    }
    let reader = writer
        .try_clone()
        .map_err(|error| TerminalError::caused("could not clone PTY reader", error))?;
    Ok((Box::new(reader), Box::new(writer)))
}

#[derive(Clone, Debug)]
pub(super) struct ProcessTree {
    session: libc::pid_t,
    active: Arc<AtomicBool>,
}

impl ProcessTree {
    pub(super) fn attach(child: &dyn portable_pty::Child) -> Result<Self, TerminalError> {
        let pid = child
            .process_id()
            .ok_or_else(|| TerminalError::invalid("PTY process ID is unavailable"))?;
        let session = libc::pid_t::try_from(pid)
            .map_err(|_| TerminalError::invalid("invalid PTY process ID"))?;
        if session <= 1 {
            return Err(TerminalError::invalid(
                "PTY child did not create an independent session",
            ));
        }
        // SAFETY: getsid 不解引用用户内存；PID 来自尚未 wait 的 PTY Child。
        let observed_session = unsafe { libc::getsid(session) };
        if observed_session != session {
            let error = io::Error::last_os_error();
            // native PTY 在 exec 前创建 SID。macOS Zombie 的 getsid 返回 ESRCH。
            // Child 尚未 wait，只有本次出生的 PID 能占据该位置。
            let exited_child = observed_session == -1
                && error.raw_os_error() == Some(libc::ESRCH)
                && is_unreaped_child(pid);
            if !exited_child {
                return Err(TerminalError::invalid(
                    "PTY child did not create an independent session",
                ));
            }
        }
        Ok(Self {
            session,
            active: Arc::new(AtomicBool::new(true)),
        })
    }

    pub(super) fn terminate(&self) -> Result<(), TerminalError> {
        let result = terminate_sync(self.session);
        if result.is_ok() {
            self.active.store(false, Ordering::Release);
        }
        result
    }
}

fn is_unreaped_child(pid: u32) -> bool {
    // SAFETY: waitid 接收初始化的输出结构，WNOWAIT 保留 Child 的回收权。
    let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut status,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // SAFETY: waitid 成功后 si_pid 是退出子进程的整数 PID。
    result == 0 && unsafe { status.si_pid() } == pid as libc::pid_t
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if Arc::strong_count(&self.active) != 1 || !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        // SAFETY: 负的受信 SID 只指向本终端初始进程组；Drop 不等待。
        unsafe {
            libc::kill(-self.session, libc::SIGKILL);
        }
    }
}

/// 父监督器回收已登记 PTY SID；不接受浏览器提交的 PID。
///
/// # Errors
/// 拒绝本进程或非法 SID，并报告内核拒绝的回收信号。
pub async fn terminate_session(process_id: u32) -> Result<(), TerminalError> {
    let session = libc::pid_t::try_from(process_id)
        .map_err(|_| TerminalError::invalid("invalid PTY process ID"))?;
    if session <= 1 {
        return Err(TerminalError::invalid("invalid PTY process ID"));
    }
    tokio::task::spawn_blocking(move || terminate_sync(session))
        .await
        .map_err(|error| TerminalError::caused("terminal session cleanup task failed", error))?
}

fn terminate_sync(session: libc::pid_t) -> Result<(), TerminalError> {
    // SAFETY: getsid(0) 查询调用方自身会话，防止错误调用清除父会话。
    if session == unsafe { libc::getsid(0) } {
        return Err(TerminalError::invalid(
            "refusing to terminate the caller session",
        ));
    }
    for _ in 0..20 {
        let mut system = sysinfo::System::new();
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let mut found = false;
        for process in system.processes().values() {
            let Ok(pid) = libc::pid_t::try_from(process.pid().as_u32()) else {
                continue;
            };
            // SAFETY: getsid/kill 使用经过范围检查的 PID，不读取指针。
            if pid > 1
                && process.status() != sysinfo::ProcessStatus::Zombie
                && unsafe { libc::getsid(pid) } == session
            {
                found = true;
                if unsafe { libc::kill(pid, libc::SIGKILL) } != 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(TerminalError::caused(
                            "could not terminate terminal descendant",
                            error,
                        ));
                    }
                }
            }
        }
        if !found {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    Err(TerminalError::invalid("terminal descendants did not exit"))
}
