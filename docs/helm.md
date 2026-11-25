<!--
[INPUT]: 主 Chart、独立 Operator Chart 与持久身份。
[OUTPUT]: 定义 Kubernetes 1.28 安装、入口排空和统一网关。
[POS]: Kubernetes 部署操作入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Helm / Kubernetes 部署

主 Chart 位于 `charts/artifactory`。它部署内嵌规划协调器的 Control、Next.js、双副本 Pingora、TimescaleDB / PostgreSQL 17、私有 SeaweedFS S3 和遥测服务。独立 Chart 交付 Planning Operator、Image Operator 和 Distribution 仓库。Planning Operator Chart 的遥测默认关闭，仅供独立验收启用。平台最低支持 Kubernetes 1.28。完整源码、AI 规划与镜像构建链在 1.28 及以上版本使用同一实现。四个 Chart 的版本约束均为 `>=1.28.0-0`。

默认 Deployment 名称为 `artifactory-planning-operator`、`artifactory-image-operator`、`artifactory-registry`、`artifactory-timescaledb` 和 `artifactory-seaweedfs`。Registry、TimescaleDB 和 SeaweedFS 的 Service 与对应 Deployment 同名。TimescaleDB 和 SeaweedFS 的 PVC 也使用该名称。

Control 使用 `emptyDir`。源码解析任务和每个执行 Pod 直接向上游仓库拉取源码。源码目录、Git 对象、LFS 和 BuildKit 状态只存在于任务临时卷。平台只持久化固定 commit、manifest、AI 对话、计划、日志、制品与最终镜像。详见 [Operator 运维](operator-operations.md) 和 [镜像构建](image-builds.md)。

业务数据库初始化基线为 `2026112101`，规划数据库基线为 `2026111901`。同一业务基线自动追加迁移 `2026112201` 镜像失败分类、`2026112301`–`2026112303` 通知基础设施及 `2026112401` 概览构建时间索引、`2026112402` 源码阶段与 `2026112403` 设备终端，保留业务历史。先升级 Control 完成迁移，再升级会校验完整迁移身份的 Telemetry。创建概览索引前备份业务库，等待现有构建结束。未知数据库历史或旧 CRD 不提供兼容入口。程序不会自动重置数据库或删除旧 Deployment、CRD、Pod、PVC。新旧数据库实例使用独立命名空间和持久身份，旧实例由管理员另行备份和退役。下文的 Service 整合仅适用于同一数据库基线内的部署。

设备终端新增 Control 私有 8446 认证流与独立网关配额。升级时保留原 PKI，先滚动 Control 完成迁移，再更新包含同一迁移集合的 Telemetry。浏览器与设备入口仍经过现有网关。试用命名空间的宽泛入站策略必须排除 Control，避免相加策略放开 8446。配置见[设备终端](terminals.md)。

通知使用 Control 内的独立后台任务，无需新服务。`notifications.smtpSecretName` 引用现有 SMTP Secret；`notifications.webhookPrivateAllowlist` 仅放行显式私网 CIDR。外发订阅默认关闭，配置与验签见[通知中心](notifications.md)。

镜像失败分类升级顺序为 Control、Telemetry、BuilderPool 的 `workerImage`。Control 先写入迁移身份。Telemetry 必须使用包含同一迁移集合的镜像。Worker 再发送新的结构化失败字段。

`auth.registrationEnabled` 默认 `true`，访客注册后没有租户或项目权限，需要创建租户或接受邀请。关闭公开注册时设置 `--set auth.registrationEnabled=false`；该开关映射到 control-plane 的 `ARTIFACTORY_REGISTRATION_ENABLED`，不会停用已有账号或会话。

## Release 镜像

GitHub `release-images` 工作流仅在 **Release published** 时执行，正式版本和预发布版本都构建。它检出该 Release 的 tag，在原生 AMD64、ARM64 runner 分别构建，再发布包含 `linux/amd64` / `linux/arm64` 的 GHCR manifest：

- `ghcr.io/zhubby/artifactory-control-plane:<release-tag>`
- `ghcr.io/zhubby/artifactory-web:<release-tag>`
- `ghcr.io/zhubby/artifactory-planning-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-planning-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-source-preparer:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-buildkit:<release-tag>`
- `ghcr.io/zhubby/artifactory-gateway:<release-tag>`
- `ghcr.io/zhubby/artifactory-emissary:<release-tag>`

标签包含原始 Release tag、完整 `sha-<commit>`；语义版本额外提供不带 `v` 的版本标签。只有正式 Release 更新 `latest`。工作流使用 `GITHUB_TOKEN` 的 `packages:write`，不需要另存 PAT；它不会创建 Release。发布前先让对应提交的 `platform-contracts` 检查通过。首次 GHCR 包如果为私有，需要设置包可见性，或通过 `imagePullSecrets` 提供有读取权限的凭据。

## 首次准备凭据与 PKI

需要 Kubernetes 1.28+、Helm 3、kubectl、Python 3、OpenSSL，以及供数据库和制品存储使用的默认 StorageClass。先确认 kube context 与目标命名空间。以下命令中的域名、镜像版本、Ingress 类和 TLS Secret 均需替换为实际配置。

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

新安装的初始账号固定为 `admin / 123456`。Control 仅在业务库没有用户时创建该账号。修改 Secret 或重启服务不会重置已有账号密码。

Chart 引用已有 Secret。默认资源前缀为 `artifactory`，与 Helm release 名称无关。默认 Secret 为 `artifactory-credentials`、`artifactory-pki` 和 `artifactory-gateway`，可分别通过 `secrets.credentials`、`secrets.pki` 和 `gateway.secretName` 覆盖。自定义前缀时，四个 Chart 的 `fullnameOverride` 与初始化脚本的 `--resource-prefix` 使用相同的 DNS 标签。前缀以小写字母起始，取前 46 个字符并去掉末尾连字符，为 `-gateway-internal` 保留长度。凭据中的数据库 URL 指向同一前缀的 `timescaledb` Service。

脚本同时生成 `registry-signing.json` 和 `registry-signing-public.json`。启用镜像构建前应用这两个 Secret；前者包含独立 RSA 私钥和公开证书，后者只含公开证书。Control 默认使用 `artifactory-registry-signing`；Registry Chart 的 `auth.certificateSecret` 指向对应公开证书。仓库 Bucket 和存储凭据须独立准备，脚本不创建它们。

默认数据库镜像为 `timescale/timescaledb:2.32.0-pg17`。新卷自动初始化独立遥测库与非管理员应用角色；`credentials.json` 另包含 `telemetry-database-password`、`telemetry-database-url` 和 `telemetry-token`。`telemetry.endpoint` 应填写 Emissary 可访问、证书 SAN 覆盖的 HTTPS 8444 地址；8093 只供 Control 内部查询。已有 PostgreSQL 卷不自动转换，部署配置与 TimescaleDB 恢复步骤见 [时序观测](telemetry.md)。

新生成的服务器证书包含 `<prefix>-gateway`、`<prefix>-gateway.<namespace>.svc`、内部网关和遥测 Service 的 SAN，并保留旧 gRPC Service 的 SAN。已有部署通过原公开域名或 IP 访问时，Service 整合不要求更换 CA 或服务器证书。首次使用新 Service DNS 连接设备端口前，先核验证书包含该 DNS。

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

公开 Service `<prefix>-gateway` 选择统一 Pingora Pod。Service 80 端口转发至 Pod 8080，将 `/api/*` 转发给 Control、`/v2/*` 转发给内建仓库、`/artifacts/object` 转发给私有对象，并将页面转发给 Next.js。同一个 Service 的 8443、8444 端口分别透传设备控制和遥测。Gateway Deployment 默认两个副本。内部 `<prefix>-gateway-internal:8445` 保持独立 ClusterIP。各端口保持独立协议和路由边界。Ingress 指向公开网关 80 端口，必须关闭响应缓冲并允许实际制品和镜像层大小。

启用内建数据库、S3、遥测和独立仓库时，共有以下八个 Service：

| Service | 端口 | 访问边界 |
|---|---|---|
| `<prefix>-gateway` | 80、8443、8444 | 网页、API、仓库、制品和设备入口 |
| `<prefix>-gateway-internal` | 8445 | 集群内部 HTTPS |
| `<prefix>-control-plane` | 8080、8443 | 私有控制后端 |
| `<prefix>-web-internal` | 3000 | 私有 Next.js 后端 |
| `<prefix>-telemetry` | 8093、8444 | 私有遥测查询和写入 |
| `<prefix>-timescaledb` | 5432 | 私有数据库 |
| `<prefix>-seaweedfs` | 8333 | 私有制品存储 |
| `<prefix>-registry` | 5000 | 私有 OCI 仓库 |

同一组 Pod 的端口可以共用 Service。不同后端需要独立 Service；一个 selector 不能按端口选择不同的 Pod。内部 8445 不能并入公开 NodePort，否则 Kubernetes 会为它分配外部入口。

Web 镜像同时包含由 mdBook 生成的完整静态文档，安装后访问控制台同域名的 `/docs`，无需登录，也不需要额外的 Service、PVC 或 Ingress 路径。文档随 Web 镜像一起升级和回滚；构建与维护说明见[文档站点](operations.md#文档站点)。

gRPC 使用 `<prefix>-gateway:8443`，转发至私有 Control Service 8443，保留服务器 TLS 与节点 mTLS。通过受信网络的 TCP LoadBalancer、NodePort 或 TLS passthrough 暴露，确保 Agent 访问的主机名/IP 在证书 SAN 中。不要使用普通 HTTP Ingress 终止这条私有 CA 认证连接，也不要公开数据库或 SeaweedFS 内部管理端口。

公开入口统一配置为 `gateway.service`。使用 NodePort 时，例如：

```yaml
gateway:
  service:
    type: NodePort
    nodePorts:
      http: 31080
      grpc: 31443
      telemetry: 31444
```

三个固定 NodePort 必须互不相同。省略某个值时，由 Kubernetes 分配该端口。`console.publicUrl` 和 `telemetry.endpoint` 仍分别填写浏览器、设备实际可达的外部地址。

## 本机 kind

本机 profile 显式启用 HTTP 开发模式；gRPC 仍校验私有 CA。默认端口为控制台和制品 31080、设备控制 31443、设备遥测 31444。S3 保持集群内部地址。先确认端口未被其他 Service 使用。

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

macOS 常规 kind 网络不能直接访问节点 NodePort。默认 values 使用 loopback，在同一个终端转发三个公开端口：

```sh
kubectl --context YOUR_KIND_CONTEXT -n artifactory port-forward \
  service/artifactory-gateway 31080:80 31443:8443 31444:8444
```

浏览器访问 `http://127.0.0.1:31080`。Agent 注册使用 `127.0.0.1:31443`、本次 `pki/ca.pem`，并仅在这个开发环境添加 `--allow-http-uploads`。使用实际节点 IP 时，更新 `console.publicUrl`，并将该 IP 加入 `--grpc-host`。浏览器和制品票据使用同一个公开网关地址；不要随意切换 localhost / IP。

## 检查与升级

### 源码准备协议切换

源码准备复用在业务库和规划库追加 `2026112501`。该升级要求现有任务排空。先单独滚动支持接单开关的 Gateway 镜像，再打开 `gateway.buildAdmissionPaused=true`。确认所有 Gateway 副本使用新镜像且该值为 `true` 后，再检查活跃任务。不要在同一次滚动升级中同时打开开关和迁移 Control。

开关映射到 `ARTIFACTORY_GATEWAY_BUILD_ADMISSION_PAUSED`，默认 `false`。暂停时，以下公开请求返回 `503`、`Retry-After: 60` 和不可缓存的 JSON 错误：

- `POST /api/v1/releases/{id}/builds`
- `POST /api/v1/jobs/{id}/retry`
- `POST /api/v1/builds/{id}/clarifications`

读取、取消、引用查询、已有任务的内部回执和健康检查继续工作。开关不取消已有任务，也不阻止设备升级与通知重试。新构建只能通过公开 Gateway 接入；不要向外暴露 Control 的私有 Service。

排空后，确认没有活跃源码、规划、原生和镜像任务，并备份业务库、规划库、当前 Helm values 与固定镜像摘要。协调升级 Control、Telemetry、Planning Operator、Source Preparer、Planning Worker 和 Web。迁移完成后，用真实构建验证新协议、后续执行和临时卷回收。最后将 `gateway.buildAdmissionPaused=false`，等待全部 Gateway 副本完成滚动，再验证公开入口接单。

迁移后的恢复必须同时考虑两库和运行协议。不要只回滚旧 Control 或旧 Source Preparer 镜像。需要恢复旧版本时，先停止接单并排空新任务，再使用已验证的两库备份与对应的组件版本恢复。

### 整合已有 Service

旧 Chart 分别创建 `<prefix>-web`、`<prefix>-grpc` 和 `<prefix>-telemetry-grpc`。当前 Chart 将前两个合并为 `<prefix>-gateway`，将遥测写入端口并入 `<prefix>-telemetry`。旧 `console.service`、`grpc.service` 配置会触发渲染错误，必须先迁移到 `gateway.service`，不能直接沿用 `--reuse-values`。

1. 将当前 Helm values、manifest、Service UID 与端口、PVC 和 Secret UID 保存到私有目录。确认目标命名空间 UID，并保留所有固定镜像摘要和现场覆盖。
2. 将旧公开 HTTP、gRPC、遥测 NodePort 分别写入 `gateway.service.nodePorts.http/grpc/telemetry`，设置统一的 `gateway.service.type`。删除 `console.service` 和 `grpc.service`，保留 `console.publicUrl`、设备外部地址及其他部署参数。`telemetry.businessDatabaseSecretKey` 指定已有凭据 Secret 中的业务库连接键，默认 `database-url`；已有部署使用独立只读身份时保留该参数。使用 `--reset-values -f <migrated-values>`，防止旧默认值重新进入配置。
3. 渲染并比较全部资源。只允许本次 Service 整合、网关遥测后端地址及 Ingress 引用发生变化。提前给现有 telemetry Service 增加 8444 端口，并创建新 gateway ClusterIP；新 Service 的 selector、Helm 所属标签和 release 注解必须与渲染结果一致。确认 EndpointSlice 已包含就绪后端。
4. 保持旧公开 Service 和 telemetry-grpc Service 可用。先将现有 Gateway Deployment 的 `ARTIFACTORY_GATEWAY_TELEMETRY` 改为 `<prefix>-telemetry:8444`，执行滚动更新并等待所有副本就绪。核验公开入口与遥测连接后，继续转移端口。
5. 先将旧公开 Service 转为 ClusterIP，释放原 NodePort，再将新 gateway Service 切换为目标类型并接管这些端口。不同 Service 不能同时占用同一 NodePort，此步骤会有短暂入口切换。不要先直接创建占用原 NodePort 的新 Service。
6. 确认新入口就绪后，执行已核验的 Helm 升级。新 Service 由同一 Helm release 接管，旧公开 Service 和 telemetry-grpc Service 此时才移除。不要依赖同一次 Helm 升级先完成 Pod 排空再删除旧 Service。
7. 核验八个 Service、公开端口、设备 TLS、内部 HTTPS、登录和文档。对照快照确认数据库、PVC、Secret、CA 和非目标工作负载保留。失败时按已保存的 Service 端口和 Helm 历史恢复；恢复旧 NodePort 前先释放新 Service 对它们的占用。

Service 整合不删除命名空间，不重建数据库，不重新初始化凭据或 CA。

### 常规核验

```sh
kubectl --context YOUR_CONTEXT -n artifactory get pods,pvc,services
curl -fsS https://build.example.com/healthz
helm --kube-context YOUR_CONTEXT -n artifactory history artifactory
```

健康检查成功后，仍需验证真实登录、CSRF、Agent mTLS 注册/心跳、SSE 和一次制品构建/下载。HTTP kind 联测不代表公网 TLS、真实业务应用、实体机启动恢复、发行签名或生产备份恢复已验收。

Control 多副本直接使用独立临时卷。连接预算和排空配置见 [Control 多副本](control-ha.md)。Secret 更新后容器环境变量不会自动刷新，需要重启对应 Deployment。数据库用户密码须通过数据库修改，初始化环境变量只作用于新卷。

应用 PVC 带有 `helm.sh/resource-policy: keep`。卸载 Helm release 会保留数据库与制品存储卷；删除命名空间仍会删除 PVC。密钥、设备 CA、数据库和制品对象应纳入备份。

## 独立 Operator 与仓库

主 Chart 的 `planning.coordinatorUrl` 指向内部网关的 `/internal/planning`，`planning.namespace` 指向 Planning Operator 所在命名空间，`planning.pool` 选择 PlanningPool。完整 Secret、TLS 与安装命令见 [Operator 运维](operator-operations.md)。

Planning Operator 协调 `PlanningRun` 与 `SourceTask`。`SourceTask` 解析 ref 或查询 refs；AI 规划 Pod 的 init container 准备固定源码。Image Operator 为每个平台创建普通源码 init、Runner 和监督型 BuildKit 两个普通容器，并单独创建 Publisher。它们不创建源码 StatefulSet、Service、PVC 或共享缓存，也不依赖原生 sidecar 功能开关。

公开入口经 `https://build.example.com` 进入 Pingora。内部服务使用 `https://<prefix>-gateway-internal:8445`。内部 TLS Secret 提供 `ca.pem/server.pem/server.key`，证书 SAN 覆盖内部 Service 名称。规划协调器和源码准备容器信任同一服务 CA。Control 通过 Kubernetes TokenReview 与 Pod→CR 的 UID 链确认任务身份；普通 Worker 不挂载源码凭据和源码专用 token。

内建 Registry 使用独立私有 S3 Bucket。Control 的 RSA 签名身份与设备 CA 分离。配置与推送/拉取验证见 [镜像构建](image-builds.md)。

## 临时源码验收

在一次 AI、原生构建和双架构镜像任务运行期间，验证卷类型、只读源码挂载和身份隔离。任务结束后验证清理：

```sh
python3 scripts/check_ephemeral_source.py --context YOUR_CONTEXT --namespace artifactory
python3 scripts/check_ephemeral_source.py --context YOUR_CONTEXT --namespace artifactory --idle
```

脚本只读 Kubernetes 元数据，不读取 Secret 内容。`--idle` 要求该命名空间没有任务 Pod、Job 和镜像任务 Secret。数据库、制品 PVC、最终镜像与设备 PKI 不属于临时任务清理范围。
