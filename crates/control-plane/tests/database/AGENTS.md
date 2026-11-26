# crates/control-plane/tests/database/
> L2 | Parent: ../AGENTS.md

fixture.rs: 初始化新数据库，提供版本元数据、创建时固定要求和目标的构建、固定源码夹具。
dashboard.rs: 验证概览租户与项目授权、撤权后的统计及非法参数拒绝。
build_activity.rs: 验证构建过程的权限、源码历史与秘密字段隔离。
release_contract.rs: 验证版本元数据、平台授权查询和原生构建设备准入。
planning_basics.rs: 验证模型兼容配置持久化、密钥脱敏、注册和规划状态边界。
account_settings.rs: 验证个人设置和项目资源权限。
ota_versions.rs: 验证手动固定目标、池并发、历史标签与重试围栏。
ota_manual.rs: 验证手动授权、池并发、续签冻结资产、进度序号和权限围栏。
node_installation.rs: 验证全局设备地址、同版本安装、签名与跨租户拒绝。
build_logs.rs: 验证历史日志分页、过滤、授权和终态流结束。
device_load.rs: 验证三个控制平面的设备群连接、续租和调度预算。
node_metadata.rs: 验证启动、心跳和重连保留名称及节点查询范围。
node_identity.rs: 验证注册改名、识别摘要、任务租约与列表预算。
node_pools.rs: 验证池写入权限、删除保护、注册锁序、审计及授权计数和设备筛选。
node_placement.rs: 验证候选准入、资源时钟和设备槽位。
node_tools.rs: 验证内嵌 Coordinator 前缀、服务鉴权及真实 MCP SDK 节点工具。
readable_routes.rs: 以显式授权设备创建构建，验证 slug 路由和对象权限。
registration.rs: 验证账号生命周期、项目权限和网关对象票据。
replication.rs: 验证副本锁和租约回收。
planning_bridge.rs: 验证无条件规划 outbox 与取消版本。
signing.rs: 验证真实 PKCS12、固定版本、撤权、可用性与摘要证据。
source_refs.rs: 验证受权引用任务与错误脱敏。
tenancy.rs: 提供租户HTTP与设备夹具，组合节点池、成员和构建权限验收。
node_operations.rs: 验证有限操作锁后权限、旧会话拒绝、先取消与结果事务确认。
terminals.rs: 验证设备终端原子限额、原账户 Session、一次性票据和设备围栏。
terminal_lifecycle.rs: 验证租户终端排空、设备租约窗口、删除清扫和项目隔离。
terminal_routing.rs: 以真实 TLS 验证三副本、丢 ACK 与内部断流恢复、慢快照授权截止。
terminal_recovery.rs: 为三副本测试重取附着票据，断言内部短断不产生终态。
terminal_load.rs: 量化十个虚拟设备会话的输出 credit、RTT、心跳、关闭和测试进程资源。
transport_certificates.rs: 为设备和终端传输验收签发独立 CA、服务与客户端证书。
tenancy_isolation.rs: 验证跨租户历史、制品流与源码计划引用拒绝。
tenancy_locks.rs: 验证权限撤销复验与设备授权外键锁顺序。
tenancy_regressions.rs: 验证租户隔离和管理权限回归。
transport.rs: 组合真实 Tonic/mTLS、设备负载与制品传输回归。
workspace_lifecycle.rs: 以显式授权设备验证构建排空、归档、删除和恢复。

images.rs: 验证镜像准入、候选隔离、外仓分发及原生准入不受镜像开关影响。
image_pool_admission.rs: 验证查池不占行锁、缺池无 Job、403/503 无永久回执且同事件可重试。
image_regressions.rs: 验证终态材料回收、失败摘要、发布失败分类与日志隔离和发布快照。
image_completion.rs: 验证候选提交与失败诊断的 UID、spec 和 generation 围栏。

notification_channels.rs: 验证邮箱单次令牌、订阅重置、绑定代数与撤权取消。
notifications.rs: 验证聚合版本、账户水位、完整范围与读取权限。
notification_followup.rs: 验证取消项目关注后静默关闭已有待处理事项。
notification_events.rs: 验证真实业务触发器、异常恢复、事务回滚和健康查询。

[PROTOCOL]: Update this header on change, then check AGENTS.md
