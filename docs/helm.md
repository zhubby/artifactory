<!--
[INPUT]: 主 Chart、独立 Operator Chart 与持久身份。
[OUTPUT]: 定义新实例安装、任务临时存储与统一网关。
[POS]: Kubernetes 部署操作入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Helm / Kubernetes 部署

主 Chart 位于 `charts/artifactory`。它部署 Control、Next.js、双副本 Pingora、TimescaleDB / PostgreSQL 17、私有 SeaweedFS S3 和遥测服务。独立 Chart 交付 Agent Operator、Image Operator 和 Distribution 仓库。完整源码、AI 规划与镜像构建链运行在 Kubernetes 1.33 及以上版本。

Control 使用 `emptyDir`。源码解析任务和每个执行 Pod 直接向上游仓库拉取源码。源码目录、Git 对象、LFS 和 BuildKit 状态只存在于任务临时卷。平台只持久化固定 commit、manifest、AI 对话、计划、日志、制品与最终镜像。详见 [Operator 运维](operator-operations.md) 和 [镜像构建](image-builds.md)。

当前 schema 是原型的新实例基线，不提供旧数据库、CRD 或 values 的兼容和迁移入口。部署当前版本使用全新业务数据库。

`auth.registrationEnabled` 默认 `true`，访客可自助注册为全实例只读用户，查看和下载全部项目的构建与产物。仅允许管理员分配账号的部署应在安装或升级时设置 `--set auth.registrationEnabled=false`；该开关映射到 control-plane 的 `ARTIFACTORY_REGISTRATION_ENABLED`，不会停用已有账号或会话。

## Release 镜像

GitHub `release-images` 工作流仅在 **Release published** 时执行，正式版本和预发布版本都构建。它检出该 Release 的 tag，在原生 AMD64、ARM64 runner 分别构建，再发布包含 `linux/amd64` / `linux/arm64` 的 GHCR manifest：

- `ghcr.io/zhubby/artifactory-control-plane:<release-tag>`
- `ghcr.io/zhubby/artifactory-web:<release-tag>`
- `ghcr.io/zhubby/artifactory-agent-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-agent-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-planner-runtime:<release-tag>`
- `ghcr.io/zhubby/artifactory-source-preparer:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-gateway:<release-tag>`

标签包含原始 Release tag、完整 `sha-<commit>`；语义版本额外提供不带 `v` 的版本标签。只有正式 Release 更新 `latest`。工作流使用 `GITHUB_TOKEN` 的 `packages:write`，不需要另存 PAT；它不会创建 Release。发布前先让对应提交的 `platform-contracts` 检查通过。首次 GHCR 包如果为私有，需要设置包可见性，或通过 `imagePullSecrets` 提供有读取权限的凭据。

## 首次准备凭据与 PKI

需要 Kubernetes 1.33+、Helm 3、kubectl、Python 3、OpenSSL，以及供数据库和制品存储使用的默认 StorageClass。先确认 kube context 与目标命名空间。以下命令中的域名、镜像版本、Ingress 类和 TLS Secret 均需替换为实际配置。

```sh
python3 scripts/init-helm.py \
  --namespace artifactory --release artifactory \
  --grpc-host agents.example.com \
  --output .deploy/helm/production

kubectl --context YOUR_CONTEXT create namespace artifactory
kubectl --context YOUR_CONTEXT apply \
  -f .deploy/helm/production/credentials.json \
  -f .deploy/helm/production/pki.json \
  -f .deploy/helm/production/gateway.json
```

`--output` 必须是新目录；脚本拒绝覆盖。生成目录与私钥只对当前用户开放。`credentials.json` 是 Kubernetes Secret，保存数据库口令、加密主密钥、S3 身份及管理员初始口令；`pki.json` 包含签发 CA、服务器证书及私钥。`access.json` 提供初始登录信息，`pki/ca.pem` 通过可信渠道交给 Agent。均保留在 Git 忽略的 `.deploy/` 中，应离线加密备份。

Chart 引用已有 Secret，**不生成或轮换任何密钥**。默认名称为 `<release>-credentials`、`<release>-pki` 和 `<release>-gateway`，分别通过 `secrets.credentials`、`secrets.pki` 和 `gateway.secretName` 覆盖。使用 `fullnameOverride` 时，初始化脚本的 `--resource-prefix` 必须与之对应。凭据中的数据库 URL 使用同一资源前缀的 PostgreSQL Service。

脚本同时生成 `registry-signing.json` 和 `registry-signing-public.json`。启用镜像构建前应用这两个 Secret；前者包含独立 RSA 私钥和公开证书，后者只含公开证书。Control 使用 `<release>-registry-signing`；Registry Chart 的 `auth.certificateSecret` 指向对应公开证书。仓库 Bucket 和存储凭据须独立准备，脚本不创建它们。

默认数据库镜像为 `timescale/timescaledb:2.32.0-pg17`。新卷自动初始化独立遥测库与非管理员应用角色；`credentials.json` 另包含 `telemetry-database-password`、`telemetry-database-url` 和 `telemetry-token`。`telemetry.endpoint` 应填写 Emissary 可访问、证书 SAN 覆盖的 HTTPS 8444 地址；8093 只供 Control 内部查询。已有 PostgreSQL 卷不自动转换，部署配置与 TimescaleDB 恢复步骤见 [时序观测](telemetry.md)。

CA 有效期十年，gRPC 服务器证书一年；到期前使用原 CA 为正确 SAN 续签，更新 PKI Secret 并安排 control-plane 重启。不要重新初始化整个目录来续签，亦不要替换已有部署的 master key。

## 生产配置

准备 `production-values.yaml`，例如：

```yaml
images:
  control-plane:
    tag: v0.2.0
  web:
    tag: v0.2.0
console:
  publicUrl: https://build.example.com
s3:
  endpoint: https://private-objects.internal.example.com
ingress:
  enabled: true
  className: nginx
  consoleHost: build.example.com
  consoleTlsSecret: artifactory-console-tls
  annotations:
    nginx.ingress.kubernetes.io/proxy-buffering: "off"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
    nginx.ingress.kubernetes.io/proxy-body-size: "0"
```

```sh
helm lint charts/artifactory --strict -f production-values.yaml
helm upgrade --install artifactory charts/artifactory \
  --kube-context YOUR_CONTEXT --namespace artifactory \
  -f production-values.yaml --wait --timeout 10m
```

示例版本必须已发布；Chart 不会自动创建域名、Ingress Controller 或 TLS Secret。公开控制台要求 HTTPS。内建 S3 默认使用固定私有 ClusterIP HTTP，Chart 为该地址显式启用 `ARTIFACTORY_S3_ALLOW_HTTP`。外部 `s3.endpoint` 默认要求 HTTPS；私有 HTTP 端点须显式设置 `s3.allowHttp=true`，不会放宽公开入口。客户端通过 Gateway 下载制品。Chart 不创建 S3 Ingress，S3 Service 固定为 ClusterIP。组织内部 S3 CA 需加入 Control 镜像内的系统信任库，不能只在宿主安装。

Web Service 指向统一 Pingora 的 8080 端口；它将 `/api/*` 转发给 Control、`/v2/*` 转发给内建仓库、`/artifacts/object` 转发给私有对象，并将页面转发给 Next.js。Gateway Deployment 默认两个副本。设备 Service 使用同一组 Pod 的 8443/8444 端口，内部 ClusterIP 使用 8445 端口。各端口保持独立协议和路由边界。Ingress 必须关闭响应缓冲并允许实际制品和镜像层大小。

Web 镜像同时包含由 mdBook 生成的完整静态文档，安装后访问控制台同域名的 `/docs`，无需登录，也不需要额外的 Service、PVC 或 Ingress 路径。文档随 Web 镜像一起升级和回滚；构建与维护说明见[文档站点](operations.md#文档站点)。

gRPC 使用独立的 `<release>-grpc:8443` Service，连接 Pingora 设备网关。它转发至私有 Control Service 8443，保留服务器 TLS 与节点 mTLS。通过受信网络的 TCP LoadBalancer、NodePort 或 TLS passthrough 暴露，确保 Agent 访问的主机名/IP 在证书 SAN 中。不要使用普通 HTTP Ingress 终止这条私有 CA 认证连接，也不要公开数据库或 SeaweedFS 内部管理端口。

## 本机 kind

本机 profile 显式启用 HTTP 开发模式；gRPC 仍校验私有 CA。默认端口为控制台和制品 31080、gRPC 31443。S3 保持集群内部地址。先确认端口未被其他 Service 使用。

```sh
python3 scripts/init-helm.py \
  --namespace artifactory --release artifactory \
  --grpc-host 127.0.0.1 --grpc-host localhost \
  --output .deploy/helm/kind
kubectl --context YOUR_KIND_CONTEXT create namespace artifactory
kubectl --context YOUR_KIND_CONTEXT apply \
  -f .deploy/helm/kind/credentials.json -f .deploy/helm/kind/pki.json \
  -f .deploy/helm/kind/gateway.json
```

未发布 Release 时，可以从本次源码构建并加载本地镜像：

```sh
docker build -t artifactory-control-plane:kind .
docker build -f web/Dockerfile -t artifactory-web:kind .
docker build -f crates/gateway/Dockerfile -t artifactory-gateway:kind .
kind load docker-image --name YOUR_CLUSTER artifactory-control-plane:kind artifactory-web:kind artifactory-gateway:kind

helm upgrade --install artifactory charts/artifactory \
  --kube-context YOUR_KIND_CONTEXT --namespace artifactory \
  -f charts/artifactory/values-kind.yaml \
  --set images.control-plane.repository=artifactory-control-plane --set images.control-plane.tag=kind \
  --set images.web.repository=artifactory-web --set images.web.tag=kind \
  --set images.gateway.repository=artifactory-gateway --set images.gateway.tag=kind \
  --wait --timeout 10m
```

如果 kind 已配置本地 registry，也可把镜像推入该 registry，并在 Helm values 设置对应 repository。不要重建已有集群来加载镜像。

macOS 常规 kind 网络不能直接访问节点 NodePort。默认 values 使用 loopback，分别在两个终端运行：

```sh
kubectl --context YOUR_KIND_CONTEXT -n artifactory port-forward service/artifactory-web 31080:80
kubectl --context YOUR_KIND_CONTEXT -n artifactory port-forward service/artifactory-grpc 31443:8443
```

浏览器访问 `http://127.0.0.1:31080`。Agent 注册使用 `127.0.0.1:31443`、本次 `pki/ca.pem`，并仅在这个开发环境添加 `--allow-http-uploads`。使用实际节点 IP 时，更新 `console.publicUrl`，并将该 IP 加入 `--grpc-host`。浏览器和制品票据使用同一个公开网关地址；不要随意切换 localhost / IP。

## 检查与升级

```sh
kubectl --context YOUR_CONTEXT -n artifactory get pods,pvc,services
curl -fsS https://build.example.com/healthz
helm --kube-context YOUR_CONTEXT -n artifactory history artifactory
```

健康检查成功后，仍需验证真实登录、CSRF、Agent mTLS 注册/心跳、SSE 和一次制品构建/下载。HTTP kind 联测不代表公网 TLS、真实业务应用、实体机启动恢复、发行签名或生产备份恢复已验收。

Control 多副本直接使用独立临时卷。连接预算和排空配置见 [Control 多副本](control-ha.md)。Secret 更新后容器环境变量不会自动刷新，需要重启对应 Deployment。数据库用户密码须通过数据库修改，初始化环境变量只作用于新卷。

应用 PVC 带有 `helm.sh/resource-policy: keep`。卸载 Helm release 会保留数据库与制品存储卷；删除命名空间仍会删除 PVC。密钥、设备 CA、数据库和制品对象应纳入备份。

## 独立 Operator 与仓库

主 Chart 的 `planning.runtimeUrl` 指向独立 Runtime，`planning.namespace` 指向 Agent Operator 所在命名空间，`planning.agentPool` 选择 AgentPool。完整 Secret、TLS 与安装命令见 [Operator 运维](operator-operations.md)。

Agent Operator 协调 `AgentRun` 与 `SourceTask`。`SourceTask` 解析 ref 或查询 refs；AI 规划 Job 的 init container 准备固定源码。Image Operator 为每个平台创建源码 init container、原生 BuildKit sidecar 和 Runner，并单独创建 Publisher。它们不创建源码 StatefulSet、Service、PVC 或共享缓存。

公开入口经 `https://build.example.com` 进入 Pingora。内部服务使用 `https://<release>-gateway-internal:8445`。内部 TLS Secret 提供 `ca.pem/server.pem/server.key`，证书 SAN 覆盖内部 Service 名称。Runtime 和源码准备容器信任同一服务 CA。Control 通过 Kubernetes TokenReview 与 Pod→Job→CR 的 UID 链确认任务身份；普通 Worker 不挂载源码凭据和源码专用 token。

内建 Registry 使用独立私有 S3 Bucket。Control 的 RSA 签名身份与设备 CA 分离。配置与推送/拉取验证见 [镜像构建](image-builds.md)。

## 临时源码验收

在一次 AI、原生构建和双架构镜像任务运行期间，验证卷类型、只读源码挂载和身份隔离。任务结束后验证清理：

```sh
python3 scripts/check_ephemeral_source.py --context YOUR_CONTEXT --namespace artifactory
python3 scripts/check_ephemeral_source.py --context YOUR_CONTEXT --namespace artifactory --idle
```

脚本只读 Kubernetes 元数据，不读取 Secret 内容。`--idle` 要求该命名空间没有任务 Pod、Job 和镜像任务 Secret。数据库、制品 PVC、最终镜像与设备 PKI 不属于临时任务清理范围。
