# images/
> L2 | Parent: ../AGENTS.md

mod.rs: 校验启动配置并导出单轮调度，重试 Kubernetes 客户端。
dispatch.rs: 固定 CR UID 与完整 spec，复核当前成功状态并回收材料。
tasks.rs: 验证完整源码清单与 commit，仅交付 ID 和摘要；复核仓库凭据快照。
registry.rs: 校验 RSA 配对，以 RFC 7638 kid 签发 Distribution 3 范围令牌。

[PROTOCOL]: Update this header on change, then check AGENTS.md
