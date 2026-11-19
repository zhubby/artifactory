//! [INPUT]: 依赖直接 Pod 契约、停机保护和协调循环。
//! [OUTPUT]: 导出 Planning Operator 模块。
//! [POS]: 独立 Planning Operator crate 入口。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
pub mod api;
pub mod controller;
pub mod leadership;
pub mod pods;
pub mod resources;
pub mod source_tasks;
