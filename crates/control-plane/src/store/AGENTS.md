# crates/control-plane/src/store/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合持久化接口、租户作用域和迁移。
auth.rs: 查询登录账户并维护认证状态。
dto.rs: 定义公开业务响应和验签证据投影。
errors.rs: 转换数据库约束为业务错误。
models.rs: 定义 Toasty 私有 CRUD 投影。
management.rs: 写入凭据、节点、设备池和 API 令牌。
projects.rs: 写入项目配置和源码授权。
signing.rs: 写入凭据版本、默认方案并投影方案当前可用性。
signing_execution.rs: 锁定当前授权并执行材料领取及最终证据登记。
tenancy.rs: 锁定租户权限并校验资源范围。
lifecycle.rs: 维护归档、删除租约与在途日志写入宽限期。
lifecycle_cleanup.rs: 依据删除清单清除业务状态和签名密文。
builds.rs: 创建构建，读取原生及镜像 Release、制品与日志游标。
executions.rs: 以兼容取消外键的写锁维护节点 session、事件与不可变制品登记。
scheduling.rs: 分配任务并生成不可变 Release。
planning.rs: 维护规划租约、工具结果和固定上下文。
planning_management.rs: 写入模型方案及规划 Worker 授权。
placements.rs: 保存节点候选并校验分配条件。
replication.rs: 协调副本归属和持久化唤醒。
runtime_bridge.rs: 转换 Runtime 规划意图与业务状态。
maintenance.rs: 回收过期租约并修复后台状态。
source_cache.rs: 记录固定源码快照和缓存占用。
lookups.rs: 读取调度、构建和授权对象。
node_metadata.rs: 记录启动元数据和能力摘要。
node_queries.rs: 执行项目范围内的节点查询。
ota.rs: 记录节点升级命令和健康回执。
ota_management.rs: 写入节点发布及升级目标。
performance.rs: 验证存储查询预算和响应规模。
execution_fences.rs: 以真实事件、心跳与取消事务验证节点锁等待及权限租约拒绝。

images.rs: 保存排空续跑身份、超时分类和发布仓库来源快照。

[PROTOCOL]: Update this header on change, then check AGENTS.md
