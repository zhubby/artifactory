# images/
> L2 | Parent: ../AGENTS.md

mod.rs: 校验镜像准入与启动配置并重试 Kubernetes 客户端。
dispatch.rs: 限制任务材料 900 KiB，核对执行身份并回收终态材料。
tasks.rs: 验证完整源码清单与 commit，仅交付 ID 和摘要；复核仓库凭据快照。
registry.rs: 校验 RSA 配对，以 RFC 7638 kid 签发 Distribution 3 范围令牌。

[PROTOCOL]: Update this header on change, then check AGENTS.md
