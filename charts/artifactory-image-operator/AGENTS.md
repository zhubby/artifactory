# artifactory-image-operator/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 定义独立交付包、版本与 Kubernetes 1.33 下限。
values.yaml: 声明源码准备镜像、BuildKit 与预建 Secret。
templates/: 渲染服务和最小 RBAC。
crds/: 保存 Image CRD。
.helmignore: 排除模块地图，避免被当作 Kubernetes 资源。

[PROTOCOL]: Update this header on change, then check AGENTS.md
