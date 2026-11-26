<!--
[INPUT]: 当前服务拓扑、持久身份和独立部署脚本。
[OUTPUT]: 定义安装、备份、节点注册和故障处理边界，并关联设备终端契约。
[POS]: 运维入口；区分初始化基线与基线内兼容升级。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 部署与运维

## 部署边界

Control 为支持多副本的 Linux 服务，内嵌规划协调器。后端统一使用 TimescaleDB 2.32+ / PostgreSQL 17，关系表保存队列和业务状态，独立 Rust Telemetry 服务保存时序数据。多副本设备调度使用 PostgreSQL Outbox 与节点行锁，部署、容量与回退见 [Control 多副本](control-ha.md)。Kubernetes 规划和源码任务的部署见 [Operator 运维](operator-operations.md)。宿主执行只接受内部可信项目。签名节点应与一般构建节点分池、最小授权、限制网络和服务账户权限。

业务数据库初始化基线为 `2026112101`，规划数据库基线为 `2026111901`。业务基线内追加 `2026112201` 镜像失败分类，以及 `2026112301`–`2026112303` 通知收件箱、渠道和事务生产者。程序只对已识别基线应用追加迁移；遇到未知旧身份时拒绝启动，不自动重置。安装脚本不退役旧 Deployment、CRD、Pod、PVC 或持久数据，完整边界见[初始化与清理](planning-protocol.md#初始化与清理)。

应用追加迁移时，先升级 Control 并完成迁移，再升级 Telemetry，最后升级镜像 Worker。Telemetry 以只读连接验证完整业务迁移集合。旧 Control 和旧 Telemetry 二进制在新迁移之后重启时会拒绝未知迁移身份。通知邮件通过独立 SMTP Secret 配置，所有外发订阅默认关闭，见[通知中心](notifications.md)。

生产需可解析的控制台和 Emissary gRPC 域名。Compose 的 Caddy 只负责 80/443 与 ACME，全部应用请求转至 Pingora。Pingora 反向代理控制台、API、内建仓库与制品，S3 没有公开入口。构建 gRPC 使用钉住私有 CA 的 TLS 8443，独立遥测 mTLS 使用同一设备域名的 8444。仅向受信网络开放设备端口；不公开数据库、遥测内部查询、SeaweedFS 管理端口及 filer/master。代理不得缓冲 SSE。

## Linux Compose

Kubernetes 和本机 kind 的 Chart、凭据初始化、TLS、持久卷与升级说明见 [Helm 部署指南](helm.md)。

在专用**非 root** 部署账户下准备：

```sh
ARTIFACTORY_DOMAIN=build.example.com \
ARTIFACTORY_GRPC_HOST=agents.example.com \
sh scripts/init-deploy.sh
docker compose --env-file .deploy/env config --quiet
docker compose --env-file .deploy/env up -d --build
```

新安装的初始账号为 `admin / 123456`。仅在业务库没有用户时创建，重启不覆盖已有密码。该初始密码使用随机盐 Argon2 散列存储。注册和修改密码仍要求 12–256 字节。

`.deploy/env` 包含随机数据库密码、Git 凭据加密主密钥、S3 密钥和初始账号；`.deploy/pki` 包含节点证书签发私钥。均不入 Git，目录权限为 0700，文件为 0600；容器以该部署账户 UID/GID 读取。不要在已有部署上重新初始化或更换主密钥。

Compose 内 S3 禁止匿名访问，只在私有网络使用 `http://s3:8333`。Control 显式设置 `ARTIFACTORY_S3_ALLOW_HTTP=true`，只放行管理员配置的私有存储地址；公开入口仍要求 HTTPS。Emissary 和浏览器只访问网关票据地址。升级既有 `.deploy/env` 时补充至少 32 字符的随机 `ARTIFACTORY_GATEWAY_TOKEN`，保留数据库口令、主密钥和设备 CA；旧 `ARTIFACTORY_S3_DOMAIN` 被忽略。不要重新运行初始化脚本覆盖已有身份。

控制平面 S3 客户端通过 Rustls 加载系统根证书。容器部署需把组织 CA 加入控制平面镜像或容器内的信任库，并确保运行账户可读；宿主安装的证书不会自动传播到容器。证书的签发链和 S3 endpoint 主机名都会校验。

S3 启动脚本把只读挂载、0600 权限的宿主认证配置复制为容器内 `seaweed` 用户专有的 0600 文件，再由镜像入口降权运行；宿主配置的权限和归属保持不变。首次初始化需要镜像原有的 root 启动阶段，长期运行的 SeaweedFS 进程为非 root。修改配置后需重建或重启 S3 容器，不能通过放宽秘密文件权限解决 UID 不同导致的读取失败。

### 用户自助注册

**自助注册默认开启。任何能够访问控制台的人均可创建全局账号，但新账号没有租户或项目权限，需要创建租户或接受邀请。** 业务数据按租户和项目授权隔离，注册不会授予已有空间的读取或管理权限。

不希望开放注册时，在本地控制平面环境中设置 `ARTIFACTORY_REGISTRATION_ENABLED=false` 并重启控制平面；Compose 在 `.deploy/env` 中设置相同值并重新创建 control-plane 服务；Helm 使用 `--set auth.registrationEnabled=false` 后升级。开关仅接受 `true` / `false`，默认 `true`。初始化脚本尊重已导出的开关值，但不会覆盖已有环境文件。关闭后注册 API 返回 `403`，登录页不展示注册入口，已有用户和会话保持可用；重新开启不改变已创建账号的角色。

注册成功自动登录，使用与现有登录一致的 24 小时 HttpOnly / SameSite=Strict 会话，生产包含 Secure。用户名区分大小写，密码长度按 UTF-8 字节校验；注册不要求邮箱验证，不提供密码找回、邀请码或审批。通知邮箱在登录后的个人设置中独立验证。每实例每 5 分钟最多受理 10 次有效格式的注册尝试，额度存储在数据库审计中，不随进程重启清零；超限返回 `429` 与 `Retry-After`。公开部署仍需配置反向代理连接 / IP 级防护；应用不会信任客户端自行提供的转发 IP 来放宽限流。

### 数据与外部存储

控制平面数据目录承载制品校验临时文件；至少留出最大并发校验文件的磁盘空间。源码只存在于任务的临时工作目录。单文件上限 20 GiB 不意味着小内存机器应使用相同大小的 tmpfs。S3 与 PostgreSQL 使用独立持久卷，默认不自动删除历史日志或制品。

使用外部 TimescaleDB / S3 时可直接部署镜像并设置与 `Settings::from_env` 对应的环境变量。每个数据库由管理员预先安装 TimescaleDB 2.32+ 扩展；遥测使用独立数据库与应用角色。S3 endpoint 只供 Control 和 Gateway 访问，二者使用相同固定地址。外部对象桶必须私有，不启用公开读取策略。新安装的 Compose / Helm 自动初始化遥测数据库；遥测 token、证书 SAN、数据保留和独立连接池见 [时序观测](telemetry.md)。

本仓库 Compose 清单和镜像不代表已经部署到用户生产域名；公有证书、DNS、磁盘容量、备份恢复与实际网络仍需现场验收。

### 隔离 Compose / TLS 验收

在有 Docker / Compose、Git、Python 3.12+ 和 OpenSSL 的 Unix 非 root 账户下运行：

```sh
python3 -u scripts/compose_acceptance.py
```

脚本从当前 Git HEAD 创建私有源码快照（未提交候选需先复制到隔离临时 Git 仓库并提交），构建实际 Control、Gateway 与控制台镜像；以独立项目名、卷、网络和随机 loopback 端口运行开发服务。测试经 Caddy TLS 入口连接 Pingora，覆盖文档、Secure Cookie、CSRF、租户鉴权和 Control 重启后的元数据持久化。CA 只注入测试客户端与独立进程，不修改宿主信任库。默认不加载已有 `.local/env` 或 `.deploy/env`，运行后仅清理本次 run 的容器、网络与卷。

完整构建链使用隔离 Kubernetes 验收：临时 SourceTask 固定提交，规划 Pod 的 init container 准备源码，原生 Emissary 拉取同一清单，再通过网关下载制品。入口和故障场景见 [Operator 运维](operator-operations.md)。Compose 验收不创建规划 Worker，也不执行构建。

`--image-prefix` 可指定拉取镜像所用的 registry 镜像源；输入变更、各镜像的实际平台与 ID 随证据记录，不修改默认部署镜像或全局 Docker 配置。可选 `--control-plane-image` / `--web-image` 仅复用 `org.opencontainers.image.revision` 与当前 HEAD、`artifactory.dockerfile.sha256` 与对应 Dockerfile 均匹配的镜像。`--probe-image` 可复用包含 python3、Git、cc、OpenSSL 的 Linux 测试工具镜像；Emissary 二进制仍从本次源码快照重新编译，验收脚本也由本次 run 单独挂载。

日志和证据保存在 `.local/compose-tls/<run>/`。其中源码快照含本次部署的主密钥、数据库 / S3 密码和测试 PKI，必须保持私有；CI 仅上传显式列出的诊断日志和证据文件，不上传快照、生成的 Compose 配置或 Emissary 私钥。专用 probe 容器只承载该验收的原生 Linux 进程，不属于产品的容器执行器，也不证明实体 Emissary 服务、五类真实应用、发行签名或公网 DNS / ACME 已验收。

## 文档站点

Compose 和 Helm 的 Web 镜像在构建时使用 mdBook 生成文档，部署后在控制台同域名的 `/docs` 免登录提供静态页面、搜索索引、Mermaid 脚本、图片和附件。静态文件随 Web 镜像版本更新、回滚；启动容器时不再构建文档，也无需额外文档服务。

构建 Web 镜像必须从仓库根目录提供上下文，Docker 会安装固定版本的文档工具：

```sh
docker build -f web/Dockerfile -t artifactory-web:local .
```

本地维护需要 mdBook 0.4.40 与 mdbook-mermaid 0.14.0：

```sh
cargo install --locked --version 0.4.40 mdbook
cargo install --locked --version 0.14.0 mdbook-mermaid
cd web
npm ci
npm run docs:build
npm run docs:serve
```

`docs:build` 输出到 Git 忽略的 `web/public/docs/`。`docs:serve` 在 `http://127.0.0.1:3001` 提供自动重建预览；启动控制台后可在其 `/docs` 验证正式路径。`npm run build` 自动先构建文档，工具缺失、版本不符或文档构建失败会中止 Web 构建。

章节目录由 `docs/SUMMARY.md` 维护，原 Markdown 路径保持稳定；新增页面须加入目录。设计计划和验收记录保留各自时点的背景与边界。Mermaid 资源由固定版本工具生成在仓库根目录的 `mermaid.min.js`、`mermaid-init.js`，已加入 Git 和 Docker 上下文忽略规则；生成文档时随静态站点打包，不从 CDN 加载。

在仓库根目录运行 `python3 scripts/check_docs.py` 检查目录覆盖、生成页面、内部链接、锚点与资源；完成 Web 生产构建后，在 `web` 运行 `npm run test:docs` 验证匿名访问、真实 404、搜索、主题、图表和移动导航。隔离 Compose TLS 验收也会检查同域名 `/docs`，文档内容不依赖 API 登录状态。

## Emissary 安装与注册

控制台提供完整的[安装并加入节点](node-installation.md)流程。
系统管理员先在 Emissary 设置保存设备公开连接地址。
再导入当前租户的签名版本，从节点页生成平台安装命令。
以下命令保留给手工安装和故障恢复。

运行 `sh scripts/cross-build-emissary.sh`，默认构建本机 Rust 目标，输出位于 `.local/emissary-bin`。其他目标由 `EMISSARY_TARGETS` 显式指定，需预装对应 linker / SDK；正式发布由五种原生 runner 分别编译。设备程序不依赖 Go。

注册时钉住管理员通过可信渠道分发的 `ca.pem`；不要从未知网络下载后直接信任。通过 `ARTIFACTORY_ENROLLMENT_TOKEN` 传短期令牌，注册后立即清除环境变量。配置文件和 `node.key` 只能由专用服务账户读取。

`register` 默认采用系统主机名；探测失败时采用 `emissary`。使用 `--name` 指定显示名称。名称去除首尾空白，长度为1–100个 UTF-8 字节，不含控制字符。注册后名称保持稳定，系统主机名、IP变化和服务升级不会自动改名。租户所有者或管理员可在设备详情重命名，或将当前主机名填入草稿后保存。改名沿用现有节点身份，不重新注册。

设备列表显示名称、短节点ID和有限硬件摘要。网络地址在详情中按接口显示，并附最近采样时间；离线设备保留最后快照。一个设备可以同时拥有局域网、VPN和IPv6地址。不要根据网关看到的连接来源推断设备唯一地址。字段缺失时先核对设备版本和启动元数据，不手工编造型号。

### Linux / systemd

以专用 `artifactory-emissary` 非 root 账户完成注册，给予其平台管理的工作目录与项目所需 SDK 访问，不给予 sudo 或通用管理员权限。

```sh
artifactory-emissary register --endpoint agents.example.com:8443 --ca /etc/artifactory/ca.pem --config /var/lib/artifactory/emissary.json --root /var/lib/artifactory/data
artifactory-emissary doctor --config /var/lib/artifactory/emissary.json
sudo artifactory-emissary service --action install --user artifactory-emissary --config /var/lib/artifactory/emissary.json
sudo artifactory-emissary service --action start --config /var/lib/artifactory/emissary.json
```

安装操作需要管理员创建 systemd 服务，但服务本身必须使用非 root 账户。确保该账户的 PATH 与 doctor 结果相同，SDK 许可已接受。

### macOS / launchd

使用专用构建账户，先在相同账户下完成 Xcode 许可、SDK、Keychain 与签名身份设置。以该账户注册，再由管理员安装带 `--user` 的 LaunchDaemon；不能假定后台 daemon 自动获得交互用户已解锁的钥匙串、登录会话或 UI 自动化授权。

```sh
artifactory-emissary doctor --config /Users/build/.config/artifactory/emissary.json
sudo artifactory-emissary service --action install --user build --config /Users/build/.config/artifactory/emissary.json
sudo artifactory-emissary service --action start --config /Users/build/.config/artifactory/emissary.json
```

完整 macOS 应用通常还需要按正确嵌套顺序签名、notarization / stapling。原生命令由仓库维护；本机 ad-hoc 验证不是 Developer ID 或 App Store 验收。

LaunchDaemon 的标准输出和错误日志保存在私有配置文件所在目录，文件名为 `emissary.log`；服务账户必须能写入该目录。launchd 降权后无法在默认 `/var/log` 中为普通账户新建文件，此时 Emissary 尚未执行就会以 `EX_CONFIG` 退出。

### Windows / Service Control Manager

使用具有“作为服务登录”权限的专用账户，不使用 LocalSystem。以管理员 PowerShell 安装，`ARTIFACTORY_SERVICE_PASSWORD` 仅供安装过程读取，不写到命令参数。工作目录、配置和私钥使用 NTFS ACL，仅授予该账户、SYSTEM 与管理员必需权限；Unix mode 位不能替代 Windows ACL 验证。

```powershell
artifactory-emissary.exe doctor --config C:\artifactory\emissary.json
artifactory-emissary.exe service --action install --user '.\artifactory-build' --config C:\artifactory\emissary.json
Remove-Item Env:\ARTIFACTORY_SERVICE_PASSWORD
artifactory-emissary.exe service --action start --config C:\artifactory\emissary.json
```

给服务账户配置 Visual Studio / SDK、PowerShell、Git 与签名证书的真实访问权限。Windows Job Object 使用挂起创建 → 分配 Job → 恢复，防止子进程在归属 Job 前逃逸；必须在 Windows 实机上验收取消、断线和服务停止。

SCM 启动或运行失败时，具体错误链原子写入配置旁的 `emissary-error.log`，保留最后一次失败及 UTC 时间。服务账户必须能写入该私有目录；SCM 的退出码不能替代此日志。

启动清理先回收旧 Worker，再删除任务目录。Windows 缓存中的 `Content.IE5` 等目录链接能拒绝列目录和同步访问。标准删除遇到访问拒绝后，Emissary 以不跟随链接的属性句柄识别目录链接，仅删除链接本身，再重试标准删除。递归期间锁定祖先目录，禁止目录被替换。此流程不修改 ACL、不进入链接目标；真实目录权限错误仍保留清理闭锁并拒绝任务准入。

三类系统均支持 `service --action stop|restart|uninstall`。本轮未向用户宿主安装任何系统服务。

Emissary 启动后若发生不可恢复的本地错误（例如配置从未注册），服务进程会记录错误并以非零状态退出，让系统服务管理器看到失败；网络中断仍由 Emissary 重连。系统是否自动重启失败服务取决于实际服务管理器及其恢复策略。

Worker 在 SIGTERM / SIGINT、租约到期和父控制管道 EOF 时先终止任务进程树并删除该 attempt 的受管工作目录。Windows 的后代由 Job Object 清理。

系统服务不要求交互式用户的 HOME / APPDATA：服务使用安装时保存的绝对 `--config` 路径。没有用户配置目录时，交互式注册命令也必须显式提供 `--config` 和 `--root`。systemd 单元对可执行文件与配置参数逐项引用，允许安装路径含空格；不把执行参数的转义误用为启动条件路径。

### 临时 CI 原生服务验收

`native-emissary` 作业在 Linux amd64 / arm64、macOS amd64 / arm64 与 Windows amd64 原生 runner 上运行 Rust 测试及 Clippy。`process_tree` 直接启动系统进程验证进程组 / Job Object 清理；`runtime` 使用真实 TLS 1.3 和 mTLS loopback 服务验证认证连接、OTA 替换 / 回滚，以及 Unix 前台和 supervisor 停止时的在途 Worker、日志确认和目录清理。

`native_service_lifecycle` 另由显式启用的 GitHub 托管临时 VM 执行，核对实际平台，创建专用账户，以带空格的路径安装、启动、重启、停止并卸载 systemd / launchd / SCM 服务，验证认证连接、在途进程树和后续任务。普通测试跳过此项，不在开发机安装系统服务。systemd / launchd / SCM 的专用账户安装、OS 重启、各平台 SDK 与发行签名必须在相应的临时 VM 或实体机单独验收；不能用过去 Go 实现的服务记录代替 Rust 版本验收。实际执行结果见 [Emissary 移植说明](https://github.com/zhubby/artifactory/blob/main/crates/emissary/PORTING.md) 与带日期的验收记录。

## 本机签名配置

在 Emissary 的私有 JSON 中增加引用到环境变量的映射，例如：

```json
{
  "signing_profiles": {
    "apple-release": {"ARTIFACTORY_APPLE_IDENTITY": "Developer ID Application: YOUR TEAM"},
    "android-release": {"ARTIFACTORY_ANDROID_KEYSTORE": "/private/signing/app.jks", "ARTIFACTORY_ANDROID_STORE_PASSWORD": "LOCAL_SECRET"}
  }
}
```

只是对已有完整配置的增量示例，不是可独立运行的配置。控制平面只保存引用名和授权；不要上传应用签名私钥。项目、节点设置及本机配置都需授权同一引用。

doctor 报告路径、版本命令是否成功、实际 OS/CPU、可发现工具及本机配置的签名引用；不保证 SDK 完整、证书有效、iOS 设备可安装或商店可发布。可使用本机 `tools` 配置协助发现非标准路径；实际脚本仍需明确可执行路径 / PATH。

## 排空、升级、续证

设备终端本地默认关闭。启用方式、固定 Shell profile、私有 8446 转发端口和独立连接预算见[设备终端](terminals.md)。升级前先关闭终端准入、撤销活动会话并确认设备 Worker 回收。终端追加迁移为 `2026112403`；保留审计和追加表，不执行破坏性降级迁移。租户删除等待终端授权和设备租约安全窗口后清理终端记录。

常规 Emissary 升级使用 [Emissary OTA](emissary-ota.md)。下面的手动流程用于首次安装启动器或管理员恢复。

1. 控制台设置 draining，不分配新任务，等待 active_jobs 为 0。
2. 停止 Emissary 服务，备份私有配置、证书与签名环境；按目标 OS/CPU 替换二进制。
3. 使用同一账户运行 version / doctor，再启动，验证能力、标签、slots 和一次实际构建。
4. 证书为 30 天有效，本版需提前排空、吊销旧节点、新路径重新注册，再重设节点授权；没有自动续证和证书热替换。
5. 协议主版本不兼容时先排空，不允许新旧版本盲目混跑。

## 故障处理

- 控制平面重启：数据库租约保留，90 秒内重连可以继续同一个 attempt；不要手动把 running 改为 queued。
- 网络分区：本地租约截止杀进程树；已开始任务记 lost，不自动重跑。确认副作用后显式 retry。
- Emissary 崩溃：Worker 控制管道关闭，终止子进程；服务恢复后旧任务不能复活。检查本机 `lost/`、`spool/` 和审计。
- 日志超额 / 存储不可用：任务明确失败或保留未确认日志，不删除 spool 来伪造成功。需要人工按 attempt 对照数据库保存日志证据。
- 发布失败：构建状态与制品保留，查看 publish_error / publish_attempts；可重试网络类问题，版本冲突不能覆盖，应以新版本创建构建。
- 取消：未开始任务取消，在途任务收到取消或失去续租后终止；日志可在剩余有效租约内补传。

## 备份、恢复与保留

必须协调备份各 TimescaleDB 数据库、S3 对象、`.deploy/env` 中主密钥及 PKI；只有数据库备份无法解密凭据，也无法恢复制品。备份应离线加密，并独立限制访问。

1. 排空节点、停止新触发、等待在途任务结束，停止 Control 和 Telemetry 写入。
2. `pg_dump` 分别保存实际部署的业务、规划协调器、遥测数据库，记录 schema 与 TimescaleDB 扩展版本；同时保存 S3 内容与对象校验清单。分别导出的数据库不自动构成跨库事务快照，必须先停写。
3. 保存主密钥、S3 密钥、签发 CA、节点授权与镜像版本；不要在普通工单里复制这些秘密。
4. 在相同 TimescaleDB 版本的隔离环境恢复：各库预装扩展，执行 `timescaledb_pre_restore()`，串行 `pg_restore` 后执行 `timescaledb_post_restore()`。使用相同 master key，检查凭据解密、Release 下载 SHA-256、遥测列存 / 聚合 / 后台策略及最新摘要，再开放流量。
5. 升级前保留备份，执行迁移后回滚必须连同 schema 和对象状态一起评估，不能仅回退二进制。

构建日志与制品没有自动 GC / retention API；遥测按 [保留策略](telemetry.md#数据与保留策略) 自动清理。禁止直接批量删除平台数据目录或 Release 对象。暂存对象和孤立失败日志的清理必须停写、基于数据库引用清单、先备份再删除；已发布制品不能按文件时间猜测是否无用。生产备份恢复演练仍是独立验收项。

旧实例的备份保留其原数据库、S3/Registry 对象、PKI 和主密钥。新实例不导入旧业务列表或授权，设备向新实例重新注册。旧实例的停写、任务排空和资源退役由管理员独立执行；安装新版本不会触发这些操作。

## 临时规划与源码

模型配置、任务预算、临时源码和审计会话见 [AI 构建运维](ai-builds.md)。AI 规划只使用 Kubernetes Pod，不注册常驻 Worker。源码由临时 SourceTask、任务 init container 和原生 attempt 直接访问上游，平台只持久化源码清单与凭据引用。

任务终态先持久化结果和日志，再按 UID 删除 Pod。Operator 确认全部容器停止后记录停止证据，移除 Pod finalizer，并等待对象消失。原生节点确认进程树退出后清理；清理失败闭锁接单。Registry 不保存构建缓存，最终镜像作为制品保留。

托管方案支持加密上传与按任务临时下发。
配置和安全边界参见[应用签名](signing.md)。

## 节点诊断与独立清单刷新

设备详情的有限操作只授予当前租户管理员 Session。诊断采用固定工具目录和 30 秒预算；结果只包含有限状态、数字版本与耗时。权限不足或工具超时会保留逐项原因；unsupported 表示工具缺失。普通成员不能读取诊断结果。

刷新成功表示新的清单观察已耐久写入本地队列。delivery=pending 时仍等待遥测确认。刷新不会改变节点启动 metadata、capabilities 或调度计划；失败不会覆盖已有成功清单。

设备重启后，在途操作为 interrupted，不能自动重跑。网络中断时等待重新连接与状态授权；outcome_unknown 只表示平台没有得到终态，不能据此判断设备是否执行。需要新采样时使用新 request_id。不要删除 operations/journal.json 来绕过去重或未确认结果。

节点操作正文在服务端保留 24 小时，元数据保留 90 天。节点或租户删除通过数据库外键清除操作记录。设备本地 journal 最多 64 项、8 MiB，摘要确认后释放正文；未确认结果最多保留 7 天。空间不足时拒绝新操作并保留既有证据。
