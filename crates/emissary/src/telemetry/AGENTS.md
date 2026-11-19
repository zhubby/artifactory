# crates/emissary/src/telemetry/
> L2 | Parent: ../AGENTS.md

mod.rs: 保存启动快照，驱动采样、磁盘缓冲与 mTLS 发送。
collector.rs: 将系统资源和受管进程转换为节点协议批次。
inventory.rs: 探测工具、容器内存额度与加速器能力。
managed.rs: 按任务登记进程根，校验 PID 身份并归属后代。
spool.rs: 独占打开磁盘队列，按额度丢弃旧批次并确认删除。

[PROTOCOL]: Update this header on change, then check AGENTS.md
