# control-plane/src/
> L2 | Parent: ../AGENTS.md

api/: 提供按租户和项目鉴权的 HTTP 接口。
auth.rs: 校验会话、令牌、租户角色与项目权限。
build.rs: 为构建创建和发布入口共用 Release 版本约束。
coordination.rs: 协调副本内连接归属与持久化唤醒。
crypto.rs: 加密私有凭据并计算认证摘要。
dispatch.rs: 向当前连接发送可重放设备命令。
error.rs: 将业务失败转换为 HTTP 错误。
execution_plan.rs: 校验、密封和转换 AI 执行计划。
grpc.rs: 认证设备并接收构建事件。
lib.rs: 导出控制平面模块与协议。
lifecycle.rs: 执行工作区归档、恢复和删除任务。
local_source.rs: 隔离 Poll 规划后端的源码缓存所有权。
main.rs: 启动业务、源码或遥测服务模式。
node_query.rs: 按项目授权查询节点能力和资源。
ota.rs: 校验节点升级包并协调升级状态。
pki.rs: 创建服务和设备证书。
planning.rs: 驱动 AI 规划租约与执行计划提交。
recipe.rs: 校验历史 YAML 配方和执行步骤。
runtime_bridge.rs: 在业务状态与独立运行时之间传递规划意图。
scheduler.rs: 将待执行任务分配给授权设备。
settings.rs: 读取启动配置与密钥路径。
source.rs: 复用共享仓库校验并读取历史配方。
source_cache.rs: 固定兼容后端的 Git/LFS 快照与凭据来源。
source_cache/: 查询兼容后端的远端分支与标签。
state.rs: 组合存储、配置和进程内连接状态。
storage.rs: 签名、上传和校验私有 S3 对象。
store/: 持久化业务状态、租约、权限和任务事件。

[PROTOCOL]: Update this header on change, then check AGENTS.md
