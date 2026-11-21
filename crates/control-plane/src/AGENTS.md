# control-plane/src/
> L2 | Parent: ../AGENTS.md

api/: 提供按租户和项目鉴权的 HTTP 接口。
auth.rs: 校验会话、令牌和权限；隔离首次管理员密码与常规密码策略。
build.rs: 校验严格 SemVer，从文件名推断后缀并生成安全交付名称。
coordination.rs: 协调副本内连接归属与持久化唤醒。
crypto.rs: 加密私有凭据并计算认证摘要。
dispatch.rs: 向当前连接发送可重放设备命令。
error.rs: 将业务失败转换为 HTTP 错误。
execution_plan.rs: 要求显式应用分类，校验并密封 AI 执行计划。
grpc.rs: 共用名称与完整心跳预算校验，认证节点会话并接收构建事件。
grpc_tests.rs: 验证完整 hello 的组合预算、重连与超额拒绝。
lib.rs: 导出控制平面模块与协议。
lifecycle.rs: 按游标执行工作区清理并清扫镜像日志前缀。
main.rs: 首次创建默认管理员，启动 HTTP、gRPC、内嵌规划运行时和后台协调。
node_query.rs: 按项目授权查询节点能力和资源。
ota.rs: 校验节点升级包并协调升级状态。
pki.rs: 创建服务和设备证书。
planning.rs: 按本次构建的完整目标合同复验执行计划。
recipe.rs: 校验应用分类、执行宿主和原生配方目标。
planning_bridge.rs: 以内嵌 Coordinator 的受权 HTTP 接口传递意图和材料。材料出口复核 Coordinator 身份。
scheduler.rs: 注入固定版本，分配任务并验证停止后的交付推广。
signing/: 提供托管材料加密、导入和默认签名约束。
settings.rs: 读取启动配置与密钥路径。
source.rs: 复用共享仓库校验与凭据引用契约。
source_tasks.rs: 固定源码 CR UID，核验令牌、直接 Pod 归属与不可变任务契约。
source_tasks_tests.rs: 验证绑定令牌、直接 CR 归属、重试身份及授权响应。
state.rs: 组合存储、配置和进程内连接状态。
storage.rs: 校验私有 S3 对象，签发绑定方法、对象与交付文件名的网关票据。
store/: 持久化业务状态、租约、权限和任务事件。

images/: 协调固定源码镜像执行与仓库认证。

[PROTOCOL]: Update this header on change, then check AGENTS.md
