//! ```text
//! [INPUT]: 依赖镜像资源、协调循环与隔离工作进程。
//! [OUTPUT]: 默认导出业务模块；最小构建仅提供本地生命周期协议。
//! [POS]: 独立镜像服务的 crate 入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
//! ```
#[cfg(feature = "operator")]
pub mod api;
#[cfg(feature = "operator")]
mod build_failure;
#[cfg(feature = "operator")]
pub mod controller;
#[cfg(feature = "operator")]
mod diagnostics;
#[cfg(feature = "operator")]
pub mod leadership;
#[cfg(feature = "operator")]
pub mod publisher;
#[cfg(feature = "operator")]
pub mod resources;
#[cfg(feature = "operator")]
pub mod transport;
#[cfg(feature = "operator")]
pub mod worker;

#[cfg(all(test, feature = "operator"))]
mod resource_tests;

#[cfg(unix)]
pub mod lifecycle;
