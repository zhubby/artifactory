# artifactory-agent-operator/
> L2 | Parent: ../../AGENTS.md

Chart.yaml: 声明 Agent Operator 统一版本与 Kubernetes 1.28 最小版本。
values.yaml: 定义资源前缀、独立 Operator、规划接口、源码镜像与显式池代理。
templates/: 部署独立服务、权限和网络边界。
crds/: 安装 AgentPool、AgentRun 与 SourceTask CRD。
.helmignore: 排除模板和 CRD 目录中的模块地图。

[PROTOCOL]: Update this header on change, then check AGENTS.md
