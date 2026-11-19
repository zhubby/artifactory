# acceptance/
> L2 | Parent: ../AGENTS.md

run.py: 验证直接 Pod 丢失恢复、任务交付和池暂停；收集全副本日志。
prepare.py: 构建独立镜像与双架构 RUN；核对 kind 归属后只导入指定平台，不解包证明清单。
test_prepare.py: 验证单平台导入和外部容器拒绝，覆盖旧 containerd 的证明清单边界。
Dockerfile: 在 Emissary 镜像中加入模型、Git 与双架构 RUN 夹具。
fixture.go: 提供确定性模型、同源 Git/LFS、显式应用分类与 native/BuildKit 规划和故障开关。
fixture_test.go: 验证 LFS 同源地址、显式镜像平台与仿真设置，以及真实 RUN 源码夹具。
live_provider.go: 转发显式选择的真实模型并记录脱敏时序。
provider.py: 验证真实模型预算、取消和供应商故障。
native.py: 临时放行单节点 Git NodePort；双端确认 refs 就绪，验证原生构建后撤销出口。
network.py: 用地址快照验证冷 Pod 首次 TCP 策略，独立记录 DNS。
telemetry.py: 预检池准入，验证遥测中断并保存白名单构建与 Pod 执行台账诊断。
cleanup.py: 按执行台账固定 Pod 名称、UID 和停止证据，验证失联撤权与收尾。
test_cleanup.py: 离线复跑失联验收，验证直接 Pod 台账与物理停机证据，并拒绝身份替换。
kind-cilium.yaml: 创建允许安装 Cilium 的隔离 kind 配置。
test_harness.py: 验证会话、池恢复、全副本日志和设备隔离。
test_telemetry.py: 验证白名单构建与执行台账诊断、超时收尾和暂停池拒绝。
test_network.py: 用真实 TCP 与解析故障验证探针不混淆 DNS 和策略。
test_native_network.py: 验证临时 Git 出口范围、受限就绪探针与异常撤销。

[PROTOCOL]: Update this header on change, then check AGENTS.md
