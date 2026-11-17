# [INPUT]: Alpine 基础镜像与实际构建节点架构。
# [OUTPUT]: 包含构建架构记录的最小运行镜像。
# [POS]: 验证双平台构建与摘要拉取的示例。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
FROM alpine:3.22
RUN uname -m > /build-architecture
CMD ["cat", "/build-architecture"]
