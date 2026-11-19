# charts/artifactory/templates/
> L2 | Parent: ../AGENTS.md

NOTES.txt: 输出部署后的访问提示。
_helpers.tpl: 统一新实例的资源前缀、私有 SeaweedFS 地址与安全上下文。
config.yaml: 生成非敏感服务配置。
control-plane.yaml: 部署无状态 Control，使用临时目录和只读持久身份。
gateway.yaml: 部署双副本统一网关，按端口和来源隔离入口。
image-dispatch.yaml: 授权 Control 下发 CR、核验执行停机与 Pod 归属并调用 TokenReview。集群角色名区分资源前缀和命名空间。
ingress.yaml: 单域名接入统一 Pingora HTTP 入口。
persistence.yaml: 只创建数据库和对象存储的 PVC。
postgres.yaml: 部署 TimescaleDB 和数据库持久卷。
s3.yaml: 部署 SeaweedFS 和对象存储持久卷。
services.yaml: 定义集群与设备服务地址。
telemetry.yaml: 部署遥测私有后端，拒绝绕过网关的公开入口。
web.yaml: 部署独立控制台与静态文档。

[PROTOCOL]: Update this header on change, then check AGENTS.md
