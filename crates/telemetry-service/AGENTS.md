# crates/telemetry-service/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明独立遥测服务和数据库验收依赖。
migrations/: 定义 TimescaleDB 遥测表和生命周期约束。
src/: 接收设备遥测，提供鉴权查询与保留策略。
tests/: 验证遥测持久化、查询内存和工作区删除。

[PROTOCOL]: Update this header on change, then check AGENTS.md
