# [INPUT]: 独立 Emissary 镜像与已编译的模型/Git 夹具。
# [OUTPUT]: 交付验收上游与设备共用的临时镜像。
# [POS]: 复用设备运行依赖，不依赖 Operator 控制器镜像。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
ARG BASE_IMAGE=localhost:5001/artifactory/emissary:rust-native
FROM ${BASE_IMAGE}
USER root
RUN apt-get update && apt-get install -y --no-install-recommends tini && rm -rf /var/lib/apt/lists/*
COPY fixture /usr/local/bin/fixture
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/fixture"]
