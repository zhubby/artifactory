# crates/planner-runtime/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出服务、生命周期和持久运行存储。
main.rs: 启动独立 Runtime 及重试回收循环。
grpc.rs: 以 8 MiB 封套提供受权 gRPC 意图、领取和执行登记。
server.rs: 凭据出口复核 generation，提供源码准备、租约、模型代理和审计读取。
lifecycle.rs: 固定 deadline、累计调用预算和 generation。
store.rs: 原子保存执行授权、提案和运行状态。
store/: 定义数据库模型和完整完成消息审计。

[PROTOCOL]: Update this header on change, then check AGENTS.md
