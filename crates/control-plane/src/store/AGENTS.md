# crates/control-plane/src/store/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合持久化接口、租户作用域和迁移。
notifications/: 按当前授权读取账户收件箱，投影聚合版本并保护已读水位。
auth.rs: 查询登录账户并维护认证状态。
dto.rs: 定义源码准备协议版本、公开业务响应、安全规划摘要与验签证据投影。
dashboard.rs: 校验统计参数，按当前租户和可见项目读取完整窗口概览。
dashboard.sql: 以窄投影和统一读取时刻计算自然日、状态摘要与近期构建活动。
dashboard_tests.rs: 验证全量构建、状态分母、归档活动、空范围与夏令时边界。
errors.rs: 转换数据库约束为业务错误。
models.rs: 定义 Toasty 私有 CRUD 投影。
management.rs: 事务改名并审计，投影可见识别摘要与节点搜索。
node_installation.rs: 保存系统设备地址，按真实账户角色授权，原子签发安装凭证和审计。
projects.rs: 写入项目配置和源码授权。
signing.rs: 写入凭据版本、默认方案并投影方案当前可用性。
signing_execution.rs: 锁定当前授权并执行材料领取及最终证据登记。
tenancy.rs: 锁定租户权限并校验资源范围。
lifecycle.rs: 围栏租户准入，排空终端租约并维护归档、删除与 Release 清单。
lifecycle_cleanup.rs: 等待终端回收窗口，按删除清单清除业务状态与签名密文。
builds.rs: 查询构建与制品，锁定构建后记录取消时刻并重试失败任务。
executions.rs: 以兼容取消外键的写锁维护节点 session、事件与不可变制品登记。
scheduling.rs: 分配任务、汇总构建并对两类执行失败发起有限修订。
failures.rs: 统一最新失败诊断；镜像按构建源码和目标计划排除旧执行。
releases.rs: 创建版本元数据、更新说明，复核原生设备授权并冻结镜像默认池和构建输入。
release_publication.rs: 冻结交付清单、正式文件名和 OCI 摘要白名单。
planning.rs: 读取固定规划上下文与执行日志，按锁后时钟保存计划终态。
planning_management.rs: 写入模型方案、澄清及对话查询上下文。
placements.rs: 保存节点候选并校验分配条件。
replication.rs: 协调副本归属和持久化唤醒。
planning_bridge.rs: 围栏 generation 与终态，保存调用、用量和诊断，以锁后时钟记录完成。
sources.rs: 以构建为归属固定 CR UID 与根提交，接纳轻量解析并授权准备材料。
source_preparation.rs: 原子绑定清单，单调保存 Pod 摘要，补齐乱序和迟到历史。
source_preparation_tests.rs: 验证清单接纳、乱序重放、迟到历史与锁等待取消围栏。
sources_tests.rs: 验证 CR UID、执行围栏、事务幂等和独立构建源码取消。
source_progress_tests.rs: 验证阶段重放、执行围栏、安全诊断和独立源码准备摘要。
source_progress.rs: 在现有源码锁与执行围栏内幂等追加有界阶段记录。
build_activity.rs: 按当前租户读取有界构建过程，不读取秘密数据。
build_activity.sql: 合并真实源码与规划动作；独立投影准备摘要，避免时间线截断丢失 ready 证据。
releases_tests.rs: 验证版本元数据、设备授权准入、源码隔离与交付封存围栏。
lookups.rs: 读取调度、构建和授权对象。
node_metadata.rs: 记录启动元数据和能力摘要。
terminals.rs: 原子登记设备终端、Session 票据和实例租约，审计真实账户与字节计数。
node_queries.rs: 聚合项目授权原生平台，查询构建冻结范围内的设备。
ota.rs: 记录节点升级命令和健康回执。
ota_management.rs: 写入节点发布及升级目标。
performance.rs: 验证存储查询预算和响应规模。
execution_fences.rs: 以真实事件、心跳与取消事务验证节点锁等待及权限租约拒绝。

images.rs: 保存执行隔离候选与失败类型，约束已发布版本的外仓分发。
registry_guard.rs: 领取递增发布租约，按冻结仓库、版本和摘要图授权正式 Manifest。

planning_bridge_tests.rs: 提供隔离夹具，验证真实 outbox 统计、取消围栏和回执幂等。
image_revision_tests.rs: 验证镜像修订诊断与日志、成功执行与制品保留、失败分类和两次修订预算。
planning_runtime_tests.rs: 以隔离数据库验证提案前用量、权限、终态优先与查询降级。

[PROTOCOL]: Update this header on change, then check AGENTS.md
