# tests/images/
> L2 | Parent: ../AGENTS.md

gateway.py: 验证监听失败退出、统一就绪、双 CA、空闲 HTTP 回收、写请求禁止重放和设备 mTLS。
run.py: 复用跨版本 AppArmor 安全规则，验证源码 init、真实 RUN、镜像图和材料清理。
setup.py: 配置 Registry 专用 SeaweedFS 和构建池；等待存储就绪后启动 Registry。
faults.py: 用 UID/CAS 和临时 finalizer 保存 deadline 终态；CRI 137 与内核同组记录证明 OOM。
node_unreachable.py: 双证实专用 kind 后停 kubelet，验证孤儿台账与容量；恢复节点并重建失败转发。
registry.py: 下载 Index 与全部 Blob，验证 digest、HEAD、Range、ELF 与实际架构标记。
arch-check.go: 交叉编译无网络静态程序，写入 Dockerfile RUN 的真实执行架构。
kind-1.28.yaml: 使用默认 CNI，明确关闭 SidecarContainers Alpha。
test_harness.py: 离线验证 API 默认化安全语义、节点恢复、OCI 真执行、deadline 收尾与 OOM 证据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
