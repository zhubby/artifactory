# image-operator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明独立镜像控制器、Runner、Publisher 与 CRD 程序。
src/: 定义 Kubernetes 协调与隔离构建进程。
config/: 保存离线 Image CRD。
Dockerfile: 构建独立 Image Operator 镜像。
Dockerfile.worker: 构建 Runner 和 Publisher 工作镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
