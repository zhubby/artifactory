# crates/emissary/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 原生节点与Prost清单依赖；Telemetry服务仅用于真实持久确认测试，Windows采集IP Helper。
Dockerfile: 限定 Emissary 包，隔离工作区特性后构建 Linux 节点镜像。
PORTING.md: Rust 执行节点的行为契约与验收边界。
src/: 主机识别、接口诊断缓存、mTLS 会话、有限操作、隔离执行与 OTA。
tests/: 真实进程、Git、存储与原生服务验收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
