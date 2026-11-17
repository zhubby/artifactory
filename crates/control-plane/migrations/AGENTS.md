# crates/control-plane/migrations/
> L2 | Parent: ../AGENTS.md

history.toml: 按不可变 ID 排列数据库迁移。
0003_snapshot.toml: 保存多租户 CRUD 模型快照。
0004_snapshot.toml: 保存可读路由迁移后的模型快照。
0005_snapshot.toml: 保存账户设置后的模型快照。
0006_snapshot.toml: 保存工作区生命周期后的模型快照。
0007_snapshot.toml: 保留 SQL 托管签名迁移后的未变 CRUD 投影。
migrations/: 保存带数据库约束和触发器的增量 SQL。

0008_snapshot.toml: 保留镜像 SQL 表之外的 CRUD 模型快照。

[PROTOCOL]: Update this header on change, then check AGENTS.md
