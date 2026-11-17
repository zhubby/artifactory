<!--
[INPUT]: 主 Chart、独立 Operator Chart 与持久身份。
[OUTPUT]: 定义安装、统一网关升级与设备地址迁移。
[POS]: Kubernetes 部署操作入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Helm / Kubernetes 部署

从旧组件名称升级时，先按[模块布局与命名](module-layout.md#从旧布局升级)迁移 PVC、身份及自定义 values，再启动新组件。

Chart 位于 `charts/artifactory`，包含可配置副本数的 Control、Next.js、独立 Pingora 网关、TimescaleDB 2.32 / PostgreSQL 17、私有 SeaweedFS S3、Pi Agent worker 和 Rust Telemetry。独立 Chart 提供 Agent Operator、Image Operator 和 Distribution 仓库。完整安装步骤见 [镜像构建与网关](image-builds.md)。Control 多副本使用独立临时目录，由独立 Source 持有原数据卷；迁移步骤和连接预算见 [Control 多副本](control-ha.md)。

`auth.registrationEnabled` 默认 `true`，访客可自助注册为全实例只读用户，查看和下载全部项目的构建与产物。仅允许管理员分配账号的部署应在安装或升级时设置 `--set auth.registrationEnabled=false`；该开关映射到 control-plane 的 `ARTIFACTORY_REGISTRATION_ENABLED`，不会停用已有账号或会话。

## Release 镜像

GitHub `release-images` 工作流仅在 **Release published** 时执行，正式版本和预发布版本都构建。它检出该 Release 的 tag，在原生 AMD64、ARM64 runner 分别构建，再发布包含 `linux/amd64` / `linux/arm64` 的 GHCR manifest：

- `ghcr.io/zhubby/artifactory-control-plane:<release-tag>`
- `ghcr.io/zhubby/artifactory-web:<release-tag>`
- `ghcr.io/zhubby/artifactory-agent-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-agent-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-planner-runtime:<release-tag>`
- `ghcr.io/zhubby/artifactory-source-service:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-operator:<release-tag>`
- `ghcr.io/zhubby/artifactory-image-worker:<release-tag>`
- `ghcr.io/zhubby/artifactory-gateway:<release-tag>`

标签包含原始 Release tag、完整 `sha-<commit>`；语义版本额外提供不带 `v` 的版本标签。只有正式 Release 更新 `latest`。工作流使用 `GITHUB_TOKEN` 的 `packages:write`，不需要另存 PAT；它不会创建 Release。发布前先让对应提交的 `platform-contracts` 检查通过。首次 GHCR 包如果为私有，需要设置包可见性，或通过 `imagePullSecrets` 提供有读取权限的凭据。

## 首次准备凭据与 PKI

需要 Helm 3、kubectl、Python 3、OpenSSL、支持 ReadWriteOnce 的默认 StorageClass。先确认 kube context 与目标命名空间。以下命令中的域名、镜像版本、Ingress 类和 TLS Secret 均需替换为实际配置。

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
    tag: v0.1.0
  web:
    tag: v0.1.0
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

示例版本必须已发布；Chart 不会自动创建域名、Ingress Controller 或 TLS Secret。公开控制台要求 HTTPS。内建 S3 默认使用固定私有 ClusterIP HTTP，Chart 为该地址显式启用 `ARTIFACTORY_S3_ALLOW_HTTP`。外部 `s3.endpoint` 默认要求 HTTPS；私有 HTTP 端点须显式设置 `s3.allowHttp=true`，不会放宽公开入口。`s3.publicEndpoint` 已废弃且被忽略，不再生成客户端 S3 链接。Chart 不创建 S3 Ingress，S3 Service 固定为 ClusterIP。组织内部 S3 CA 需加入 Control 镜像内的系统信任库，不能只在宿主安装。

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

升级使用同一命名空间、release 名、Secret 和 PVC，显式指定新镜像 tag。未分离 Source 时保持 control-plane 单副本；设置多个副本前按[控制平面多副本](control-ha.md)启用 `localSource.enabled` 并核算数据库连接预算。升级前排空 Agent，协调备份数据库、对象存储、control-plane 数据、主密钥与 CA。Secret 更新后容器环境变量不会自动刷新，需要安排相关 Deployment 重启；数据库用户密码也不会因更改初始化环境变量而自动更新。首次隔离试验可使用[受限三副本 profile](versioning.md#隔离的-kubernetes-试部署)，不要将内网 HTTP profile 当作生产 TLS 部署。

应用 PVC 带有 `helm.sh/resource-policy: keep`，`helm uninstall` 会保留它们；外部 Secret 同样保留。**删除命名空间仍会删除这些资源**。不要将卸载、删除命名空间或重建 kind 集群用作升级流程。

## 从原网关升级

升级前排空构建并备份数据库、私有对象、主密钥与设备 CA。已有实例不得重新运行初始化脚本并覆盖原 Secret。只补充新的网关令牌和独立仓库签名身份。

```sh
umask 077
mkdir .deploy/gateway-upgrade
python3 -c 'import secrets; from pathlib import Path; Path(".deploy/gateway-upgrade/token").write_text(secrets.token_hex(32))'
kubectl --context YOUR_CONTEXT -n artifactory create secret generic artifactory-gateway \
  --from-file=token=.deploy/gateway-upgrade/token
openssl req -x509 -newkey rsa:3072 -nodes -sha256 -days 365 \
  -subj '/CN=artifactory Registry Token Issuer' \
  -keyout .deploy/gateway-upgrade/registry.key -out .deploy/gateway-upgrade/registry.crt
kubectl --context YOUR_CONTEXT -n artifactory create secret generic artifactory-registry-signing \
  --from-file=tls.key=.deploy/gateway-upgrade/registry.key \
  --from-file=tls.crt=.deploy/gateway-upgrade/registry.crt
kubectl --context YOUR_CONTEXT -n artifactory create secret generic artifactory-registry-signing-public \
  --from-file=tls.crt=.deploy/gateway-upgrade/registry.crt
```

示例使用默认 release 前缀。已有 Secret 时停止执行并核对原值，禁止直接替换。新签名身份不使用设备 CA。将新增文件纳入离线加密备份。

内部 Gateway 默认使用既有设备 PKI 的服务器证书。旧证书须用原 CA 续签，保留全部既有 SAN，并加入 `<release>-gateway-internal` 和 `<release>-gateway-internal.<namespace>.svc`。不更换 CA 或业务主密钥。若 Runtime/Source 已使用独立服务 CA，配置 `gateway.internalTlsSecret`，引用包含 `ca.pem/server.pem/server.key` 的 Secret；服务器证书由该服务 CA 签发，并覆盖相同内部 SAN。服务器私钥与 CA 私钥独立，Gateway 不挂载 CA 私钥。Runtime/Source 只读信任该服务 CA，地址为 `https://<release>-gateway-internal:8445`。内部 Service 不对公网暴露，公开入口继续拒绝 `/internal/*`。

从分离网关升级时，移除 `gateway.dedicatedData`、`dataReplicas`、`deviceReplicas` 和 `internalReplicas`，统一设置 `gateway.replicas`。先部署新统一网关并验证四个监听，再把 Web、设备、内部 Service 切换到统一选择器。滚动升级保留原 Service 名称。内部 TLS 使用独立挂载，公开端口不接受内部路由。NetworkPolicy 对内部 8445 只允许同命名空间 Pod；公开与设备来源通过 `gateway.networkPolicy.publicFrom/deviceFrom` 配置。

设备控制地址指向 `<release>-grpc:8443`，遥测地址指向 `https://<release>-grpc:8444`。集群外设备使用对应的可达域名、NodePort 或持久隧道，并验证后端服务器证书 SAN。已有注册设备须更新本机配置的 `endpoint`、`telemetry_endpoint`；仅修改注册回复不会更新已有设备。临时隧道直接选择正式 Service，不再选择旧 Pod。先验证设备重连与最新遥测，再移除旧 `emissary-gateway` StatefulSet、Service 和转发任务。

`<release>-telemetry-grpc` 固定为私有 ClusterIP。旧 `telemetry.service.type=NodePort/LoadBalancer` 和非空 `telemetry.service.nodePort` 会拒绝渲染。先将旧后端 Service 改为 ClusterIP，释放旧 NodePort，再用 `grpc.service.telemetryNodePort` 将端口分配给统一网关。

将 `s3.endpoint` 改为 Control/Gateway 可访问的私有地址。设备制品字节通过公开网关的 `/artifacts/object`，不保留旧设备 S3 转发。升级后验证网关下载、上传、Range/HEAD 和设备 mTLS，再移除旧 S3 Ingress、NodePort、LoadBalancer 及公网 DNS。关闭公开 Bucket 策略。旧预签名链接按原到期时间失效；不要将它们重新写入新页面。

## Agent worker

默认启用 `agentWorker.enabled`，使用独立身份 PVC、只读 CA 和单副本 Recreate。`images.agent-worker` 需与 control-plane/web 使用同一候选版本；配置 `agentWorker.serverName` 时必须匹配服务器证书 SAN。初始 Pod 等待注册，按 [AI 构建指南](ai-builds.md) 从标准输入注册，并在管理界面配置真实模型。`sourceCache.maxBytes` 控制平台缓存配额，kind 默认 512 MiB。已有 PVC/Secret 不得重新初始化覆盖。
