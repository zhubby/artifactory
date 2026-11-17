# source-service/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 声明独立源码快照服务与缓存依赖。
Dockerfile: 只交付 artifactory-source-service 二进制。
src/: 保存源码元数据并提供认证 RPC 与设备下载。
tests/: 验证固定源码、私有对象恢复与授权。
migrations/: 保存 Source 独立数据库迁移。

[PROTOCOL]: Update this header on change, then check AGENTS.md
