# crates/emissary/tests/native_service/
> L2 | Parent: ../AGENTS.md

command.rs: 隔离父进程数据库凭据，限制命令与双流输出；有界读回Job终止后的异步退出。
cleanup.rs: 共用只读清理门禁，账户残留、目录残留或查询失败均阻断成功。
durable.rs: 固定mTLS端点，在维护或满盘恢复后由原生服务确认原源和新样本的真实事务ACK。
disk.rs: 在父驱动私有ext4卷填满空间，核对原生服务、会话与原队列保持及恢复采集。
maintenance.rs: 共用签名和冻结队列，验证维护与runtime OTA，为磁盘阶段提供文件及权限读回。
installer.rs: 固定仓库安装器副本，按真实平台接口传递签名候选、摘要和既有服务路径。
post_maintenance.rs: 从候选实际版本构造签名请求，经mTLS验证维护后runtime OTA和健康确认。
queues.rs: 为维护和Runner恢复生成真实未确认队列，校验原字节并输出分类摘要。
readiness.rs: 通过真实过期拒收确认操作日志已重开，再采样权限基线。

[PROTOCOL]: Update this header on change, then check AGENTS.md
