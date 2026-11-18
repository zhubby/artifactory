# .github/workflows/
> L2 | Parent: ../AGENTS.md

ci.yml: 独立验证 1.28 普通容器镜像生命周期与 1.37 Cilium 网络隔离。
deployment-images.yml: 按源码 SHA 发布十一个服务与构建运行镜像的 amd64 产物。
emissary-release.yml: 生成 Emissary 各平台发布产物。
release-images.yml: 发布含监督型 BuildKit 的双架构镜像和四个版本化 Chart。
trial-image-mirrors.yml: 保留数据库、S3、tls-edge、Registry 与 BuildKit 固定 digest。
web-image.yml: 单独发布固定源码的 Web 镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
