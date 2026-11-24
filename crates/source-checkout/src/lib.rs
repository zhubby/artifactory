// [INPUT]: 依赖临时凭据、固定清单与本机 Git/LFS。
// [OUTPUT]: 提供直接检出、引用查询、安全诊断与阶段合同。
// [POS]: 控制平面、init 与原生设备共享源码边界。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
mod checkout;
mod diagnostics;
mod git;
mod manifest;
mod progress;
mod refs;

pub use checkout::{Checkout, InspectedRepository, Submodule, TagSelection, verify};
pub use diagnostics::{GitFailure, GitFailureKind};
pub use git::{Options, askpass, build_environment, private_dir};
pub use manifest::{
    GitCredential, LfsObject, PrepareResponse, SourceCredential, SourceManifest, SourceRepository,
    managed_path, resolve_submodule, same_origin, valid_commit, validate_repository,
};
pub use progress::{SourceDiagnostic, SourceProgress, SourceProgressState, SourceStage};
pub use refs::{GitRef, RepositoryRefs};
