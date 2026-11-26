# crates/emissary-protocol/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 组合节点 JSON 契约并保留生成 RPC 的根导出。
commands.rs: 固定目录与执行器策略，解码显式空执行通知并分类拒收。
inventory.rs: 校验独立清单观察，固定长度前缀摘要和旧批次迁移身份。
operations.rs: 固定只读操作目录、身份截止、类型结果与摘要确认。
ota.rs: 定义有界真实升级进度、启动器能力和排空计数，不授予执行权限。
nodes.rs: 定义清单冲突诊断、名称校验、心跳预算和解码、启动快照与有序选机边界。
signing.rs: 定义材料、公开身份、版本绑定和验签证据。
telemetry.rs: 定义有界采样批次和清单；禁止秘密字段进入传输。
telemetry_negotiation.rs: 协商遥测格式交集，固定摘要ACK和有限拒收原因。
terminal.rs: 分离终端结构与期限校验，定义设备能力、窗口和独立流预算。

[PROTOCOL]: Update this header on change, then check AGENTS.md
