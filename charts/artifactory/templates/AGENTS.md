# charts/artifactory/templates/
> L2 | Parent: ../AGENTS.md

NOTES.txt: 输出部署后的访问提示。
_helpers.tpl: 定义资源命名和安全上下文。
config.yaml: 生成非敏感服务配置。
control-plane.yaml: 部署无状态 Control，使用临时目录和只读持久身份。
gateway.yaml: 部署双副本统一网关，按端口和来源隔离入口。
image-dispatch.yaml: 授权 Control 下发 CR、校验直接 Pod 归属和调用 TokenReview。
ingress.yaml: 单域名接入统一 Pingora HTTP 入口。
persistence.yaml: 只创建数据库和对象存储的 PVC。
postgres.yaml: 部署 PostgreSQL 和 TimescaleDB。
s3.yaml: 部署私有 S3 服务。
services.yaml: 定义集群与设备服务地址。
telemetry.yaml: 部署遥测私有后端，拒绝绕过网关的公开入口。
web.yaml: 部署独立控制台与静态文档。

[PROTOCOL]: Update this header on change, then check AGENTS.md
