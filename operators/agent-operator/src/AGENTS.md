# agent-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义 AgentPool、AgentRun 和 SourceTask 契约。
client.rs: 通过 8 MiB 认证 gRPC 封套查询 Runtime 并登记执行。
controller.rs: 协调规划 Job、运行身份与终态资源回收。
controller_tests.rs: 验证资源归属、临时源码、回收和故障恢复。
source_tasks.rs: 协调临时源码 Job、配额与终态清理。
leadership.rs: 通过 Lease 互斥写入并校验租约。
lib.rs: 导出 Agent Operator 模块。
main.rs: 启动独立控制器和选主循环。
bin/: 输出离线 Agent CRD。

[PROTOCOL]: Update this header on change, then check AGENTS.md
