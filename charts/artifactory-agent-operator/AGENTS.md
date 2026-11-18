# artifactory-agent-operator/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 声明 Agent Operator 统一版本与 Kubernetes 1.33 最小版本。
values.yaml: 配置独立 Operator、Runtime、源码准备与遥测镜像。
templates/: 部署独立服务、权限和网络边界。
crds/: 安装 AgentPool、AgentRun 与 SourceTask CRD。
.helmignore: 排除模板和 CRD 目录中的模块地图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
