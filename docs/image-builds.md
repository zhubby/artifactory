<!--
[INPUT]: 独立镜像 Operator、池代理、监督型 BuildKit 与私有仓库。
[OUTPUT]: 定义最低 Kubernetes 1.28 的镜像生命周期、出口和交付边界。
[POS]: 镜像构建与统一网关的部署入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 集群镜像构建、内建仓库与网关

Image Operator 在 Kubernetes 中启动 BuildKit。平台固定源码清单后，为每个平台创建独立 Pod。所有平台镜像完成内容校验后，Publisher 生成 OCI Index，再写入最终标签。构建不占用 Emissary 设备 attempt。

内建仓库使用 Distribution 3.0.0。仓库数据写入独立、私有的 S3 Bucket。外部仓库是发布目的地。Pingora 反向代理内建仓库，不提供上游镜像缓存。

## 服务边界

| 服务 | 入口与职责 | 持久状态 |
|---|---|---|
| Control | 固定源码、执行授权、任务派发、仓库令牌 | 业务 PostgreSQL |
| Agent Operator | AgentPool、AgentRun 与临时 SourceTask | 独立 Lease，任务 CRD |
| Image Operator | BuilderPool、ImageBuild 与 BuildKit Pod | 独立 Lease，镜像 CRD |
| Distribution | OCI Registry API、上传与内容读取 | 独立私有 S3 Bucket |
| Pingora HTTP | 控制入口、仓库反向代理、制品票据流转 | 无本地制品副本 |
| Pingora TCP | 在建连时选择 Control，透传 TLS 字节 | 原有设备 mTLS 会话 |

主 Chart 只创建一个 Gateway Deployment，默认两个副本。每个 Pod 同时提供公开 HTTP、设备控制、设备遥测和内部 HTTPS 四个监听端口。公开 API、仓库和制品共享 HTTP 连接预算；内部请求、设备控制和遥测各有独立预算。三个 Service 分别选择同一组 Pod 的对应端口。

| Service | Pod 端口 | 访问边界 |
|---|---|---|
| `<prefix>-web` | 8080 | 控制台、API、仓库与制品；拒绝 `/internal/*` |
| `<prefix>-grpc` | 8443、8444 | 控制与遥测的端到端设备 mTLS 透传 |
| `<prefix>-gateway-internal` | 8445 | 仅内部 HTTPS API 与健康检查 |

Gateway NetworkPolicy 将 8445 限制为 Control、planning.namespace 和 imageBuilds.namespace 中的 Pod。`gateway.networkPolicy.publicFrom` 和 `deviceFrom` 接受 Kubernetes 网络来源列表；空列表允许任意来源。设置这些列表时同步检查其他 NetworkPolicy，相加规则仍能放行相同流量。设备长连接在建立时选择后端，网关不重放设备消息。新增 Control 副本接纳新连接，已有连接通过排空和客户端重连迁移。

Runtime 与源码任务使用独立 ClusterIP `https://<prefix>-gateway-internal:8445`。该入口只接受内部 API 和健康检查，强制 TLS，并由 Control 核验服务令牌或任务身份。默认服务器身份来自设备 PKI；已有独立服务 CA 使用 `gateway.internalTlsSecret` 提供受 Runtime 与源码任务信任的证书。不要将内部 Service 添加到公开 Ingress。

公开和内部 `/healthz` 汇总全部必要监听。网关向本机固定监听端口发起启动探测，等待自己的接受回调；其他进程占用端口不能使探测成功。四个监听全部完成端口绑定、TLS 构建并接受连接后，健康检查返回 200。在此之前返回 503。启动探测期限为三十五秒，包含 Pingora 的端口占用重试。监听任务崩溃、提前结束或探测超时会使进程非零退出。收到正常关闭信号后撤销就绪状态。

HTTP 上游连接回池后最多空闲三十秒。该时限不终止活动中的流式上传或下载。网关不自动重放失败的写请求。错误日志只记录类型、来源、监听角色和连接复用状态，不记录请求头、查询串或对象地址。

原生制品的上传、下载、Range 和 HEAD 均通过 `/artifacts/object`。Control 生成绑定对象、方法和期限的加密票据。Gateway 通过内部认证接口换取私有 S3 签名地址，并流式转发字节。浏览器、Emissary 和外部客户端不取得 S3 地址或凭据。网关拒绝公开访问 `/internal/*`。

## 安装顺序

部署前准备 Kubernetes 1.28 及以上版本、StorageClass、业务 PostgreSQL、制品 S3、仓库 S3 和可达的 HTTPS 网关。主 Chart 的内建制品 S3 使用私有 ClusterIP HTTP；外部私有 HTTP 端点须显式设置 `s3.allowHttp=true`。构建使用两个普通应用容器，无原生 sidecar 或版本分支。任务使用 rootless user namespace、Unconfined seccomp/AppArmor，以及 `--oci-worker-no-process-sandbox`。节点和准入策略必须允许这些设置。任务只运行可信项目源码；rootless 不能替代租户安全沙箱。

1. 按 [Helm 指南](helm.md) 准备主部署 Secret。设备 CA 与仓库 RSA 签名身份独立。仓库签名私钥只挂载到 Control；Distribution 只挂载公开证书。
2. 预先创建仓库 Bucket。使用独立 S3 身份，仅授权该 Bucket。Distribution 3 写对象时设置私有 ACL，身份须具备 `s3:PutObjectAcl`。SeaweedFS 除读写、列举与标签权限外，还须授予 `WriteAcp:<bucket>`。不要改用全局 `Admin` 权限。不要复用原生制品 Bucket。保持 Bucket 私有，并限制网络入口。
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

同一命名空间安装一套 Image Operator。Control 的 `imageBuilds.namespace` 必须指向该命名空间。Chart 在目标命名空间为 Control 创建任务声明和 Secret 权限；目标命名空间须预先存在。Image Operator 不读取 Secret，源码 init container 使用专属投影 token，主容器不取得该 token。

Control 未设置 `ARTIFACTORY_IMAGE_NAMESPACE` 时拒绝镜像计划、重试和发布，原生目标继续执行。设置该变量后，缺失配置或无效 RSA 签名身份会阻止 Control 启动。Kubernetes 客户端初始化失败时，调度循环持续重试。

主 Chart 配置：

```yaml
gateway:
  registryUrl: http://artifactory-registry:5000
  replicas: 2
  httpConnections: 1024
  internalConnections: 1024
  deviceConnections: 4096
  telemetryConnections: 4096
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

BuilderPool 是管理员配置。它固定工作镜像、BuildKit 镜像、节点选择器、资源和并发预算。`maxActive` 限制池内并发 Pod；`maxPerProject` 限制单项目并发 Pod。每个平台和 Publisher 分别占用一个 Pod 配额。

```yaml
builderPool:
  name: default
  workerImage: ghcr.io/zhubby/artifactory-image-worker:0.2.0
  buildkitImage: ghcr.io/zhubby/artifactory-buildkit:0.2.0
  sourcePreparerImage: ghcr.io/zhubby/artifactory-source-preparer:0.2.0
  controlUrl: https://artifactory-gateway-internal:8445
  maxActive: 8
  maxPerProject: 4
  resources:
    source: {cpu: 250m, memory: 512Mi, ephemeralStorage: 10Gi}
    runner: {cpu: 250m, memory: 256Mi, ephemeralStorage: 1Gi}
    buildkit: {cpu: '1', memory: 2Gi, ephemeralStorage: 20Gi}
    publisher: {cpu: 500m, memory: 512Mi, ephemeralStorage: 20Gi}
  nodeSelector:
    artifactory.dev/image-builds: 'true'
  caSecret: build-service-ca
  buildkitConfigMap: buildkit-config
```

需要通过指定出口访问 GitHub、ECR 或其他外部仓库时，在 `builderPool.proxy` 配置 `httpProxy`、`httpsProxy` 和显式 `noProxy`。直接创建 CR 时，对应字段为 `BuilderPool.spec.proxy`。配置格式与[Agent 池代理](operator-operations.md#池级出口代理)相同，不提供任意 Pod 环境入口。

```yaml
builderPool:
  proxy:
    httpProxy: http://artifactory-egress:18443
    httpsProxy: http://artifactory-egress:18443
    noProxy: localhost,127.0.0.1,.svc,.cluster.local,artifactory-gateway-internal,artifactory-registry,kubernetes.default.svc,10.233.0.1
```

代理仅接受 HTTP(S) origin，拒绝空白和控制字符。管理员须在 `noProxy` 覆盖内部 Control、Registry、Registry 认证 realm、Kubernetes API DNS 与实际 API Service IP；短主机名和认证域名逐项列出。Operator 创建 Pod 前校验池中已知的内部地址。它不强制外部发布目标直连。部署环境的 NetworkPolicy 须放行代理的实际 TCP 端口。

源码 init、Runner、BuildKit daemon 和 Publisher 同时接收大小写代理环境。Runner 仅在池已配置代理时，将非空标准代理变量传为 Dockerfile frontend 的 `build-arg`。这样基础镜像拉取、令牌请求、Dockerfile `RUN` 和镜像发布均能使用出口。已配置的池代理优先于同名配方参数；池未配置或对应代理值缺省时，保留配方参数。

标准代理 build args 默认不进入镜像历史或构建缓存键。Dockerfile 不声明同名 `ARG` 或 `ENV`，以保持该默认行为。代理配置不进入执行计划，构建输出和 daemon 诊断会清除代理 URL、原始及 URL 编码密码和 Basic 凭据。未配置代理时不增加代理参数；不需要包装正式工作镜像。

每个平台 Pod 先执行普通源码 init，再并发启动 Runner 和 BuildKit 普通容器。源码位于有大小限额的 emptyDir，Runner 只读挂载。Git 凭据与投影 token 只挂载到 init；推送凭据 Secret 只挂载到 Runner。BuildKit 不挂载这些凭据、源码卷或 Runner 私有工作卷。Publisher 不拉源码，不启动 BuildKit。

`resources` 按角色设置 CPU、内存与临时存储，requests 等于 limits。`source` 是源码 init，`runner` 是 buildctl 与校验进程，`buildkit` 是实际编译进程树，`publisher` 是独立发布 Pod。每种角色覆盖时须同时提供三个字段。池级 `cpu`、`memory`、`ephemeralStorage` 字段已删除。

默认平台 Pod 的普通容器合计申请 1.25 CPU、2304 MiB 内存与 31 GiB 临时存储。Kubernetes 按普通容器合计与最大 init 请求的较大值调度。源码卷在 init 退出后仍占磁盘，因此 Runner 的磁盘请求包含 `source` 与 `runner` 两项预算；CPU 和内存不叠加源码预算。Publisher 仅申请自己的预算。

`source.ephemeralStorage` 限制 workspace，`runner` 或 `publisher` 的该字段限制 work，`buildkit` 的该字段限制 buildkit-state。存储值使用正整数字节或正整数加 SI、二进制后缀，例如 `512Mi`、`20Gi`、`2G`。不接受小数、毫字节和超出 int64 的值。各卷总使用量、容器日志和可写层仍受 Pod 临时存储预算与节点磁盘约束，接近工作卷总上限的任务须额外留出余量。

监督入口作为 BuildKit 容器 PID 1，启动 rootlesskit/buildkitd，并校验容器 PID namespace 与 `/proc`。Runner 等待 daemon 就绪，再调用 buildctl。它在结果与源码校验后请求停止，并等待进程树回收确认。EOF、心跳到期、总期限或信号使监督程序停止 BuildKit。Pod 保留 45 秒终止窗口，本地收尾期限为 15 秒。

不启用共享 PID namespace，也不额外嵌套 rootlesskit PID namespace。该路径适用于默认关闭 `ProcMountType` 的 1.28。Dockerfile `RUN` 能看到 BuildKit 容器内的监督进程；它不能访问另一容器的 Runner PID 或私有卷。可信源码约束仍然适用。

节点必须支持未特权 user namespace，且 `user.max_user_namespaces` 非零。先在实际构建节点验证 UID/GID 映射、容器 PID/procfs 和真实 Dockerfile `RUN`，再添加构建节点标签。旧内核须单独验证 snapshotter，不能仅依据 Kubernetes 版本判断可用性。不得以 privileged、hostPID 或宿主目录挂载绕过失败。

Control 校验完整清单和固定提交后，在 `build.json` 的源码部分只写入 `source_manifest_id` 和 `source_digest`。完整清单由 init 直接领取并写入临时卷，Runner 核对 ID、摘要和清单内容，再在构建前后校验工作树。任务材料信封上限保持 900 KiB，合法 4 MiB 清单不写入 Kubernetes Secret。旧 `source` 字段被拒绝。

BuildKit 的内容存储与快照仅存于本次 Pod 的 emptyDir。不导入或导出 Registry、S3、inline 或本地共享构建缓存，也不复用跨任务 daemon。`RUN --mount=type=cache` 只在本次 BuildKit 生命周期内有效。正式镜像是允许保留的制品，其 Dockerfile 明确复制的源码随镜像保存。

`caSecret` 必须包含 `ca.crt`。任务信任卷只投影该键，挂载到源码 init、Runner、Publisher 和 BuildKit 的 `/run/ca`；同一 Secret 的证书私钥不会进入任务。源码 init 用它验证 Git；Runner 和 Publisher 用它验证 Control 与 Registry HTTPS。Buildctl 通过 `--registry-auth-tlscontext` 为内建仓库的独立令牌请求追加该 CA，保留系统根证书。BuildKit 读取的 `buildkitd.toml` 必须单独声明私有仓库 CA，例如：

```toml
[registry."build.example.com"]
  ca = ["/run/ca/ca.crt"]
```

将文件以 `buildkitd.toml` 键放入 `buildkitConfigMap`。公开 CA 无需覆盖系统根证书。HTTP 仓库只用于显式开发环境，须同时配置平台开发开关和 BuildKit 对该域名的 `http = true`。

默认按 `kubernetes.io/arch` 选择原生 amd64/arm64 Ready 节点。缺少架构节点时任务等待。只有目标 `allow_emulation=true`，且管理员配置 `emulationPlatforms` 与非空 `emulationNodeSelector` 时，才调度模拟构建。QEMU/binfmt 由管理员预装，Operator 不部署特权安装器。生产多架构交付优先准备两类原生节点。

## 目标与凭据

AI 规划支持 `executor: buildkit`。AI 生成计划中的镜像目标使用以下结构；构建步骤由 Dockerfile 定义，不填写原生 steps、artifacts 或设备 placements：

```yaml
version: 1
targets:
  image:
    os: linux
    arch: amd64
    executor: buildkit
    application_platform: null
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

源码地址、提交和清单摘要在入队前固定。源码 init container 每次从 Git/LFS 上游拉取固定清单，Runner 只读核验后为单平台推送候选镜像。Control 读取镜像 manifest/config，校验 digest、OS 和架构。缺少任一请求平台时不发布最终 Index。

`context` 和 `dockerfile` 均相对源码根目录，不能逃逸该目录。Dockerfile 路径不相对 context。仓库中的 `examples/container-image.yaml` 和 `examples/image/Dockerfile` 提供双原生架构验收模板。

项目资源页可创建镜像仓库凭据和构建 Secret。凭据加密存储，API 只回传元数据。镜像目标的 `pull_credentials` 是同项目 Registry 凭据 UUID 数组；`secrets` 是 BuildKit Secret 名称到同项目凭据 UUID 的映射。Secret 通过 BuildKit secret mount 使用，不写入 build args。Runner 取得内建候选镜像的任务推送权限。外部 Registry push 凭据仅进入 Publisher。

构建输出页显示平台结果、Index digest、构建日志与发布状态。内建仓库路径由租户、项目和镜像名称组成。使用平台 API Token 登录公开网关域名，并按控制台提供的完整路径拉取。令牌的项目权限仍适用，跨项目仓库访问被拒绝。

项目制品页列出已验证镜像、完整摘要引用和来源构建。Release 固定引用镜像摘要；标签后续变更不改变已发布 Release。

外部发布从已验证的内建仓库 digest 复制完整 OCI 图，再提交目标标签。每次发布固定不含用户名和密码的仓库来源地址，历史引用在凭据停用或轮换后保持完整。发布需要项目 release 权限和该项目的 Registry 凭据。失败后在输出页重试发布；它创建独立执行，不重新构建 Dockerfile。内部发布失败且平台结果完整时，同样可重试发布。多个标签按顺序写入；OCI Registry 不提供跨标签事务，失败时部分标签已经写入，重试最终收敛到同一 digest。

## 取消、清理与备份

排空阻止新建构建和新发布。已接受的排队及运行镜像任务继续执行，生命周期操作等待它们完成。LFS 能力只注入原生目标，持久化计划在注入后重新校验。

取消或截止期限使执行令牌和源码授权失效。超时记为失败；所有平台结果完整时记为发布失败，允许重试发布。显式取消与旧计划执行禁止重试。已签发的仓库 JWT 最长保留 60 秒权限。Image Operator 按 Pod UID 删除，避免删除同名替换资源。终态任务 Secret 被清理。每个 Pod 直接归属 ImageBuild，使用 `restartPolicy: Never`，并带停止确认 finalizer。Operator 将全部容器停止的证据写入 `status.podOwnership.pods[].stopped` 后，才移除 Pod finalizer并等待对象消失。节点失联或 Pod 无故消失且未保存停止证据时，保留 `CleanupPending` 和容量，不在原 execution 中重建 Pod。ImageBuild 作为执行审计记录保留；不要将 CRD 终态当作 Registry 内容回收信号。

日志存入原生制品 Bucket，执行与发布元数据存入业务数据库。最终镜像 manifest 和 blob 存入独立仓库 Bucket。项目删除等待至少五分钟的在途日志写入宽限期，再清理已登记对象及租户、项目作用域的镜像日志前缀。清扫包含拒绝回执留下的孤立对象，并以持久化游标从存储故障中恢复。项目删除不自动删除仓库 blob。当前关闭在线 Registry 删除与自动 GC；按组织保留策略安排停写备份、离线 GC 和容量告警。不能根据业务记录删除数推断 S3 空间已释放。

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
