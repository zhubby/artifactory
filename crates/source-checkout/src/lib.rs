// [INPUT]: 依赖临时凭据、固定清单与本机 Git/LFS。
// [OUTPUT]: 提供直接上游检出、引用查询和完整性验证。
// [POS]: 控制平面、init 与原生设备共享源码边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
mod checkout;
mod git;
mod manifest;
mod refs;

pub use checkout::{Checkout, InspectedRepository, Submodule, TagSelection, verify};
pub use git::{Options, askpass, build_environment, private_dir};
pub use manifest::{
    GitCredential, LfsObject, PrepareResponse, SourceCredential, SourceManifest, SourceRepository,
    managed_path, resolve_submodule, same_origin, valid_commit, validate_repository,
};
pub use refs::{GitRef, RepositoryRefs};
