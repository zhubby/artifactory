FROM node:24-bookworm-slim AS builder
WORKDIR /app/packages/planning-worker
COPY packages/planning-worker/package.json packages/planning-worker/package-lock.json ./
RUN npm ci
COPY packages/planning-worker/tsconfig.json ./
COPY packages/planning-worker/src ./src
RUN npm run build && npm prune --omit=dev

FROM node:24-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home artifactory
WORKDIR /app/packages/planning-worker
COPY --from=builder /app/packages/planning-worker/package.json ./
COPY --from=builder /app/packages/planning-worker/node_modules ./node_modules
COPY --from=builder /app/packages/planning-worker/dist ./dist
USER 10001:10001
ENTRYPOINT ["node", "dist/pod-main.js"]
