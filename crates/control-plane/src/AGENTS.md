# control-plane/src/
> L2 | Parent: ../AGENTS.md

api/: 提供按租户和项目鉴权的 HTTP 接口。
auth.rs: 校验会话、令牌、租户角色与项目权限。
build.rs: 为构建创建和发布入口共用 Release 版本约束。
coordination.rs: 协调副本内连接归属与持久化唤醒。
crypto.rs: 加密私有凭据并计算认证摘要。
dispatch.rs: 向当前连接发送可重放设备命令。
error.rs: 将业务失败转换为 HTTP 错误。
execution_plan.rs: 要求显式应用分类，校验并密封 AI 执行计划。
grpc.rs: 认证设备并接收构建事件。
lib.rs: 导出控制平面模块与协议。
lifecycle.rs: 按游标执行工作区清理并清扫镜像日志前缀。
main.rs: 校验启动配置并启动 HTTP、gRPC 和后台协调。
node_query.rs: 按项目授权查询节点能力和资源。
ota.rs: 校验节点升级包并协调升级状态。
pki.rs: 创建服务和设备证书。
planning.rs: 分隔原生 LFS 能力与镜像准入，复验并提交执行计划。
recipe.rs: 校验应用分类、原生步骤和 BuildKit 目标；保留非应用 null。
runtime_bridge.rs: 以 8 MiB 封套传递意图和材料；材料出口复核 Runtime 身份。
scheduler.rs: 校验解析目标准入并向授权设备分配原生任务。
signing/: 提供托管材料加密、导入和默认签名约束。
settings.rs: 读取启动配置与密钥路径。
source.rs: 复用共享仓库校验与凭据引用契约。
source_tasks.rs: 固定源码 CR UID，核验令牌、直接 Pod 归属与不可变任务契约。
source_tasks_tests.rs: 验证绑定令牌、直接 CR 归属、重试身份及授权响应。
state.rs: 组合存储、配置和进程内连接状态。
storage.rs: 校验私有 S3 对象并签发网关对象票据。
store/: 持久化业务状态、租约、权限和任务事件。

images/: 协调固定源码镜像执行与仓库认证。

[PROTOCOL]: Update this header on change, then check AGENTS.md
