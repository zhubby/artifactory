# crates/control-plane/migrations/migrations/
> L2 | Parent: ../AGENTS.md

0000_baseline.sql: 创建租户、源码任务、镜像候选及两类 CR UID 绑定；保存 Release 版本说明、构建独立来源与目标、交付冻结和发布租约；仅安装到新数据库。
0001_image_failure_kind.sql: 为镜像失败追加有界分类；历史未分类结果保持空值。

[PROTOCOL]: Update this header on change, then check AGENTS.md
