# crates/
> L2 | Parent: ../AGENTS.md

control-plane/: 提供租户授权、任务状态与镜像发布协调。
database/: 提供共享数据库连接、迁移与 SQL 边界。
emissary/: 提供原生构建节点、mTLS 会话与源码校验。
emissary-protocol/: 定义固定源码和设备执行契约。
gateway/: 提供 Pingora HTTP 反向代理与设备 TLS 透传。
image-protocol/: 定义平台镜像结果、任务与 OCI Index。
image-registry/: 校验镜像内容并按摘要复制 OCI 图。
planner-runtime/: 提供独立规划状态机与任务派发。
runtime-protocol/: 定义规划运行时通信契约。
source-core/: 校验源码身份、凭据引用和快照。
source-service/: 提供隔离源码服务与缓存。
telemetry-model/: 定义设备资源和遥测数据。
telemetry-service/: 独立写入与查询时序数据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
