//! [INPUT]: 依赖本机进程、取消令牌与本任务秘密。
//! [OUTPUT]: 提供通用进程、事件管道、私有写入和日志脱敏。
//! [POS]: 原生节点与镜像 Worker 共用的执行边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod events;
pub mod fs;
pub mod process;
pub mod redact;
