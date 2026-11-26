# crates/telemetry-service/src/store/
> L2 | Parent: ../AGENTS.md

mod.rs: 组合遥测存储与只读授权，分页保护当前startup摘要并清扫历史。
connect.rs: 初始化遥测读写池，校验业务只读连接和版本。
connect/: 使用当前迁移元数据验证只读账号授权边界。
history.rs: 按startup查询实时观察，标明legacy来源，并有界读取历史和聚合遥测。
ingest.rs: 校验批次并写入遥测、清单和幂等回执。
inventory.rs: 以事务回执确认独立清单，按startup隔离最新观察并限流。
models.rs: 定义 Toasty 遥测数据投影。
purge.rs: 按工作区归属分批删除遥测数据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
