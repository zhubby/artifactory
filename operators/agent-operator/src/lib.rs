//! [INPUT]: 依赖资源契约、服务客户端和协调循环。
//! [OUTPUT]: 导出 Agent Operator 模块。
//! [POS]: 独立 Agent Operator crate 入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod api;
pub mod client;
pub mod controller;
pub mod leadership;
