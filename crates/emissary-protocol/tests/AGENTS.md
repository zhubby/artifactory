# crates/emissary-protocol/tests/
> L2 | Parent: ../AGENTS.md

operations.rs: 验证固定操作输入、摘要归一化与部分失败结果预算。
nodes.rs: 验证名称 UTF-8 边界、快照与资源准入。
commands.rs: 验证目录策略、显式通知、结构/schema拒收与能力握手。
inventory_delivery.rs: 验证清单原字节摘要、身份、大小、回执和旧批次迁移。
telemetry_negotiation.rs: 验证格式交集、输入预算、摘要回执与有限拒收详情。
terminal.rs: 验证终端启动身份、窗口、profile 和字节预算。
wire_compatibility.rs: 固定旧JSON和字段号，验证schema、摘要ACK和通知字段12。
fixtures/: 保存旧契约字节与完整心跳的合法容量输入。

[PROTOCOL]: Update this header on change, then check AGENTS.md
