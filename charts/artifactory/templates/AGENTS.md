# charts/artifactory/templates/
> L2 | Parent: ../AGENTS.md

NOTES.txt: 输出部署后的访问提示。
_helpers.tpl: 统一新实例的资源前缀、私有 SeaweedFS 地址与安全上下文。
config.yaml: 生成非敏感服务配置。
control-plane.yaml: 部署无状态 Control，注入镜像默认池、通知出口及私有 Secret。
terminal-relay.yaml: 保留同命名空间 HTTP/gRPC，限定同实例 Control 间终端私有流。
gateway.yaml: 部署双副本统一网关，隔离端口、来源和终端长连接预算。
image-dispatch.yaml: 授权 Control 只读查 BuilderPool、下发 CR、核验停机并调用 TokenReview。集群角色名区分前缀和命名空间。
ingress.yaml: 单域名接入统一 Pingora HTTP 入口。
persistence.yaml: 只创建数据库和对象存储的 PVC。
postgres.yaml: 部署 TimescaleDB 和数据库持久卷。
s3.yaml: 部署 SeaweedFS 和对象存储持久卷。
services.yaml: 统一公网 HTTP 与设备多端口 Service，校验旧参数和端口冲突。
telemetry.yaml: 查询和写入共用私有 Service，授权查询使用指定 Secret 身份。
web.yaml: 部署独立控制台与静态文档。

[PROTOCOL]: Update this header on change, then check AGENTS.md
