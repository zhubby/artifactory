# crates/source-preparer/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 声明一次性源码任务二进制依赖。
Dockerfile: 交付多架构 Git/LFS init 镜像。
src/: 以 Pod 身份领取凭据，固定元数据并临时检出。
tests/: 验证认证来源和幂等元数据提交。

[PROTOCOL]: Update this header on change, then check AGENTS.md
