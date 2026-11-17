# crates/control-plane/migrations/migrations/
> L2 | Parent: ../AGENTS.md

0000_baseline.sql: 定义原始业务表与约束。
0001_control_replication.sql: 增加副本归属与持久化调度。
0002_node_placements.sql: 增加节点候选与分配条件。
0003_multitenancy.sql: 增加租户、授权范围和引用约束。
0004_readable_routes.sql: 增加租户、项目 slug 与构建编号。
0005_account_settings.sql: 增加个人设置。
0006_workspace_lifecycle.sql: 增加归档、删除和工作区写入门禁。
0007_managed_signing.sql: 增加加密签名库、版本快照和制品证据约束。

0008_image_builds.sql: 隔离镜像执行、凭据、仓库来源快照与源码授权。

[PROTOCOL]: Update this header on change, then check AGENTS.md
