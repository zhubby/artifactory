# crates/telemetry-service/tests/
> L2 | Parent: ../AGENTS.md

database.rs: 创建策略前停用当前测试库调度器，验证微秒时间、持久重放、列存聚合、mTLS 授权和存储延迟。
history_memory.rs: 实测有界历史查询在大结果集下的内存预算。
workspace_lifecycle.rs: 创建策略前停用当前测试库调度器，验证手动维护、身份保留和重放隔离。
inventory_delivery.rs: 验证清单回执、孤立引用升级回滚、正文外键、启动分区与节点删除隔离。
ingest_metrics.rs: 以真实mTLS和隔离数据库验证接收计数、有限标签和64个并发准入。

[PROTOCOL]: Update this header on change, then check AGENTS.md
