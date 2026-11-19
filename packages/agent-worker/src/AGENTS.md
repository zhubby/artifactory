# packages/agent-worker/src/
> L2 | Parent: ../AGENTS.md

runtime.ts: 要求显式应用分类并执行 Pi 规划；持久上报完成消息。
runtime.test.ts: 验证应用分类、工具、凭据脱敏和期限。
pod-main.ts: 获取 Kubernetes Runtime 租约并提交终态。
pod-main.test.ts: 验证提案恢复、审计幂等和大源码响应字节上限。
source-tools.ts: 从任务只读 checkout 查询文件、文本和固定 Git 元数据。
source-tools.test.ts: 拒绝路径穿越、符号链接、二进制和超大文件。
node-mcp.ts: 通过绑定租约暴露只读 MCP 节点工具。
node-mcp.test.ts: 验证 MCP SDK 与 Runtime 传输授权。
transport/: 封装 Runtime HTTP 租约与持久提案。

[PROTOCOL]: Update this header on change, then check AGENTS.md
