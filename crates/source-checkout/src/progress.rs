//! [INPUT]: 依赖源码解析动作和安全 Git 失败分类。
//! [OUTPUT]: 提供有界阶段记录与失败诊断的数据合同。
//! [POS]: Source Preparer 与 Control 共用的元数据边界。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use crate::GitFailureKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceStage {
    Authorization,
    ResolveRef,
    PinRef,
    Inspect,
    Manifest,
    Fetch,
    Submodules,
    Lfs,
    Verify,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProgressState {
    Started,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceProgress {
    pub sequence: u32,
    pub stage: SourceStage,
    pub state: SourceProgressState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDiagnostic {
    pub stage: SourceStage,
    pub code: GitFailureKind,
}
