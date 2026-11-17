# [INPUT]: Rust 工作区与 Source 独立二进制。
# [OUTPUT]: 交付固定版本的源码快照服务镜像。
# [POS]: 由 Agent Operator 编排持久化源码分片。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
ARG RUST_IMAGE=rust:1.95-bookworm
ARG BASE_IMAGE=debian:bookworm-slim
FROM ${RUST_IMAGE} AS builder
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY vendor ./vendor
COPY crates ./crates
COPY operators ./operators
COPY proto ./proto
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --locked --release -p artifactory-source-service --bin artifactory-source-service && cp target/release/artifactory-source-service /usr/local/bin/service
FROM ${BASE_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates git git-lfs openssh-client && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
COPY --from=builder /usr/local/bin/service /usr/local/bin/artifactory-source-service
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/artifactory-source-service"]
