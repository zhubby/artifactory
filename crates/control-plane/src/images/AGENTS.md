# images/
> L2 | Parent: ../AGENTS.md

mod.rs: 校验镜像准入与启动配置并重试 Kubernetes 客户端。
dispatch.rs: 核对执行身份，区分超时和取消后回收任务材料。
tasks.rs: 固定源码，核对凭据的仓库快照并校验仓库内容。
registry.rs: 校验 RSA 密钥证书配对并签发多仓库范围令牌。

[PROTOCOL]: Update this header on change, then check AGENTS.md
