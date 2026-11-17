# 集群镜像构建、内建仓库与网关

Image Operator 在 Kubernetes 中启动 BuildKit。平台固定源码快照后，为每个平台创建独立 Job。所有平台镜像完成内容校验后，Publisher 生成 OCI Index，再写入最终标签。构建不占用 Emissary 设备 attempt。

内建仓库使用 Distribution 3.0.0。仓库数据写入独立、私有的 S3 Bucket。外部仓库是发布目的地。Pingora 反向代理内建仓库，不提供上游镜像缓存。

## 服务边界

| 服务 | 入口与职责 | 持久状态 |
|---|---|---|
| Control | 固定源码、执行授权、任务派发、仓库令牌 | 业务 PostgreSQL |
| Agent Operator | AgentPool、AgentRun 与 Source 工作负载 | 独立 Lease，既有 CRD |
| Image Operator | BuilderPool、ImageBuild 与 BuildKit Job | 独立 Lease，镜像 CRD |
| Distribution | OCI Registry API、上传与内容读取 | 独立私有 S3 Bucket |
| Pingora HTTP | 控制入口、仓库反向代理、制品票据流转 | 无本地制品副本 |
| Pingora TCP | 在建连时选择 Control，透传 TLS 字节 | 原有设备 mTLS 会话 |

设备入口使用独立连接预算。`gateway.dedicatedData=true` 为仓库和制品部署独立副本与预算，并通过 Ingress 路径分流。控制入口此时拒绝数据路径；关闭该选项时，公开控制与数据共享 HTTP 预算。设备长连接在建立时选择后端；网关不重放已经发送的设备消息。新增 Control 副本只接纳新连接，已有连接通过排空和客户端重连迁移。

Runtime/Source 使用独立 ClusterIP `https://<release>-gateway-internal:8445`。该入口只接受内部 API 和健康检查，强制 TLS，并由 Control 校验既有服务令牌。默认服务器身份来自设备 PKI；已有独立服务 CA 使用 `gateway.internalTlsSecret` 提供受 Runtime/Source 信任的证书。不要将内部 Service 添加到公开 Ingress。

原生制品的上传、下载、Range 和 HEAD 均通过 `/artifacts/object`。Control 生成绑定对象、方法和期限的加密票据。Gateway 通过内部认证接口换取私有 S3 签名地址，并流式转发字节。浏览器、Emissary 和外部客户端不取得 S3 地址或凭据。网关拒绝公开访问 `/internal/*`。

## 安装顺序

部署前准备 Kubernetes、StorageClass、业务 PostgreSQL、制品 S3、仓库 S3 和可达的 HTTPS 网关。主 Chart 的内建制品 S3 使用私有 ClusterIP HTTP；外部私有 HTTP 端点须显式设置 `s3.allowHttp=true`。仓库的 Kubernetes 测试配置使用 1.37。BuildKit 使用原生 sidecar、rootless user namespace、Unconfined seccomp/AppArmor，以及 `--oci-worker-no-process-sandbox`。节点和准入策略必须允许这些设置。任务只运行可信项目源码；rootless 不能替代租户安全沙箱。

1. 按 [Helm 指南](helm.md) 准备主部署 Secret。设备 CA 与仓库 RSA 签名身份独立。仓库签名私钥只挂载到 Control；Distribution 只挂载公开证书。
2. 预先创建仓库 Bucket。使用独立 S3 身份，仅授权该 Bucket。不要复用原生制品 Bucket。保持 Bucket 私有，并限制网络入口。
3. 创建 `artifactory-registry-storage` Secret，键为 `access-key`、`secret-key`、`http-secret`。所有 Distribution 副本共享同一个 `http-secret`，升级时保留。
4. 应用镜像 CRD，再安装独立 Image Operator Chart。Helm 只在首次安装时创建 CRD，升级时须显式审查和应用 CRD。
5. 安装 Registry Chart。配置认证 realm、service、issuer 和公开证书，随后启用主 Chart 的 `imageBuilds.enabled`。

```sh
kubectl --context YOUR_CONTEXT apply -f operators/image-operator/config/crd/resources.yaml
helm upgrade --install images charts/artifactory-image-operator \
  --kube-context YOUR_CONTEXT -n artifactory -f image-operator-values.yaml
helm upgrade --install registry charts/artifactory-registry \
  --kube-context YOUR_CONTEXT -n artifactory -f registry-values.yaml
helm upgrade --install artifactory charts/artifactory \
  --kube-context YOUR_CONTEXT -n artifactory -f production-values.yaml
```

同一命名空间安装一套 Image Operator。Control 的 `imageBuilds.namespace` 必须指向该命名空间。Chart 在目标命名空间为 Control 创建任务声明和 Secret 权限；目标命名空间须预先存在。Image Operator 不读取 Secret，工作 Pod 不取得 Kubernetes API 令牌。

Control 未设置 `ARTIFACTORY_IMAGE_NAMESPACE` 时拒绝镜像计划、重试和发布，原生目标继续执行。设置该变量后，缺失配置或无效 RSA 签名身份会阻止 Control 启动。Kubernetes 客户端初始化失败时，调度循环持续重试。

主 Chart 配置：

```yaml
gateway:
  registryUrl: http://artifactory-registry:5000
  dedicatedData: true
  replicas: 2
  dataReplicas: 2
  deviceReplicas: 2
  httpConnections: 1024
  deviceConnections: 4096
imageBuilds:
  enabled: true
  namespace: artifactory
  registryUrl: https://build.example.com
  controlUrl: https://build.example.com
  registryService: artifactory-registry
  registryIssuer: artifactory
s3:
  endpoint: https://private-artifacts.internal.example.com
```

Registry Chart 配置：

```yaml
replicas: 2
service: artifactory-registry
s3:
  bucket: artifactory-registry
  endpoint: https://private-s3.internal.example.com
  region: us-east-1
  secure: true
  forcePathStyle: true
  secretName: artifactory-registry-storage
auth:
  realm: https://build.example.com/api/v1/registry/token
  service: artifactory-registry
  issuer: artifactory
  certificateSecret: artifactory-registry-signing-public
```

Distribution 的 `relativeurls=true` 保持上传 Location 在公开仓库域名内。`redirect.disable=true` 保证 blob 字节经过 Gateway。不要开启 S3 重定向或 Registry proxy 模式。Ingress 须关闭响应缓冲，允许实际镜像层大小，并配置足够的上传和流式响应超时。

## 构建节点和私有 CA

BuilderPool 是管理员配置。它固定工作镜像、BuildKit 镜像、节点选择器、资源和并发预算。`maxActive` 限制池内并发 Job；`maxPerProject` 限制单项目并发 Job。每个平台和 Publisher 分别占用一个 Job 配额。

```yaml
builderPool:
  name: default
  workerImage: ghcr.io/zhubby/artifactory-image-worker:0.2.0
  buildkitImage: moby/buildkit:v0.26.2-rootless
  maxActive: 8
  maxPerProject: 4
  nodeSelector:
    artifactory.dev/image-builds: 'true'
  caSecret: build-service-ca
  buildkitConfigMap: buildkit-config
```

`caSecret` 包含 `ca.crt`，同时挂载到 Runner、Publisher 和 BuildKit 的 `/run/ca`。Runner 将它用于源码、Control 和 Registry HTTPS。Buildctl 通过 `--registry-auth-tlscontext` 为内建仓库的独立令牌请求追加该 CA，保留系统根证书。BuildKit 读取的 `buildkitd.toml` 必须单独声明私有仓库 CA，例如：

```toml
[registry."build.example.com"]
  ca = ["/run/ca/ca.crt"]
```

将文件以 `buildkitd.toml` 键放入 `buildkitConfigMap`。公开 CA 无需覆盖系统根证书。HTTP 仓库只用于显式开发环境，须同时配置平台开发开关和 BuildKit 对该域名的 `http = true`。

默认按 `kubernetes.io/arch` 选择原生 amd64/arm64 Ready 节点。缺少架构节点时任务等待。只有目标 `allow_emulation=true`，且管理员配置 `emulationPlatforms` 与非空 `emulationNodeSelector` 时，才调度模拟构建。QEMU/binfmt 由管理员预装，Operator 不部署特权安装器。生产多架构交付优先准备两类原生节点。

## 目标与凭据

AI 规划支持 `executor: buildkit`。兼容执行配方中的镜像目标使用以下结构；构建步骤由 Dockerfile 定义，不填写原生 steps、artifacts 或设备 placements：

```yaml
version: 1
targets:
  image:
    os: linux
    arch: amd64
    executor: buildkit
    image:
      name: backend
      context: .
      dockerfile: Dockerfile
      platforms: [linux/amd64, linux/arm64]
      pool: default
      allow_emulation: false
      tags: [candidate]
      build_args:
        MODE: production
```

源码地址、提交和快照摘要在入队前固定。Runner 校验快照身份，并为单平台推送候选镜像和项目缓存。Control 读取镜像 manifest/config，校验 digest、OS 和架构。缺少任一请求平台时不发布最终 Index。

`context` 和 `dockerfile` 均相对源码根目录，不能逃逸该目录。Dockerfile 路径不相对 context。仓库中的 `examples/container-image.yaml` 和 `examples/image/Dockerfile` 提供双原生架构验收模板。

项目资源页可创建镜像仓库凭据和构建 Secret。凭据加密存储，API 只回传元数据。镜像目标的 `pull_credentials` 是同项目 Registry 凭据 UUID 数组；`secrets` 是 BuildKit Secret 名称到同项目凭据 UUID 的映射。Secret 通过 BuildKit secret mount 使用，不写入 build args。Registry push 凭据仅进入 Publisher。

构建输出页显示平台结果、Index digest、构建日志与发布状态。内建仓库路径由租户、项目和镜像名称组成。使用平台 API Token 登录公开网关域名，并按控制台提供的完整路径拉取。令牌的项目权限仍适用，跨项目仓库访问被拒绝。

项目制品页列出已验证镜像、完整摘要引用和来源构建。Release 固定引用镜像摘要；标签后续变更不改变已发布 Release。

外部发布从已验证的内建仓库 digest 复制完整 OCI 图，再提交目标标签。每次发布固定不含用户名和密码的仓库来源地址，历史引用在凭据停用或轮换后保持完整。发布需要项目 release 权限和该项目的 Registry 凭据。失败后在输出页重试发布；它创建独立执行，不重新构建 Dockerfile。内部发布失败且平台结果完整时，同样可重试发布。多个标签按顺序写入；OCI Registry 不提供跨标签事务，失败时部分标签已经写入，重试最终收敛到同一 digest。

## 取消、清理与备份

排空阻止新建构建和新发布。已接受的排队及运行镜像任务继续执行，生命周期操作等待它们完成。LFS 能力只注入原生目标，持久化计划在注入后重新校验。

取消或截止期限使执行令牌和源码授权失效。超时记为失败；所有平台结果完整时记为发布失败，允许重试发布。显式取消与旧计划执行禁止重试。已签发的仓库 JWT 最长保留 60 秒权限。Image Operator 按 owner UID 删除 Job，避免删除同名替换资源。终态任务 Secret 被清理，Job 通过 TTL 回收。ImageBuild 作为执行审计记录保留；不要将 CRD 终态当作 Registry 内容回收信号。

日志存入原生制品 Bucket，执行与发布元数据存入业务数据库。镜像 manifest、blob 和缓存存入独立仓库 Bucket。项目删除等待至少五分钟的在途日志写入宽限期，再清理已登记对象及租户、项目作用域的镜像日志前缀。清扫包含拒绝回执留下的孤立对象，并以持久化游标从存储故障中恢复。项目删除不自动删除仓库 blob。当前关闭在线 Registry 删除与自动 GC；按组织保留策略安排停写备份、离线 GC 和容量告警。不能根据业务记录删除数推断 S3 空间已释放。

协调备份业务数据库、主密钥、两个 Bucket、Registry `http-secret` 和仓库签名身份。签名证书到期前轮换：先让 Distribution 信任新旧公开证书，再切换全部 Control 签名私钥；等待现有 60 秒仓库令牌过期后移除旧证书。设备 CA 不参与此次轮换。Gateway token 轮换须协调所有 Gateway 与 Control 副本，避免制品票据解析暂时失败。

## 验证入口

```sh
cargo test --locked -p artifactory-image-protocol -p artifactory-image-registry -p artifactory-image-operator
cargo build --locked -p artifactory-gateway
python3 tests/images/gateway.py
helm lint charts/artifactory-image-operator --strict
helm lint charts/artifactory-registry --strict
```

本机网关验收覆盖 8 MiB 上传、Range/HEAD、方法票据、固定上游、仓库分段上传、内部 TLS 入口和真实客户端证书透传。独立服务 CA 与系统 CA 同时生效。它不证明 Kubernetes rootless、双原生架构构建或外部仓库供应商已验收。部署验收须额外执行双平台 Dockerfile、合成 Index 后按 digest 拉取、外部复制失败重试、跨租户拒绝、运行中取消和控制器切主，并保存实际集群版本与镜像 digest。

[PROTOCOL]: Update this header on change, then check AGENTS.md
