# charts/artifactory/templates/
> L2 | Parent: ../AGENTS.md

NOTES.txt: 输出部署后的访问提示。
_helpers.tpl: 定义资源命名和安全上下文。
agent-worker.yaml: 部署规划 Worker。
config.yaml: 生成非敏感服务配置。
control-plane.yaml: 部署 Control 和独立 Source；私有密钥环初始化沿用 Control 资源预算。
ingress.yaml: 暴露控制台与对象存储入口。
persistence.yaml: 创建持久化存储声明。
postgres.yaml: 部署 PostgreSQL 和 TimescaleDB。
s3.yaml: 部署私有 S3 服务。
services.yaml: 定义集群与设备服务地址。
telemetry.yaml: 部署遥测服务。
web.yaml: 部署控制台和网关。

[PROTOCOL]: Update this header on change, then check AGENTS.md
