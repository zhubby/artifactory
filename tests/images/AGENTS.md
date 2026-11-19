# tests/images/
> L2 | Parent: ../AGENTS.md

gateway.py: 验证监听就绪、固定下载文件名、正式 Manifest 围栏、空闲回收和设备 mTLS。
run.py: 经 Release 创建镜像构建，发布后验证正式版本标签、外仓分发和材料清理。
setup.py: 配置 Registry 专用存储、Gateway 网络白名单和发布停机核验权限。
faults.py: 用 UID/CAS 和临时 finalizer 保存 deadline 终态；CRI 137 与内核同组记录证明 OOM。
node_unreachable.py: 双证实专用 kind 后停 kubelet，验证孤儿台账与容量；恢复节点并重建失败转发。
registry.py: 下载完整 OCI 图，验证 digest、Release 元数据、HEAD、Range 与真实架构。
arch-check.go: 交叉编译无网络静态程序，写入 Dockerfile RUN 的真实执行架构。
kind-1.28.yaml: 使用默认 CNI，明确关闭 SidecarContainers Alpha。
test_harness.py: 离线验证 Release 发布等待、OCI 元数据、容器隔离和真实故障证据。

[PROTOCOL]: Update this header on change, then check AGENTS.md
