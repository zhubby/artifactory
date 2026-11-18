# image-operator/src/bin/
> L2 | Parent: ../AGENTS.md

buildkit.rs: 启动无业务依赖的 Linux PID 1 监督程序，支持 musl 静态交付。
crdgen.rs: 从 Rust 契约生成离线 CRD。
runner.rs: 启动固定任务的单平台构建进程。
publisher.rs: 启动独立镜像发布进程。

[PROTOCOL]: Update this header on change, then check AGENTS.md
