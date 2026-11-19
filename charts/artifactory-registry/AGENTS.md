# artifactory-registry/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 定义独立交付包、应用版本与 Kubernetes 1.28 下限。
values.yaml: 定义资源前缀、Gateway 标签和预建存储身份 Secret。
templates/: 渲染私有 Distribution、只读认证证书与 Gateway 专属入口策略。
.helmignore: 排除模板目录中的模块地图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
