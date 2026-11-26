# .github/workflows/
> L2 | Parent: ../AGENTS.md

ci.yml: 验证镜像链、遥测、切换及OTA；常规回归排除专用服务用例，Linux原生满盘由独立门禁执行。
deployment-images.yml: 按源码 SHA 发布服务与构建运行镜像的 amd64 产物。
emissary-release.yml: 固定来源标签，六平台维护和Linux原生满盘及ext4故障通过后签名发布。
release-images.yml: 发布含监督型 BuildKit 的双架构镜像和四个版本化 Chart。
trial-image-mirrors.yml: 保留数据库、S3、tls-edge、Registry 与 BuildKit 固定 digest。
web-image.yml: 单独发布固定源码的 Web 镜像。

[PROTOCOL]: Update this header on change, then check AGENTS.md
