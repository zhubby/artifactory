# charts/artifactory/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 声明应用与 Chart 版本。
values.yaml: 定义基础配置和可选签名密钥环 Secret。
values-ha.yaml: 定义控制平面多副本参数。
values-kind.yaml: 定义本机 kind 参数。
values-trial.yaml: 定义隔离试部署参数。
templates/: 渲染业务、存储和只读密钥挂载。
.helmignore: 排除打包缓存和不能作为 Kubernetes 清单的模块地图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
