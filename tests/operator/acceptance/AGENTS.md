# acceptance/
> L2 | Parent: ../AGENTS.md

run.py: 部署隔离中间件；创建唯一 SemVer 版本和独立源码目标构建，验证规划与池恢复。
prepare.py: 构建独立镜像与双架构 RUN；核对 kind 归属后只导入指定平台，不解包证明清单。
test_prepare.py: 验证单平台导入和外部容器拒绝，覆盖旧 containerd 的证明清单边界。
Dockerfile: 在 Emissary 镜像中加入模型、Git 与双架构 RUN 夹具。
fixture.go: 提供同源 Git/LFS、app/image 固定目标和无自定义标签的 Release 规划夹具。
fixture_test.go: 验证 LFS 同源地址、构建目标、空标签和双架构 RUN 源码夹具。
live_provider.go: 转发显式选择的真实模型并记录脱敏时序。
provider.py: 经 Release 验证真实模型预算、取消和供应商故障。
native.py: 临时放行单节点Git NodePort；核对Release原生构建与四次真实模型调用，再撤销出口。
network.py: 用地址快照验证冷 Pod 首次 TCP 策略，独立记录 DNS。
telemetry.py: 预检池准入，经 Release 验证遥测中断并保存白名单执行诊断。
cleanup.py: 创建 Release 构建，按台账 UID 等待 Pi，验证失联撤权后恢复数据库和 DNS。
test_cleanup.py: 离线验证 Release 构建、Node Unknown 端点恢复与命令诊断脱敏。
kind-cilium.yaml: 创建禁用原生 sidecar 的 Kubernetes 1.28 Cilium 隔离集群。
test_harness.py: 验证版本元数据先于构建、构建独立源码与目标、会话和设备隔离。
test_telemetry.py: 验证 Release 构建超时、白名单执行诊断和暂停池拒绝。
test_network.py: 用真实 TCP 与解析故障验证探针不混淆 DNS 和策略。
test_native_network.py: 验证临时 Git 出口范围、受限就绪探针与异常撤销。

[PROTOCOL]: Update this header on change, then check AGENTS.md
