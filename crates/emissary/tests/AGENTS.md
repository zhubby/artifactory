# crates/emissary/tests/
> L2 | Parent: ../AGENTS.md

source.rs: 验证缓存隔离、Windows 系统路径、受限缓存链接、活动目录保护和孤儿会话清理。
contracts.rs: 验证制品边界、签名清单与 OTA 状态机。
release_manifest.rs: 运行发布 CLI，验证统一版本签名与错误标签无写入拒绝。
ota_download.rs: 验证离线资产重用、严格Range、票据刷新、取消和摘要准入。
ota_download_budget.rs: 用真实HTTP与受控时钟验证重试、续传及续签共享总期限，新执行另获预算。
ota_supervisor.rs: 验证保存故障保全进程、候选失败恢复、邮箱故障及重连健康确认。
ota_requests.rs: 验证请求耐久接受、交接锁和取消清理重试保持进程。
ota_filesystem_faults.rs: 用HFS+/APFS/ext4私有小卷验证真实下载、监督器保存故障；可选原队列事务确认。
ota_storage.rs: 验证空间预算、使用顺序、保护资产、旧状态和链接/未知目录闭锁。
node_operations.rs: 通过真实Runner与mTLS验证能力确认、重复探测和终态重放。
telemetry_protocol.rs: 通过真实mTLS验证混合端、损坏清单隔离和100秒积压新鲜度。
telemetry_legacy_receiver.rs: 连接冻结旧RPC与旧存储，验证原摘要、新采样、重放唯一及清单保留。
telemetry_legacy_sender.rs: 请求父驱动运行冻结旧Handle，验证真实确认并提供精确自有库清理入口。
telemetry_inventory_conflict.rs: 真实Service/store产生身份冲突，验证重开隔离与后续刷新事务ACK。
telemetry_durable_receiver.rs: 重开真实队列，验证mTLS接收事务确认、原字节保留与幂等入库。
telemetry_runner_recovery.rs: 重启真实Runner，验证Hello先登记、原字节保留与当前存储事务ACK。
telemetry_current_main.rs: 验证完整Telemetry、双库授权、Runner重启与可选积压新鲜度。
live_protocol.rs: 复用隔离租户夹具，验证会话、租约、事件与存储协议。
process_tree.rs: 验证前台与服务入口在配置缺失时退出。
native_service.rs: 隔离账户验证维护、生命周期与持久补报；Linux私有ext4场景验证原生满盘恢复。
native_service/: 共享有界命令、签名候选、冻结源队列、维护及补报断言。
runtime.rs: 验证EOF、租约、SIGTERM回收和OTA邮箱锁竞争；超时暴露阶段和子进程状态。
ota_runtime_health.rs: 验证真实Runner的会话、健康及排空保存故障保持进程与mTLS连接，恢复后再确认。
terminal_protocol.rs: 通过真实 Emissary、mTLS 和 PTY 验证输入突发、locale、租约与退出码。
support/: 共享平台证书夹具、首连超时诊断与真实租户协议夹具。

[PROTOCOL]: Update this header on change, then check AGENTS.md
