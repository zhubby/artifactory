//! ```text
//! [INPUT]: 依赖受信设备 Shell 配置、原生 PTY 和调用方运行时。
//! [OUTPUT]: 提供终端字节、窗口、父注册握手、退出状态及进程回收。
//! [POS]: 可选设备终端原语；不读取租户、设备凭据或网络协议。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, path::PathBuf};

mod runtime;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub use runtime::run;
#[cfg(unix)]
pub use unix::terminate_session;

/// 仅由设备配置产生的 Shell；环境是显式白名单，不继承服务环境。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalConfig {
    pub shell: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub columns: u16,
    pub rows: u16,
    pub term: String,
    pub environment: BTreeMap<String, String>,
    /// 父进程须在 Ready 后持久登记身份，再确认；确认前保留子进程身份。
    #[serde(default)]
    pub require_registration: bool,
}

impl fmt::Debug for TerminalConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalConfig")
            .field("shell", &self.shell)
            .field("cwd", &self.cwd)
            .field("columns", &self.columns)
            .field("rows", &self.rows)
            .field("argument_count", &self.args.len())
            .field("environment_keys", &self.environment.keys())
            .field("require_registration", &self.require_registration)
            .finish()
    }
}

/// 当前附着连接的输入；关闭输入通道也会回收终端。
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalInput {
    Registered,
    Input { data: Vec<u8> },
    Resize { columns: u16, rows: u16 },
    Close,
}

impl fmt::Debug for TerminalInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registered => f.write_str("Registered"),
            Self::Input { data } => f.debug_struct("Input").field("bytes", &data.len()).finish(),
            Self::Resize { columns, rows } => f
                .debug_struct("Resize")
                .field("columns", columns)
                .field("rows", rows)
                .finish(),
            Self::Close => f.write_str("Close"),
        }
    }
}

/// 设备本地进程事实；输出保持原始字节，退出码不折叠。
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TerminalEvent {
    Ready {
        process_id: u32,
    },
    Output {
        data: Vec<u8>,
    },
    Exited {
        code: Option<i32>,
        signal: Option<String>,
    },
}

impl fmt::Debug for TerminalEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready { process_id } => f
                .debug_struct("Ready")
                .field("process_id", process_id)
                .finish(),
            Self::Output { data } => f
                .debug_struct("Output")
                .field("bytes", &data.len())
                .finish(),
            Self::Exited { code, signal } => f
                .debug_struct("Exited")
                .field("code", code)
                .field("signal", signal)
                .finish(),
        }
    }
}

/// 可定位操作的终端错误，不把启动参数或环境值写入错误消息。
#[derive(Debug, thiserror::Error)]
#[error("{operation}")]
pub struct TerminalError {
    operation: &'static str,
    #[source]
    source: Option<anyhow::Error>,
}

impl TerminalError {
    fn invalid(operation: &'static str) -> Self {
        Self {
            operation,
            source: None,
        }
    }

    fn caused(operation: &'static str, source: impl Into<anyhow::Error>) -> Self {
        Self {
            operation,
            source: Some(source.into()),
        }
    }
}

/// 单帧输入与输出上限，队列项数不能放大单帧分配。
pub const MAX_FRAME_BYTES: usize = 16 * 1024;

fn validate_size(columns: u16, rows: u16) -> Result<(), TerminalError> {
    if !(1..=500).contains(&columns) || !(1..=500).contains(&rows) {
        return Err(TerminalError::invalid("invalid terminal dimensions"));
    }
    Ok(())
}
