//! [INPUT]: 依赖 OCI 执行契约、HTTP 和临时文件。
//! [OUTPUT]: 提供带认证和摘要校验的 Registry 客户端。
//! [POS]: Control 与 Publisher 共用的 OCI 仓库客户端库。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod auth;
mod client;
mod graph;
pub use client::Registry;
#[cfg(test)]
mod graph_tests;
