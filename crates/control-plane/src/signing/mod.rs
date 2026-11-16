//! [INPUT]: 依赖共享签名契约、凭据加密与原生密钥解析。
//! [OUTPUT]: 提供签名密钥环、导入校验与执行计划约束。
//! [POS]: HTTP 管理和节点领取共用的签名业务边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
mod encryption;
mod import;
pub mod policy;
pub use artifactory_emissary_protocol::signing::*;
pub use encryption::KeyRing;
pub use import::inspect;
