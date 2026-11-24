# crates/execution-support/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明执行原语与可选 terminal 特性的 PTY、会话和作业依赖。
src/: 供原生节点和镜像 Worker 复用执行原语。
tests/: 验证进程回收、输入隔离与私有文件边界。

[PROTOCOL]: Update this header on change, then check AGENTS.md
