# gateway/
> L2 | Parent: ../../AGENTS.md

Cargo.toml: 固定 Pingora 0.9.1，启用 Rustls、接受过滤回调与 tracing 日志。
Dockerfile: 交付独立网关运行镜像。
src/: 同进程提供四监听，全部就绪后对外报告健康；监听失败使进程退出。

[PROTOCOL]: Update this header on change, then check AGENTS.md
