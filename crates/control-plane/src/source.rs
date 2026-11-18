//! [INPUT]: 依赖共享源码校验和凭据引用。
//! [OUTPUT]: 导出仓库校验及项目源码凭据绑定。
//! [POS]: 控制平面源码配置边界，不读取仓库文件。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub use artifactory_source_checkout::GitCredential;
pub use artifactory_source_checkout::validate_repository;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialBinding {
    pub repository_url: String,
    pub credential_id: Uuid,
}
