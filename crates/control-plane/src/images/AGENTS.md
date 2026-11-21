# images/
> L2 | Parent: ../AGENTS.md

mod.rs: 校验启动配置并导出单轮调度，重试 Kubernetes 客户端。
pools.rs: 冻结独立默认池，事务外查池；404 可修正，其余故障重试。
pool_tests.rs: 验证默认与显式池、404 与故障类型、超时和原生无集群依赖。
dispatch.rs: 固定 CR UID 与完整 spec，复核终态 generation 后保存结果或失败原因。
tasks.rs: 校验固定源码与凭据快照，仅交付 ID 和摘要；通过 OCI 仓库客户端复核镜像报告。
registry.rs: 校验 RSA 配对，以 RFC 7638 kid 签发 Distribution 3 范围令牌。
release.rs: 按各 CR 终态协议复核物理停机，再以发布租约推广并读回完整镜像图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
