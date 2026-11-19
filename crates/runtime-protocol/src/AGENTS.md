# crates/runtime-protocol/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 定义固定源码身份、执行故障、Pod 丢失围栏与 generation 运行授权。
crypto.rs: 加密服务侧意图并计算幂等摘要。
error.rs: 将领域拒绝与瞬态服务故障转换为 HTTP 状态。
metrics.rs: 提供有界标签的 HTTP 指标。
service.rs: 校验端点，以 8 MiB 上限传递源码封套并监听 TLS。

[PROTOCOL]: Update this header on change, then check AGENTS.md
