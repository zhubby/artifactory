# acceptance/
> L2 | Parent: ../AGENTS.md

run.py: 创建隔离服务栈，验证固定源码和网关制品路径。
prepare.py: 构建独立镜像，以目标架构临时归档导入 kind。
Dockerfile: 在 Emissary 镜像中加入模型与 Git 夹具。
fixture.go: 提供确定性模型、Git/LFS 和故障开关。
live_provider.go: 转发显式选择的真实模型并记录脱敏时序。
provider.py: 验证真实模型预算、取消和供应商故障。
native.py: 验证主机原生设备并恢复临时公告地址。
network.py: 验证新 Pi Pod 首次网络访问的隔离策略。
telemetry.py: 验证遥测中断期间构建和持久重放。
recovery.py: 恢复四个数据库、私有对象和密钥。
cleanup.py: 验证 kubelet 不可达时的授权撤销和清理。
kind-cilium.yaml: 创建允许安装 Cilium 的隔离 kind 配置。
test_harness.py: 验证网关凭据边界和独立镜像构建上下文。

[PROTOCOL]: Update this header on change, then check AGENTS.md
