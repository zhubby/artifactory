# templates/
> L2 | Parent: ../AGENTS.md

workloads.yaml: 部署 Agent Operator，按 release 隔离选择器，并声明规划接口与 AgentPool。
telemetry.yaml: 部署独立遥测服务，按 release 隔离选择器。
rbac.yaml: 授权直接 Pod、UID 删除和 CR finalizer。
network-policy.yaml: 约束规划与源码任务的服务、Git 和 DNS 访问。

[PROTOCOL]: Update this header on change, then check AGENTS.md
