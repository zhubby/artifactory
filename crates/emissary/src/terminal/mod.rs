//! [INPUT]: 依赖固定设备配置、独立 mTLS 流与共享 PTY 原语。
//! [OUTPUT]: 提供默认关闭的终端设置、会话监督和隐藏 Worker 入口。
//! [POS]: 设备终端边界；不读取构建材料，不开放入站端口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod settings;
mod supervisor;
mod transport;
mod worker;

pub(crate) use settings::capabilities;
pub use settings::{ShellProfile, TerminalSettings};
pub(crate) use supervisor::Supervisor;
pub(crate) use transport::sweep;
pub use worker::stdin_worker;
