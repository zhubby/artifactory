# tests/
> L2 | Parent: ../AGENTS.md

leadership.rs: 使用本机 API 夹具验证选主和过期隔离。
resources.rs: 验证直接 Pod、池代理传播、源码凭据隔离与不可变 CR 台账。
pod_lifecycle.rs: 验证创建回执、旧版本围栏、未知配额和终态保护释放。
support/: 提供受版本约束的 Kubernetes 与 Runtime HTTP 夹具。

[PROTOCOL]: Update this header on change, then check AGENTS.md
