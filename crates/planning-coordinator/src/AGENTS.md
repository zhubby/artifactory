# crates/planning-coordinator/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出服务、生命周期和持久运行存储。
server.rs: 使用独立数据库构建运行时；认证 Operator 上报和源码准备。
lifecycle.rs: 固定 deadline、累计调用预算和 generation。
store.rs: 原子保存执行与提案，查询运行用量并输出权威累计统计。
store/: 定义数据库模型、脱敏失败终态和完整完成消息审计。

[PROTOCOL]: Update this header on change, then check AGENTS.md
