# crates/emissary/src/telemetry/
> L2 | Parent: ../AGENTS.md

mod.rs: 固定启动快照，隔离清单故障，缓存观测并驱动mTLS发送。
durability.rs: 记录目录同步债务；命名空间修改成功后保持内存索引一致。
collector.rs: 按UTF8字节预算生成新采样；每轮重建接口集合并清除失效地址。
inventory.rs: 探测工具、容器内存额度与可取消的平台加速器能力。
inventory_upload.rs: 持久隔离永久冲突，继续发送有效观察；本地故障和真实ACK分别报告。
inventory_upload_tests.rs: 真实mTLS回归本地故障、冲突后有效ACK与未知计数。
inventory_conflict_tests.rs: 验证冲突源保留、索引重开、写盘失败与活动标记有界保留。
inventory_delivery.rs: 保留当前启动与最新刷新，持久隔离冲突身份，按观察摘要确认。
inventory_lane.rs: 隔离清单存储故障，透传永久冲突索引与未知计数，保护未提取来源。
managed.rs: 按任务登记进程根，校验 PID 身份并归属后代。
network.rs: 从本地接口生成有界地址诊断，过滤回环地址并保留私网与 VPN。
windows_network.rs: 以完整 IP Helper 单播枚举提供 Windows 地址，不依赖严格计数接口筛选。
windows_network_tests.rs: 以 Windows ABI 缓冲区验证 VPN、地址生命周期、重试和安全遍历。
scheduler.rs: 在1MiB内分离主机槽并报告新增计数，等量合并不触发历史淘汰。
sample_bounds.rs: 仅裁剪新主机批次的UTF8字段与设备数组，按实际JSON长度保证512KiB。
storage_observation.rs: 后台有界扫描正文、临时文件、索引与分配块，失败保留未知。
spool.rs: 独占队列，保护旧清单来源，原子提交缺口并缓存磁盘占用。
spool_removal.rs: 将缺口计数与待删除身份同事务持久化，按原摘要恢复删除。
spool_removal_tests.rs: 验证拒收和淘汰跨重启、写盘及目录同步故障只计一次。
spool_capacity_tests.rs: 以8192条真实记录验证主机等量合并不淘汰历史，新增槽只释放一个名额。
upload.rs: 临时协商失败沿用安全格式，保持摘要确认强度和分队列进展。

[PROTOCOL]: Update this header on change, then check AGENTS.md
