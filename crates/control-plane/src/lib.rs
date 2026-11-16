//! [INPUT]: 依赖业务、源码、遥测与设备协议模块。
//! [OUTPUT]: 导出控制平面服务与持久化接口。
//! [POS]: 各服务二进制和集成测试的公共模块边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod api;
pub mod auth;
pub mod coordination;
pub mod crypto;
pub mod dispatch;
pub mod error;
pub mod execution_plan;
pub mod grpc;
pub mod lifecycle;
pub mod local_source;
pub mod ota;
pub mod pki;
pub mod planning;
pub mod recipe;
pub mod scheduler;
pub mod settings;
pub mod signing;
pub mod source;
pub mod source_cache;
pub mod state;
pub mod storage;

pub mod protocol {
    pub use artifactory_emissary_protocol::*;
}
pub mod runtime_bridge;

pub mod store;

mod build;
mod node_query;
