//! [INPUT]: 依赖 HTTP、TLS、服务身份和出口代理配置。
//! [OUTPUT]: 提供认证通信、密封凭据、HTTP 错误和有界指标。
//! [POS]: 服务共享基础设施，不依赖规划或持久化契约。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod crypto;
pub mod error;
pub mod metrics;
pub mod proxy;
pub mod service;
