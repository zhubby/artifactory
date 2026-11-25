# crates/planning-coordinator/tests/database/
> L2 | Parent: ../AGENTS.md

source_payload.rs: 验证大清单、材料前后存储故障恢复及取消与 generation 拒绝。
conversation_chunks.rs: 验证大提案分片分页、Worker 分片保留与事务回滚。
execution_failure.rs: 验证故障终态、中断恢复、预算保留、身份隔离和槽位释放。
worker_failure.rs: 验证终态错误保留、秘密脱敏、UTF-8 限长和提案围栏。

source_preparation.rs: 验证清单绑定、本代就绪与模型租约边界。

[PROTOCOL]: Update this header on change, then check AGENTS.md
