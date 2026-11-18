# tests/images/
> L2 | Parent: ../AGENTS.md

gateway.py: 验证监听失败退出、统一就绪、双 CA、空闲 HTTP 回收、写请求禁止重放和设备 mTLS。
run.py: 验证源码 init、真实 RUN、镜像图和材料清理；只采集监督入口的已知 I/O 枚举。
setup.py: 复用 Acceptance 安装与恢复，等待 S3 就绪后配置仓库；撤销会话后关闭转发。
faults.py: 核对 UID 与完整 cgroup 路径后注入；CRI 137 与内核同组 OOM 记录共同证明 OOM。
node_unreachable.py: 双证实专用 kind 后停 kubelet，验证孤儿台账与容量；恢复节点并重建失败转发。
registry.py: 下载 Index 与全部 Blob，验证 digest、HEAD、Range、ELF 与实际架构标记。
arch-check.go: 交叉编译无网络静态程序，写入 Dockerfile RUN 的真实执行架构。
kind-1.28.yaml: 使用默认 CNI，明确关闭 SidecarContainers Alpha。
test_harness.py: 离线验证隔离、节点恢复、OCI 真执行、UID/cgroup 归属与内核 OOM 证据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
