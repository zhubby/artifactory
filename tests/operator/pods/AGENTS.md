# pods/
> L2 | Parent: ../AGENTS.md

run.py: 接受 Kubernetes 1.28 及以上专用 kind，记录二进制 SHA256；以随机标签和 UID 围栏只删除本次新建命名空间；先收尾执行再删除命名空间。
coordinator.py: 提供有认证的本机 规划协调器替身，围栏 generation 与完整身份，保留终态并重排可恢复丢失。
fixture.js: 用短进程、非零退出和共享卷模拟容器生命周期；不执行业务构建。
Dockerfile: 打包与专用节点架构一致的生命周期 fixture。
test_harness.py: 验证版本下限、集群和命名空间围栏、规划身份与终态、故障恢复的独立清理。

[PROTOCOL]: Update this header on change, then check AGENTS.md
