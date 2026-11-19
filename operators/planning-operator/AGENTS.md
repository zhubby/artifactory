# planning-operator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明 Planning Operator 独立 crate 和 CRD 生成程序。
Dockerfile: 构建只包含 Planning Operator 的运行镜像。
src/: 定义直接 Pod、项目配额、持久创建意图与已证实停机回收。
config/: 保存离线生成的 CRD。
tests/: 验证直接 Pod 资源、UID 围栏、恢复、重试与选主。

[PROTOCOL]: Update this header on change, then check AGENTS.md
