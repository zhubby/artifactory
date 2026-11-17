//! [INPUT]: 依赖镜像资源、协调循环与隔离工作进程。
//! [OUTPUT]: 导出 Image Operator 与镜像工作模块。
//! [POS]: 独立镜像服务的 crate 入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod api;
pub mod controller;
pub mod leadership;
pub mod publisher;
pub mod resources;
pub mod transport;
pub mod worker;

#[cfg(test)]
mod resource_tests;
