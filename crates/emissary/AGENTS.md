# crates/emissary/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 原生节点二进制、节点协议和共享执行依赖。
Dockerfile: 限定 Emissary 包，隔离工作区特性后构建 Linux 节点镜像。
PORTING.md: Rust 执行节点的行为契约与验收边界。
src/: mTLS 会话、隔离执行、源码校验与 OTA。
tests/: 真实进程、Git、存储与原生服务验收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
