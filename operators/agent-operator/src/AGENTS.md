# agent-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义 Agent 和 Source 的 Kubernetes 契约。
client.rs: 通过认证服务接口查询 Runtime 并登记执行。
controller.rs: 协调任务 Job、Source 分片与快照回收。
controller_tests.rs: 验证资源归属、回收和故障恢复。
leadership.rs: 通过 Lease 互斥写入并校验租约。
lib.rs: 导出 Agent Operator 模块。
main.rs: 启动独立控制器和选主循环。
bin/: 输出离线 Agent CRD。

[PROTOCOL]: Update this header on change, then check AGENTS.md
