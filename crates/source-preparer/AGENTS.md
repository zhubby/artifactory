# crates/source-preparer/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 声明一次性源码任务二进制依赖。
Dockerfile: 交付多架构 Git/LFS init 镜像。
src/: 以 Pod 身份领取凭据，轻量解析提交并在规划 Pod 准备最终源码。
tests/: 验证认证来源、幂等回执和真实 Git/LFS 目录复用。

[PROTOCOL]: Update this header on change, then check AGENTS.md
