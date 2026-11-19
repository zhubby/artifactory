# [INPUT]: 使用与专用节点架构一致的 Node.js 基础镜像。
# [OUTPUT]: 提供普通 Pod 生命周期 fixture 镜像。
# [POS]: 只含测试进程，不连接代码仓库或制品仓库。
# [PROTOCOL]: Update this header on change, then check AGENTS.md
FROM node:24-bookworm-slim
WORKDIR /app
COPY fixture.js /app/fixture.js
RUN mkdir dist && cp fixture.js dist/pod-main.js && \
    for binary in artifactory-source-preparer artifactory-image-runner artifactory-image-buildkit artifactory-image-publisher; do \
      printf '#!/bin/sh\nexec node /app/%s "$@"\n' "$binary" > /usr/local/bin/"$binary"; \
      ln -s fixture.js /app/"$binary"; chmod +x /usr/local/bin/"$binary"; \
    done
