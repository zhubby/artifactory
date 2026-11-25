# charts/artifactory/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 声明应用与 Chart 版本，以及 Kubernetes 1.28 最低版本。
values.yaml: 定义网关接单开关、镜像默认池、连接预算、通知出口与私有存储。
values-ha.yaml: 定义控制平面多副本参数。
values-kind.yaml: 定义本机 kind 参数。
values-trial.yaml: 定义隔离试部署参数。
templates/: 渲染业务、存储和只读密钥挂载。
.helmignore: 排除打包缓存和不能作为 Kubernetes 清单的模块地图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
