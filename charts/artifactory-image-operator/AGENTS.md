# artifactory-image-operator/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 定义独立交付包、版本与 Kubernetes 1.28 下限。
values.yaml: 定义资源前缀、源码与监督镜像、分角色预算、具名池代理和 Secret。
templates/: 渲染控制器和仅含 Pod 编排的最小 RBAC。
crds/: 保存 Image CRD。
.helmignore: 排除模块地图，避免被当作 Kubernetes 资源。

[PROTOCOL]: Update this header on change, then check AGENTS.md
