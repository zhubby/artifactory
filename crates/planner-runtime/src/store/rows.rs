//! [INPUT]: 依赖 Runtime 初始化基线的物理列顺序。
//! [OUTPUT]: 提供一致的运行行投影和事务行锁。
//! [POS]: 隔离 SQL 记录解码顺序约束。
//! [PROTOCOL]: Update this header on change, then check AGENTS.md
use super::*;
pub(super) fn run_fields() -> Vec<(&'static str, Type)> {
    vec![
        ("tenant_id", Type::Uuid),
        ("id", Type::Uuid),
        ("version", Type::I64),
        ("intent_hash", Type::String),
        ("intent_ciphertext", Type::String),
        ("project_id", Type::Uuid),
        ("pool", Type::String),
        ("source_manifest_id", Type::Uuid),
        ("source_digest", Type::String),
        ("lifecycle", Type::String),
        ("status", Type::String),
        ("execution", Type::String),
        ("grant_hash", Type::String),
        ("grant_ciphertext", Type::String),
        ("usage", Type::String),
        ("report_sequence", Type::I64),
        ("created_at", Type::Timestamp),
        ("dispatched_at", Type::Timestamp),
        ("updated_at", Type::Timestamp),
    ]
}

pub(super) async fn locked(tx: &mut toasty::db::Transaction<'_>, run: Uuid) -> Result<sql::Record> {
    Ok(sql::record(
        tx,
        "SELECT * FROM runtime_runs WHERE id=$1 FOR UPDATE",
        params![run],
        &run_fields(),
    )
    .await?)
}
