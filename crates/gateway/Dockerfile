# [INPUT]: 依赖Pingora crate 与 Rustls 构建依赖。
# [OUTPUT]: 交付独立 HTTP 和 TCP 网关镜像。
# [POS]: 由部署配置选择公共、内部或设备入口。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
ARG RUST_IMAGE=rust:1.95-bookworm
ARG BASE_IMAGE=debian:bookworm-slim
FROM ${RUST_IMAGE} AS builder
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler cmake pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY vendor ./vendor
COPY crates ./crates
COPY operators ./operators
COPY proto ./proto
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/build/target cargo build --locked --release -p artifactory-gateway --bin artifactory-gateway && cp target/release/artifactory-gateway /usr/local/bin/service
FROM ${BASE_IMAGE}
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
COPY --from=builder /usr/local/bin/service /usr/local/bin/artifactory-gateway
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/artifactory-gateway"]
