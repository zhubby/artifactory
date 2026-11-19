# crates/planner-runtime/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出服务、生命周期和持久运行存储。
main.rs: 启动独立 Runtime 及重试回收循环。
grpc.rs: 以 8 MiB 封套提供受权 gRPC 意图、领取和执行登记。
server.rs: 认证 Operator 故障和丢失上报；源码准备区分拒绝、等待与故障。
lifecycle.rs: 固定 deadline、累计调用预算和 generation。
store.rs: 原子保存执行与提案，返回固定项目的准入身份并查询准备资格。
store/: 定义数据库模型、执行故障终态和完整完成消息审计。

[PROTOCOL]: Update this header on change, then check AGENTS.md
