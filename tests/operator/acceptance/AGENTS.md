# acceptance/
> L2 | Parent: ../AGENTS.md

run.py: 验证临时源码、AI init、会话回执收敛、预算恢复与原生制品。
prepare.py: 构建独立镜像，以目标架构临时归档导入 kind。
Dockerfile: 在 Emissary 镜像中加入模型与 Git 夹具。
fixture.go: 提供确定性模型、Git/LFS 和故障开关。
live_provider.go: 转发显式选择的真实模型并记录脱敏时序。
provider.py: 验证真实模型预算、取消和供应商故障。
native.py: 经共同可达的 Git NodePort 验证主机直拉与原生构建。
network.py: 用地址快照验证冷 Pod 首次 TCP 策略，独立记录 DNS。
telemetry.py: 验证遥测中断期间构建和持久重放。
cleanup.py: 传递独立 curl 会话租户，验证节点失联时的撤权和清理。
kind-cilium.yaml: 创建允许安装 Cilium 的隔离 kind 配置。
test_harness.py: 验证会话分页、回执等待与脱敏证据、设备隔离和镜像边界。
test_network.py: 用真实 TCP 与解析故障验证探针不混淆 DNS 和策略。

[PROTOCOL]: Update this header on change, then check AGENTS.md
