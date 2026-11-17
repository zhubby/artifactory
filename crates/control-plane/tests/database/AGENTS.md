# crates/control-plane/tests/database/
> L2 | Parent: ../AGENTS.md

account_settings.rs: 验证个人设置和项目资源权限。
build_logs.rs: 验证历史日志分页、过滤、授权和终态流结束。
device_load.rs: 验证大规模设备查询和调度预算。
local_source.rs: 验证源码服务与固定快照。
node_metadata.rs: 验证节点元数据及查询范围。
readable_routes.rs: 验证 slug 路由与对象权限。
registration.rs: 验证注册和账号生命周期。
replication.rs: 验证副本锁和租约回收。
runtime.rs: 验证 Runtime 持久化桥接。
signing.rs: 验证真实 PKCS12、固定版本、撤权、可用性与摘要证据。
source_refs.rs: 验证受权远端分支与标签查询。
tenancy.rs: 提供租户 HTTP 夹具及子模块入口。
tenancy_regressions.rs: 验证租户隔离和管理权限回归。
transport.rs: 验证执行 RPC 与传输状态。
workspace_lifecycle.rs: 验证工作区归档、删除和故障恢复。

images.rs: 验证镜像准入、轮转调度、固定发布与凭据撤销。
image_regressions.rs: 验证排空续跑、超时重发、日志清扫和发布快照。

[PROTOCOL]: Update this header on change, then check AGENTS.md
