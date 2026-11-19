# crates/control-plane/tests/database/
> L2 | Parent: ../AGENTS.md

fixture.rs: 初始化新数据库并提供租户和固定源码清单。
planning_basics.rs: 验证模型配置、注册和规划状态边界。
account_settings.rs: 验证个人设置和项目资源权限。
build_logs.rs: 验证历史日志分页、过滤、授权和终态流结束。
device_load.rs: 验证三个控制平面的设备群连接、续租和调度预算。
node_metadata.rs: 验证节点元数据和 Runtime 查询范围，提供候选夹具。
node_placement.rs: 验证候选准入、资源时钟和设备槽位。
node_tools.rs: 验证内嵌 Runtime 前缀、服务鉴权及真实 MCP SDK 节点工具。
readable_routes.rs: 验证 slug 路由与对象权限。
registration.rs: 验证账号生命周期、项目权限和网关对象票据。
replication.rs: 验证副本锁和租约回收。
runtime.rs: 验证无条件 Runtime outbox 与取消版本。
signing.rs: 验证真实 PKCS12、固定版本、撤权、可用性与摘要证据。
source_refs.rs: 验证受权引用任务与错误脱敏。
tenancy.rs: 提供租户 HTTP 夹具及子模块入口。
tenancy_regressions.rs: 验证租户隔离和管理权限回归。
transport.rs: 组合真实 Tonic/mTLS、设备负载与制品传输回归。
workspace_lifecycle.rs: 验证工作区归档、删除和故障恢复。

images.rs: 验证镜像准入、轮转调度、固定发布与凭据撤销。
image_regressions.rs: 验证终态材料回收、失败摘要、发布日志隔离和发布快照。
image_completion.rs: 验证候选提交与失败诊断的 UID、spec 和 generation 围栏。

[PROTOCOL]: Update this header on change, then check AGENTS.md
