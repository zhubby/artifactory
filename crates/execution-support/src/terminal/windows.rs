//! [INPUT]: 依赖 ConPTY 子进程句柄和 Windows Job Object。
//! [OUTPUT]: 将终端进程树限制在关闭即终止的作业内。
//! [POS]: Windows 终端回收边界；外层 Worker 仍须受父 Job 监督。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::TerminalError;
use std::{
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

#[derive(Clone, Debug)]
pub(super) struct ProcessTree {
    job: Arc<OwnedHandle>,
}

impl ProcessTree {
    pub(super) fn attach(child: &dyn portable_pty::Child) -> Result<Self, TerminalError> {
        let process = child
            .as_raw_handle()
            .ok_or_else(|| TerminalError::invalid("ConPTY process handle is unavailable"))?;
        // SAFETY: 空的安全属性和名称创建一个不继承的匿名 Job。
        let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if handle.is_null() {
            return Err(TerminalError::caused(
                "could not create terminal job",
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: CreateJobObjectW 返回的有效句柄转移到唯一的 RAII 所有者。
        let job = unsafe { OwnedHandle::from_raw_handle(handle) };
        // SAFETY: 结构体为 Win32 POD，所有零值字段表示无额外约束。
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: 句柄有效，结构地址与长度匹配对应的信息类。
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 {
            return Err(TerminalError::caused(
                "could not configure terminal job",
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: 子进程句柄在调用期间由 portable-pty 所有，Job 句柄由本函数所有。
        if unsafe { AssignProcessToJobObject(handle, process) } == 0 {
            return Err(TerminalError::caused(
                "could not assign terminal process to job",
                io::Error::last_os_error(),
            ));
        }
        Ok(Self { job: Arc::new(job) })
    }

    pub(super) fn terminate(&self) -> Result<(), TerminalError> {
        // SAFETY: Arc 保持有效 Job 句柄；本作业只包含此终端进程树。
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            return Err(TerminalError::caused(
                "could not terminate terminal job",
                io::Error::last_os_error(),
            ));
        }
        Ok(())
    }
}
