# crates/control-plane/tests/
> L2 | Parent: ../AGENTS.md

config.rs: 验证应用平台、执行配方和阶段限制。
crypto.rs: 验证凭据加密上下文。
bootstrap_auth.rs: 验证固定初始密码的 Argon2 散列与常规密码策略隔离。
database.rs: 组合隔离数据库夹具，验证授权设备下的构建和业务边界。
database/: 保存分模块数据库与 HTTP 验收。
fixtures/: 保存传输验收夹具。
permissions.rs: 验证 HTTP 和项目权限。
scheduling.rs: 验证授权节点和调度约束。
storage_errors.rs: 验证对象存储故障。
storage_tls.rs: 验证 HTTPS 存储证书。

baseline.rs: 验证初始化、概览索引兼容追加迁移与历史数据保留；误连未知历史时无损拒绝。

[PROTOCOL]: Update this header on change, then check AGENTS.md
