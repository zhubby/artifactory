# .github/workflows/
> L2 | Parent: ../AGENTS.md

ci.yml: 固定 kind 0.33.0 与 Kubernetes 1.37，允许精确节点 CIDR 策略；验证临时源码、隔离 SSH、会话与跨服务契约。
deployment-images.yml: 按源码 SHA 发布十个独立服务的 amd64 镜像。
emissary-release.yml: 生成 Emissary 各平台发布产物。
release-images.yml: 发布独立双架构镜像和四个版本化 Chart。
trial-image-mirrors.yml: 保留数据库、S3、tls-edge、Registry 与 BuildKit 固定 digest。
web-image.yml: 单独发布固定源码的 Web 镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
