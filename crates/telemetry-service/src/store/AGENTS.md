# crates/telemetry-service/src/store/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合遥测存储和业务授权查询，导出迁移契约。
connect.rs: 初始化遥测读写池，校验业务只读连接和版本。
connect/: 使用当前迁移元数据验证只读账号授权边界。
history.rs: 查询实时快照、历史游标和聚合遥测。
ingest.rs: 校验批次并写入遥测、清单和幂等回执。
models.rs: 定义 Toasty 遥测数据投影。
purge.rs: 按工作区归属分批删除遥测数据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
