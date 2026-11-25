# crates/execution-support/
> L2 | Parent: ../AGENTS.md

Cargo.toml: Unix回收依赖libc/sysinfo；terminal特性提供PTY和Windows作业能力。
src/: 供原生节点和镜像 Worker 复用执行原语。
tests/: 验证进程回收、输入隔离与私有文件边界。

[PROTOCOL]: Update this header on change, then check AGENTS.md
