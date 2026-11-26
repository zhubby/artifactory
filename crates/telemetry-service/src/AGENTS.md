# crates/telemetry-service/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 以mTLS协商格式，聚合采样与清单接收计数，固定拒收标签与64个RPC许可。
main.rs: 连接数据库并启动遥测 RPC、HTTP 与保留任务。
query.rs: 校验查询范围和服务令牌，提供遥测历史接口。
store/: 隔离时序读写、业务只读授权与数据清扫。

[PROTOCOL]: Update this header on change, then check AGENTS.md
