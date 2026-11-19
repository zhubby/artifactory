# templates/
> L2 | Parent: ../AGENTS.md

workloads.yaml: 部署 Agent Operator，按 release 隔离选择器，声明规划接口与具名池代理。
telemetry.yaml: 部署独立遥测服务，按 release 隔离选择器。
rbac.yaml: 授权直接 Pod、UID 删除和 CR finalizer。
network-policy.yaml: 约束任务服务、Git 和 DNS 出站，显式放行池代理的 TCP 端口。

[PROTOCOL]: Update this header on change, then check AGENTS.md
