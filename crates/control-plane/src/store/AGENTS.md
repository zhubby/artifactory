# crates/control-plane/src/store/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合持久化接口、租户作用域和迁移。
auth.rs: 查询登录账户并维护认证状态。
dto.rs: 定义公开业务响应、安全规划摘要、节点识别与验签证据投影。
errors.rs: 转换数据库约束为业务错误。
models.rs: 定义 Toasty 私有 CRUD 投影。
management.rs: 事务改名并审计，投影可见识别摘要与节点搜索。
projects.rs: 写入项目配置和源码授权。
signing.rs: 写入凭据版本、默认方案并投影方案当前可用性。
signing_execution.rs: 锁定当前授权并执行材料领取及最终证据登记。
tenancy.rs: 锁定租户权限并校验资源范围。
lifecycle.rs: 维护归档、删除租约和 Release 审计资源清单。
lifecycle_cleanup.rs: 依据删除清单清除业务状态和签名密文。
builds.rs: 创建构建，读取原生及镜像 Release、制品与日志游标。
executions.rs: 以兼容取消外键的写锁维护节点 session、事件与不可变制品登记。
scheduling.rs: 分配任务并汇总构建状态。
releases.rs: 创建版本元数据、更新说明，复核原生设备授权并保存独立构建输入。
release_publication.rs: 冻结交付清单、正式文件名和 OCI 摘要白名单。
planning.rs: 读取固定规划上下文、计划和失败日志。
planning_management.rs: 写入模型方案、澄清及对话查询上下文。
placements.rs: 保存节点候选并校验分配条件。
replication.rs: 协调副本归属和持久化唤醒。
planning_bridge.rs: 转换规划意图，按 generation 和终态围栏保存累计调用与用量。
sources.rs: 以构建为归属固定源码 CR UID、pin 和清单，隔离取消与失败传播。
sources_tests.rs: 验证 CR UID、执行围栏、事务幂等和独立构建源码取消。
releases_tests.rs: 验证版本元数据、设备授权准入、源码隔离与交付封存围栏。
lookups.rs: 读取调度、构建和授权对象。
node_metadata.rs: 记录启动元数据和能力摘要。
node_queries.rs: 聚合项目授权原生平台，查询构建冻结范围内的设备。
ota.rs: 记录节点升级命令和健康回执。
ota_management.rs: 写入节点发布及升级目标。
performance.rs: 验证存储查询预算和响应规模。
execution_fences.rs: 以真实事件、心跳与取消事务验证节点锁等待及权限租约拒绝。

images.rs: 保存执行隔离候选，约束已发布版本的外仓分发。
registry_guard.rs: 领取递增发布租约，按冻结仓库、版本和摘要图授权正式 Manifest。

planning_bridge_tests.rs: 验证真实 outbox 统计投影、取消围栏、回执幂等和重排不变约束。
planning_runtime_tests.rs: 以隔离数据库验证提案前用量、权限、终态优先与查询降级。

[PROTOCOL]: Update this header on change, then check AGENTS.md
