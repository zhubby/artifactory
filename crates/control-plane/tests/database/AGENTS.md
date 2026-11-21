# crates/control-plane/tests/database/
> L2 | Parent: ../AGENTS.md

fixture.rs: 初始化新数据库，提供版本元数据、创建时固定要求和目标的构建、固定源码夹具。
release_contract.rs: 验证版本元数据、平台授权查询和原生构建设备准入。
planning_basics.rs: 验证模型兼容配置持久化、密钥脱敏、注册和规划状态边界。
account_settings.rs: 验证个人设置和项目资源权限。
build_logs.rs: 验证历史日志分页、过滤、授权和终态流结束。
device_load.rs: 验证三个控制平面的设备群连接、续租和调度预算。
node_metadata.rs: 验证启动、心跳和重连保留名称及节点查询范围。
node_identity.rs: 验证注册改名、识别摘要、任务租约与列表预算。
node_placement.rs: 验证候选准入、资源时钟和设备槽位。
node_tools.rs: 验证内嵌 Coordinator 前缀、服务鉴权及真实 MCP SDK 节点工具。
readable_routes.rs: 以显式授权设备创建构建，验证 slug 路由和对象权限。
registration.rs: 验证账号生命周期、项目权限和网关对象票据。
replication.rs: 验证副本锁和租约回收。
planning_bridge.rs: 验证无条件规划 outbox 与取消版本。
signing.rs: 验证真实 PKCS12、固定版本、撤权、可用性与摘要证据。
source_refs.rs: 验证受权引用任务与错误脱敏。
tenancy.rs: 提供租户 HTTP 与设备夹具，验证成员和构建权限。
tenancy_regressions.rs: 验证租户隔离和管理权限回归。
transport.rs: 组合真实 Tonic/mTLS、设备负载与制品传输回归。
workspace_lifecycle.rs: 以显式授权设备验证构建排空、归档、删除和恢复。

images.rs: 验证镜像准入、候选隔离、外仓分发及原生准入不受镜像开关影响。
image_regressions.rs: 验证终态材料回收、失败摘要、发布日志隔离和发布快照。
image_completion.rs: 验证候选提交与失败诊断的 UID、spec 和 generation 围栏。

[PROTOCOL]: Update this header on change, then check AGENTS.md
