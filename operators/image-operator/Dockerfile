# [INPUT]: 依赖镜像 Operator crate 与 Kubernetes 契约。
# [OUTPUT]: 交付独立 Image Operator 控制器镜像。
# [POS]: 只执行控制循环；构建由 worker 镜像执行。
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
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --locked --release -p artifactory-image-operator --bin artifactory-image-operator && cp target/release/artifactory-image-operator /usr/local/bin/service
FROM ${BASE_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
COPY --from=builder /usr/local/bin/service /usr/local/bin/artifactory-image-operator
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/artifactory-image-operator"]
