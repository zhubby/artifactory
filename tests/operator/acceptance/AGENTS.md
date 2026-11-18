# acceptance/
> L2 | Parent: ../AGENTS.md

run.py: 验证任务交付和池暂停恢复，收集全副本日志。
prepare.py: 构建独立镜像，以目标架构临时归档导入 kind。
Dockerfile: 在 Emissary 镜像中加入模型与 Git 夹具。
fixture.go: 提供确定性模型、同源 Git/LFS 和故障开关。
fixture_test.go: 通过真实 TLS 验证 LFS 下载保留 Service 与 NodePort 地址。
live_provider.go: 转发显式选择的真实模型并记录脱敏时序。
provider.py: 验证真实模型预算、取消和供应商故障。
native.py: 临时放行单节点 Git NodePort；双端确认 refs 就绪，验证原生构建后撤销出口。
network.py: 用地址快照验证冷 Pod 首次 TCP 策略，独立记录 DNS。
telemetry.py: 预检池准入，验证遥测中断并保存白名单构建诊断。
cleanup.py: 传递独立 curl 会话租户，验证节点失联时的撤权和清理。
kind-cilium.yaml: 创建允许安装 Cilium 的隔离 kind 配置。
test_harness.py: 验证会话、池恢复、全副本日志和设备隔离。
test_telemetry.py: 验证白名单构建诊断、超时收尾和暂停池拒绝。
test_network.py: 用真实 TCP 与解析故障验证探针不混淆 DNS 和策略。
test_native_network.py: 验证临时 Git 出口范围、受限就绪探针与异常撤销。

[PROTOCOL]: Update this header on change, then check AGENTS.md
