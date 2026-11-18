//! [INPUT]: 依赖运行生命周期与服务模块。
//! [OUTPUT]: 导出 Runtime 接口。
//! [POS]: 独立规划 crate 的模块入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod grpc;
pub mod lifecycle;
pub mod server;

pub mod store;
