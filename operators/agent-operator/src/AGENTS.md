# agent-operator/src/
> L2 | Parent: ../AGENTS.md

api.rs: 定义项目身份和预留名称、创建意图、UID、截止时间、停机证据台账。
controller.rs: 上报规划 Pod 故障，保留未知配额，终态清净后释放 CR 保护。
pods.rs: 用最新 CR 版本围栏创建，用直接归属查询确认保护释放。
resources.rs: 创建规划和源码普通 Pod，只使用 Kubernetes 1.28 稳定字段。
source_tasks.rs: 固定总期限和三次源码尝试，终态清净后释放 CR 保护。
leadership.rs: 通过 Lease 互斥写入并校验租约。
lib.rs: 导出 Agent Operator 模块。
main.rs: 启动独立控制器和选主循环。
bin/: 输出离线 Agent CRD。

[PROTOCOL]: Update this header on change, then check AGENTS.md
