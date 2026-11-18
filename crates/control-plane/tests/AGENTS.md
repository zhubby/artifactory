# crates/control-plane/tests/
> L2 | Parent: ../AGENTS.md

config.rs: 验证执行配方和阶段限制。
crypto.rs: 验证凭据加密上下文。
database.rs: 组合隔离数据库夹具及业务验收。
database/: 保存分模块数据库与 HTTP 验收。
fixtures/: 保存传输验收夹具。
permissions.rs: 验证 HTTP 和项目权限。
scheduling.rs: 验证授权节点和调度约束。
storage_errors.rs: 验证对象存储故障。
storage_tls.rs: 验证 HTTPS 存储证书。

[PROTOCOL]: Update this header on change, then check AGENTS.md
