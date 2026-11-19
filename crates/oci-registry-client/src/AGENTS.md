# oci-registry-client/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 向 Control 与 Publisher 导出共享 OCI 仓库客户端。
auth.rs: 解析 Registry Bearer 挑战并限制凭据发送目标。
client.rs: 有界读取 Manifest，并通过临时文件校验和传输 Blob。
graph.rs: 校验架构、Release 标签和完整 Manifest 集合，按摘要复制镜像图。
graph_tests.rs: 通过本机 OCI 服务验证图顺序和损坏拒绝。

[PROTOCOL]: Update this header on change, then check AGENTS.md
