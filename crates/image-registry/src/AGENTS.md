# image-registry/src/
> L2 | Parent: ../AGENTS.md

lib.rs: 导出认证 OCI 客户端。
auth.rs: 解析 Registry Bearer 挑战并限制凭据发送目标。
client.rs: 有界读取 Manifest，并通过临时文件校验和传输 Blob。
graph.rs: 校验真实架构并复制 Index 引用的完整镜像图。
graph_tests.rs: 通过本机 OCI 服务验证图顺序和损坏拒绝。

[PROTOCOL]: Update this header on change, then check AGENTS.md
