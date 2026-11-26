# support/
> L2 | Parent: ../AGENTS.md

mod.rs: 共享控制与终端流、HTTP Git和首连诊断，连接发布前等待Hello事务回调。
tls.rs: 为Control与Telemetry生成同源mTLS材料，显式进程测试私有落盘服务证书。
live_protocol.rs: 传递会话租户，创建固定清单任务，使用行锁验证事件授权。
legacy_receiver_fixture.rs: 在冻结旧测试包启动旧路由或旧main，回收随机数据库并发布私有就绪文件。
legacy_main_fixture.rs: 应用真实旧业务迁移，创建只读授权角色，监督完整旧服务进程。
legacy_sender_fixture.rs: 复制到冻结旧包执行原Handle，不接触数据库，发布每次进程源文件标记。
current_main_legacy.rs: 两个冻结旧进程直连完整当前main，核对零丢弃、事务确认和旧清单来源。
frozen_sender.rs: 共用旧进程的顺序请求、完成标记和父驱动退出回执。
durable_receiver.rs: 创建具名隔离库，提供真实业务种子、Hello事务、精确清理和原正文核对。
current_main_databases.rs: 按私有清单创建双库和受限角色，执行部署授权并按标记定点清理。
current_main_runner.rs: 将两次真实Runner接入完整Telemetry进程，核对Hello提交与队列事务确认。
current_main_fairness.rs: 在完整main、双库与受限角色下，验证历史积压中的真实ACK和当前startup新鲜度。
runner_task.rs: 共用真实Runner取消、队列锁释放与超时回收边界。
telemetry_gate.rs: 暂拒上传并透传真实协商，保留mTLS身份，记录最后主机样本及请求和真实重复ACK。
telemetry_sources.rs: 准备或读取真实源集合，固定Fresh身份并核对ACK前后原字节。
filesystem_sources.rs: 冻结故障卷原队列与身份，验证满盘恢复、真实事务ACK和设备端源删除。
filesystem_supervisor.rs: 验证真实小卷满盘下载保持当前进程，分别读回状态探测与终态保存时机。

[PROTOCOL]: Update this header on change, then check AGENTS.md
