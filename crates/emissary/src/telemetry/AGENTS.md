# crates/emissary/src/telemetry/
> L2 | Parent: ../AGENTS.md

mod.rs: 固定 hostname 与 Product 启动快照，缓存动态诊断并驱动 mTLS 发送。
collector.rs: 生成严格批次；每轮重建计数集合，Windows 地址独立枚举并清除失效地址。
inventory.rs: 探测工具、容器内存额度与加速器能力。
managed.rs: 按任务登记进程根，校验 PID 身份并归属后代。
network.rs: 从本地接口生成有界地址诊断，过滤回环地址并保留私网与 VPN。
windows_network.rs: 以完整 IP Helper 单播枚举提供 Windows 地址，不依赖严格计数接口筛选。
windows_network_tests.rs: 以 Windows ABI 缓冲区验证 VPN、地址生命周期、重试和安全遍历。
spool.rs: 独占打开磁盘队列，原字节重放旧批次，按额度丢弃并确认删除。

[PROTOCOL]: Update this header on change, then check AGENTS.md
