# .github/workflows/
> L2 | Parent: ../AGENTS.md

ci.yml: 在 1.28 基线验证镜像链、首次安装隔离与 Cilium，归档服务日志。
deployment-images.yml: 按源码 SHA 发布服务与构建运行镜像的 amd64 产物。
emissary-release.yml: 核对来源标签，发布根 VERSION 对应的六平台程序与签名清单。
release-images.yml: 发布含监督型 BuildKit 的双架构镜像和四个版本化 Chart。
trial-image-mirrors.yml: 保留数据库、S3、tls-edge、Registry 与 BuildKit 固定 digest。
web-image.yml: 单独发布固定源码的 Web 镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
