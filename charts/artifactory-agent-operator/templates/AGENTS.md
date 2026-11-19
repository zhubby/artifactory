# templates/
> L2 | Parent: ../AGENTS.md

workloads.yaml: 部署 Agent Operator 与 Runtime，并声明 AgentPool 与源码准备镜像。
telemetry.yaml: 部署独立遥测服务。
rbac.yaml: 授权直接 Pod、UID 删除和 CR finalizer；Runtime 只读 Pod 与 AgentRun。
network-policy.yaml: 约束规划与源码任务的服务、Git 和 DNS 访问。

[PROTOCOL]: Update this header on change, then check AGENTS.md
