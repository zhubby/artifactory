# deploy/
> L2 | Parent: ../AGENTS.md

Caddyfile: 自动终止公网 TLS，整个入口转发至 Pingora。
seaweed-entrypoint.sh: 复制私有 S3 凭据后启动非 root 存储。
timescale-init.sh: 初始化独立遥测数据库与扩展。
telemetry-business-readonly.sql: 授予遥测认证所需的最小业务读取。
kubernetes-trial.yaml: 保存试用命名空间配额和部署账号。
releases/: 保存已发布试用版本的不可变镜像配置。

[PROTOCOL]: Update this header on change, then check AGENTS.md
