# agent-operator/
> L2 | Parent: ../AGENTS.md

Cargo.toml: 声明 Agent Operator 独立 crate 和 CRD 生成程序。
Dockerfile: 构建只包含 Agent Operator 的运行镜像。
src/: 定义资源契约、选主和协调循环。
config/: 保存离线生成的 CRD。
tests/: 验证任务资源、固定身份和选主。

[PROTOCOL]: Update this header on change, then check AGENTS.md
