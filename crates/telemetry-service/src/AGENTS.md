# crates/telemetry-service/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 校验设备遥测身份、限制接收并发并导出服务指标。
main.rs: 连接数据库并启动遥测 RPC、HTTP 与保留任务。
query.rs: 校验查询范围和服务令牌，提供遥测历史接口。
store/: 隔离时序读写、业务只读授权与数据清扫。

[PROTOCOL]: Update this header on change, then check AGENTS.md
