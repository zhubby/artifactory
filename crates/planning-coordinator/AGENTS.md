# crates/planning-coordinator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明内嵌规划 Coordinator 与数据库依赖。
src/: 提供规划授权、模型调用、执行故障上报与持久队列。
tests/: 验证持久状态、预算、并发准入与生命周期。
migrations/: 保存 Coordinator 新数据库初始化基线。

[PROTOCOL]: Update this header on change, then check AGENTS.md
