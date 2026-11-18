# [INPUT]: 依赖 Debian、Git 与 OpenSSH server。
# [OUTPUT]: 提供本机与 CI 回归的隔离 sshd 镜像。
# [POS]: ssh.rs 通过 ARTIFACTORY_TEST_SSHD_IMAGE 显式选择。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends git openssh-server \
    && mkdir -p /run/sshd && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["/usr/sbin/sshd"]
