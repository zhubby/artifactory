# packages/planning-worker/src/
> L2 | Parent: ../AGENTS.md

planning.ts: 固定构建完整目标与版本来源，逐轮提示剩余预算并持久上报完成消息。
planning.test.ts: 验证任意模型名、预算收敛与校验修正、应用分类、凭据脱敏和期限。
model-configuration.ts: 按协议与显式配置构造请求字段，隔离名称和端点推断。
pod-main.ts: 获取 Kubernetes Planning coordinator 租约；用退出码区分未确认中断和终态。
pod-main.test.ts: 验证终态确认、提案恢复、审计幂等和响应字节上限。
source-tools.ts: 从只读 checkout 按行读取、限定目录搜索并返回有界续页。
source-tools.test.ts: 验证行范围、零命中与单文件搜索续页，拒绝不安全路径和文件。
node-mcp.ts: 通过绑定租约暴露只读 MCP 节点工具。
node-mcp.test.ts: 验证 MCP SDK 与 Planning coordinator 传输授权。
transport/: 封装 Planning coordinator HTTP 租约与持久提案。

[PROTOCOL]: Update this header on change, then check AGENTS.md
