# image-operator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明镜像入口与共享执行依赖，最小监督入口不启用 Operator 特性。
src/: 直接协调临时 Pod，以 UID 台账和收尾 Finalizer 防止重复执行。
tests/: 验证 Pod API、逐容器收尾、UID 归属与失联容量。
config/: 保存离线 Image CRD。
Dockerfile: 构建独立 Image Operator 镜像。
Dockerfile.worker: 构建 Runner 和 Publisher 工作镜像。
Dockerfile.buildkit: 将静态 Rust 监督入口加入固定 rootless BuildKit 镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
