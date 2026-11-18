FROM node:24-bookworm-slim AS builder
WORKDIR /app/packages/agent-worker
COPY packages/agent-worker/package.json packages/agent-worker/package-lock.json ./
RUN npm ci
COPY packages/agent-worker/tsconfig.json ./
COPY packages/agent-worker/src ./src
RUN npm run build && npm prune --omit=dev

FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
WORKDIR /app/packages/agent-worker
COPY --from=builder /app/packages/agent-worker/package.json ./
COPY --from=builder /app/packages/agent-worker/node_modules ./node_modules
COPY --from=builder /app/packages/agent-worker/dist ./dist
USER 10001:10001
ENTRYPOINT ["node", "dist/job-main.js"]
