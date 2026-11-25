# crates/planning-coordinator/tests/
> L2 | Parent: ../AGENTS.md

lifecycle.rs: 验证 generation、预算和过期授权。
database.rs: 在真实 PostgreSQL 验证并发预算、审计与准备资格。
database/: 验证大源码、审计分片、脱敏错误和执行终态的持久链路。

baseline.rs: 验证基线与追加迁移身份；误连旧库时保留数据并拒绝启动。

[PROTOCOL]: Update this header on change, then check AGENTS.md
