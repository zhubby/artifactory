# crates/
> L2 | Parent: ../AGENTS.md

control-plane/: 提供租户授权、任务状态与镜像发布协调。
database/: 提供共享数据库连接、迁移与 SQL 边界。
emissary/: 提供原生构建节点、mTLS 会话与源码校验。
emissary-protocol/: 定义节点通信、签名、资源与遥测契约。
execution-support/: 提供原生与镜像执行共享的进程、事件、私有写入和脱敏。
gateway/: 提供 Pingora HTTP 反向代理与设备 TLS 透传。
image-protocol/: 定义平台镜像结果、任务与 OCI Index。
oci-registry-client/: 提供仓库认证、镜像内容校验与按摘要复制 OCI 图的共享客户端。
planning-coordinator/: 提供控制平面内嵌规划状态机与任务派发。
planning-protocol/: 定义纯数据规划意图、执行身份和租约契约。
service-transport/: 提供认证 HTTP、TLS、密封凭据、出口代理和有界指标。
source-checkout/: 提供固定 Git、子模块和 LFS 的共享检出引擎。
source-preparer/: 提供临时解析、引用查询与 init 检出程序。
telemetry-service/: 独立写入与查询时序数据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
