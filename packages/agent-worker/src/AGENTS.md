# packages/agent-worker/src/
> L2 | Parent: ../AGENTS.md

main.ts: Poll Worker 注册、任务租约与子进程隔离。
task.ts: 子进程执行规划并通过 IPC 上报结果。
runtime.ts: Pi 规划循环、原生与镜像目标契约。
runtime.test.ts: 验证规划协议、凭据边界与截止时间。
job-main.ts: Kubernetes Runtime 任务入口与终态上报。
job-main.test.ts: 验证 Runtime 失败、取消与恢复。
node-mcp.ts: 将授权节点查询接入 MCP 工具。
node-mcp.test.ts: 验证节点查询的工具契约。
task.test.ts: 验证任务子进程隔离与取消。
transport/: Runtime HTTP 租约与任务传输。

[PROTOCOL]: Update this header on change, then check AGENTS.md
