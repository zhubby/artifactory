# terminal/
> L2 | Parent: ../AGENTS.md

mod.rs: 导出终端配置、设备会话监督与隐藏 Worker 入口。
settings.rs: 校验默认关闭的设备终端和固定 Shell profile。
supervisor.rs: 绑定设备启动和控制会话，按 OTA 实际阶段限制准入并取消终端。
transport.rs: 先登记 PTY 身份再确认启动；转发独立 mTLS 字节并监督 Worker。
worker.rs: 通过有界 stdin/stdout 协议承载 PTY，父管道 EOF 触发回收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
