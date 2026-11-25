# crates/planning-coordinator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明内嵌 Coordinator、数据库和源码清单校验依赖。
src/: 提供规划授权、模型调用、执行故障上报与持久队列。
tests/: 验证持久状态、预算、并发准入与生命周期。
migrations/: 保存 Coordinator 初始化基线与源码准备追加迁移。

[PROTOCOL]: Update this header on change, then check AGENTS.md
