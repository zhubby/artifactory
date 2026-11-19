# crates/planner-runtime/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出服务、生命周期和持久运行存储。
server.rs: 使用独立数据库构建运行时；认证 Operator 上报和源码准备。
lifecycle.rs: 固定 deadline、累计调用预算和 generation。
store.rs: 原子保存执行与提案，返回固定项目的准入身份并查询准备资格。
store/: 定义数据库模型、执行故障终态和完整完成消息审计。

[PROTOCOL]: Update this header on change, then check AGENTS.md
