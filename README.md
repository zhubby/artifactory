<!--
[INPUT]: 工作区实现、部署文档与隔离租户验收入口。
[OUTPUT]: 提供项目简介、本机启动与验证命令。
[POS]: 根目录入口，链接模块文档与历史验收记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

# Artifactory

项目统一版本以根目录 `VERSION` 为准。版本检查、Release 与受限 Kubernetes 试部署见[统一版本与发布](docs/versioning.md)。

自助注册默认开启：访客注册后自动登录为全实例只读用户，可查看和下载全部项目的构建与产物。仅允许管理员分配账号时，设置 `ARTIFACTORY_REGISTRATION_ENABLED=false`；Helm 使用 `auth.registrationEnabled=false`。详情见 [用户自助注册](docs/operations.md#用户自助注册)。

面向个人与内部可信团队的跨平台应用构建、签名与内部分发平台。

文档使用 mdBook 按使用、部署、接口、验收与历史设计整理：[文档首页](docs/README.md)。Web 部署后通过同域名 `/docs` 免登录阅读，构建与维护命令见[文档站点](docs/operations.md#文档站点)。

项目于 2026-11-11 统一更名为 **Artifactory**，产品展示名首字母大写，仓库为 [zhubby/artifactory](https://github.com/zhubby/artifactory)。命令、配置与环境变量使用 `artifactory` 标识；设备端现为 Rust `artifactory-emissary`。命名与迁移说明见 [更名记录](docs/artifactory-rename-2026-11-11.md)，后续复审修复及实际部署见[本机 / Mac mini 验收](docs/local-macmini-acceptance-2026-11-11.md)；历史设计和验收记录保留各自提交、运行结果与验收边界。

**只读 Git 项目 → 固定源码清单 → Pi AI 执行计划 → 原生节点构建 → 签名 / 校验 → 私有制品 → 不可变 Release。**

新构建不需要仓库配置文件。用户描述目标，AI 只读分析源码并生成命令与外置脚本，所有说明和计划保存在平台；最多自动修订两次。模型配置、任务身份和临时源码流程见 [零侵入 AI 构建](docs/ai-builds.md)，真实模型及本地 / Mac mini 测试见 [AI 构建验收记录](docs/ai-builds-acceptance-2026-11-11.md)。

## 实现范围

- Rust/Axum/Tonic 控制平面，支持 [多副本部署](docs/control-ha.md)；PostgreSQL 持久化命令、队列、租约与审计，无新增消息中间件。
- Rust Artifactory Emissary 主动出站接入；Linux/macOS/Windows 原生执行器；独立 Worker、进程树取消、有界日志缓冲。Pi Agent 专注仓库分析和规划。
- Next.js/React/shadcn 风格 Radix 组件/Tailwind 控制台，项目、构建矩阵、SSE 日志、节点、制品、Release、身份与授权均使用真实 API。
- HTTPS/SSH Git 凭据加密托管；提交、仓库地址、凭据引用、配置、参数按构建快照固定。
- 节点证书认证、一次性注册、角色与项目范围 Token、S3 内容校验、签名引用的项目 / 节点双向授权。
- 独立 Image Operator 在 Kubernetes 启动 BuildKit，支持 Linux amd64/arm64、完整 OCI Index、内建私有 S3 仓库和外部仓库独立发布；部署与验收边界见[镜像构建与网关](docs/image-builds.md)。
- Pingora 为控制入口、内建仓库和制品提供反向代理；S3 保持私有。设备入口透传原有 mTLS，数据与设备连接使用独立预算。
- 原生执行仅适用于可信代码，不是安全沙箱。不兼容 GitHub Actions YAML、通信协议或插件生态。

**状态：已提供可执行首版与自动化验收；不能将本机联测或交叉编译视为三类宿主、五种应用目标全部验收。实际证据与缺口见 [验收记录](docs/acceptance.md)。**

## 本地开发

需要 Rust 1.95、Node 24、npm、protoc、OpenSSL、Git、Git LFS、Python 3、Docker。只有独立验收的上游 HTTP fixture 使用 Go；设备运行程序完全由 Rust 实现。

```sh
sh scripts/init-local.sh
sh scripts/local-services.sh
set -a; . .local/env; set +a
export GIT_SSL_CAINFO="$ARTIFACTORY_PKI_DIR/ca.pem"
cargo run --bin artifactory-control-plane
```

后端依赖统一为 TimescaleDB 2.32 / PostgreSQL 17；新建本地环境还需在独立终端运行 `sh scripts/run-telemetry.sh`。Emissary 的主机指标、托管构建进程与能力清单通过独立 Rust 遥测服务记录，配置、保留策略和查询 API 见 [时序观测](docs/telemetry.md)。

另开终端启动控制台：

```sh
cd web
npm ci
npm run dev
```

另开终端加载 `.local/env` 并运行 `cargo run -p artifactory-gateway`，浏览器从 Pingora `http://127.0.0.1:8081` 登录、上传和下载。初始账号及随机密码保存在 `.local/env`。Control 8080 与 Next.js 3000 为内部后端，数据库和 S3 仅绑定本机。已有环境须补充随机 `ARTIFACTORY_GATEWAY_TOKEN` 和对应网关地址，不重新生成主密钥或设备 CA。本地 HTTP 配置不能直接用于生产。

Emissary 注册：在控制台创建 15 分钟有效的一次性令牌，以环境变量传递，不写入命令行或仓库。

```sh
cargo build --locked --bin artifactory-emissary
export ARTIFACTORY_ENROLLMENT_TOKEN='从控制台获取'
target/debug/artifactory-emissary register --endpoint localhost:8443 --ca .local/pki/ca.pem --config .local/emissary/emissary.json --root "$(pwd)/.local/emissary/data" --allow-http-uploads
unset ARTIFACTORY_ENROLLMENT_TOKEN
target/debug/artifactory-emissary doctor --config .local/emissary/emissary.json
target/debug/artifactory-emissary run --config .local/emissary/emissary.json
```

生产部署、三类系统服务与签名节点设置见 [运维手册](docs/operations.md)。不要在已有生产配置旁运行本地验收脚本。

Kubernetes / 本机 kind 使用 [Helm 部署指南](docs/helm.md)。GitHub Release 发布独立服务的 AMD64、ARM64 镜像和四个 Chart。新 AI 构建前需按 [AI 构建指南](docs/ai-builds.md) 配置模型与任务身份。

Kubernetes Pi 由独立 Rust Operator 创建临时 Job。Runtime 保存预算和会话审计，SourceTask 和各任务的 init container 从上游拉取固定源码。原生设备每次 attempt 独立拉取。平台不保存源码工作目录或 Git/LFS 缓存。部署见 [Operator 运维](docs/operator-operations.md)，契约见 [运行协议](docs/runtime-protocol.md)。

## 验证

```sh
python3 -m unittest discover -s scripts -p 'test_*.py' -v
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
ARTIFACTORY_TEST_DATABASE_URL=postgres://artifactory:local-test-only@127.0.0.1:15432/artifactory cargo test --test database -- --ignored
cargo test --locked -p artifactory-emissary --all-targets
sh scripts/cross-build-emissary.sh
(cd web && npm run typecheck && npm run build)
(cd packages/agent-worker && npm ci && npm run check && npm test)
```

完整端到端验收需要 Kubernetes，在隔离命名空间启动确定性 Git/模型夹具，验证临时源码、Pi、Rust Emissary 与网关制品交付：

```sh
python3 tests/operator/acceptance/prepare.py --help
python3 tests/operator/acceptance/run.py --help
```

Rust 原生测试验证控制管道断开、租约到期与进程信号后的清理、日志确认和 OTA。各系统原生服务及真实宿主验收单独记录，交叉编译不能代替原生测试。

Compose/TLS 验收检查服务健康、HTTPS、会话、CSRF、租户鉴权和控制平面重启。完整规划与构建只走 Kubernetes：

```sh
python3 scripts/compose_acceptance.py
```

## 仓库导航

模块命名及依赖边界见[模块布局与命名](docs/module-layout.md)。

| 目录 | 内容 |
|---|---|
| `crates/control-plane` | 控制平面、SQL 迁移、权限 / 状态 / 数据库测试 |
| `operators/agent-operator` / `charts/artifactory-agent-operator` | 独立 Agent Operator、AgentRun/SourceTask CRD |
| `operators/image-operator` / `charts/artifactory-image-operator` | 独立 Image Operator、BuildKit/Publisher Job |
| `crates/image-protocol` / `crates/image-registry` | 镜像执行契约、OCI 内容校验与按 digest 复制 |
| `crates/gateway` / `charts/artifactory-registry` | Pingora 入口与私有 Distribution 仓库 |
| `crates/planner-runtime` / `crates/runtime-protocol` | Pi 队列、租约/预算、模型代理和幂等交接 |
| `crates/source-checkout` / `crates/source-preparer` | 固定源码清单、上游检出、临时解析和 init 程序 |
| `crates/emissary` / `crates/emissary-protocol` | Rust 设备运行程序、独立 Worker、原生服务、OTA 与共享协议 |
| `packages/agent-worker` | Node 24 Pi SDK 规划进程、只读工具和模型接口 |
| `packages` | Node.js 模块目录，各模块独立管理依赖与构建 |
| `proto/artifactory/emissary/v1` | Emissary Protobuf 公共契约 |
| `schemas` / `examples` | 内部执行配方和历史配置 schema 与目标参考 |
| `web` | 控制台、Playwright 真实后台测试 |
| `deploy` / `compose.yaml` | Linux 单实例部署 |
| `docs` | 配置、API、协议、运维与验收边界 |

商店发布、Webhook 和 OIDC 尚未实现。集群镜像构建已提供代码与部署入口；本机测试不代替真实双原生架构集群、外部仓库和生产存储验收。
