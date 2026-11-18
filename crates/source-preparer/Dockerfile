# [INPUT]: Rust 工作区与共享临时检出二进制。
# [OUTPUT]: 交付支持 amd64/arm64 的源码 init 镜像。
# [POS]: 由两个 Operator 启动一次性任务。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
ARG RUST_IMAGE=rust:1.95-bookworm
ARG BASE_IMAGE=debian:bookworm-slim
FROM ${RUST_IMAGE} AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY vendor ./vendor
COPY crates ./crates
COPY operators ./operators
COPY proto ./proto
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --locked --release -p artifactory-source-preparer && cp target/release/artifactory-source-preparer /usr/local/bin/
FROM ${BASE_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates git git-lfs openssh-client && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
COPY --from=builder /usr/local/bin/artifactory-source-preparer /usr/local/bin/
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/artifactory-source-preparer"]
