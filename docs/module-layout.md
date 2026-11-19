<!--
[INPUT]: Rust 工作区与独立 Node.js 规划模块。
[OUTPUT]: 定义当前模块名称、职责和依赖方向。
[POS]: 项目目录导航；只描述当前架构。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 模块布局与命名

| 目录 | 包或命令 | 职责 |
|---|---|---|
| `crates/control-plane` | `artifactory-control-plane` | 业务身份、源码元数据、构建授权与制品 |
| `crates/database` | `artifactory-database` | 数据库连接预算、基线校验与 SQL 边界 |
| `crates/planning-protocol` | `artifactory-planning-protocol` | 规划意图、执行身份、租约与事件的纯数据契约 |
| `crates/service-transport` | `artifactory-service-transport` | 服务认证、HTTP/TLS、密封凭据、代理与有界指标 |
| `crates/execution-support` | `artifactory-execution-support` | 进程、事件、私有文件与流式脱敏 |
| `crates/emissary-protocol` | `artifactory-emissary-protocol` | 节点 RPC、签名、资源身份与遥测模型 |
| `crates/telemetry-service` | `artifactory-telemetry-service` | 独立的时序写入、查询与节点授权 |
| `crates/source-checkout` | `artifactory-source-checkout` | 固定源码清单、直接 Git/LFS 拉取与核验 |
| `crates/source-preparer` | `artifactory-source-preparer` | 任务 Pod 的源码解析及 init container 入口 |
| `crates/planning-coordinator` | `artifactory-planning-coordinator` | Control 内嵌库，提供 AI 预算、租约、模型请求与规划恢复 |
| `packages/planning-worker` | `artifactory-planning-worker` | 每个规划 Pod 的本地只读源码工具与 AI 循环 |
| `operators/planning-operator` | `artifactory-planning-operator` | SourceTask 与 PlanningRun 临时 Pod |
| `operators/image-operator` | `artifactory-image-operator` | 每个平台的临时 BuildKit 与 Publisher Pod |
| `crates/image-protocol` | `artifactory-image-protocol` | 镜像目标与执行材料契约 |
| `crates/oci-registry-client` | `artifactory-oci-registry-client` | 共享 OCI 仓库客户端，提供认证、内容校验与镜像图复制 |
| `crates/gateway` | `artifactory-gateway` | 统一反向代理和设备 TLS 透传 |
| `crates/emissary` | `artifactory-emissary` | 原生设备任务、临时 checkout 与制品上传 |
| `web` | `artifactory-console` | Next.js 多租户控制台 |

两个 Operator 是独立 crate 和 Deployment，均位于 `operators/`。共享服务与库位于 `crates/`。`oci-registry-client` 编译进 Control、Runner 与 Publisher，由调用方访问仓库。Registry Chart 单独部署 Distribution 仓库服务。Node.js 规划主容器通过规划协调器领取任务，每次 Pod 的 init container 提供本次源码。

`planning-protocol` 只依赖序列化和 UUID。`service-transport` 不依赖领域契约或数据库。Emissary 与 Image Operator 共同依赖 `execution-support`；Image Operator 不依赖 Emissary。节点专用会话恢复留在 Emissary。遥测模型归属 `emissary-protocol`，遥测服务保持独立部署。

Control 只保存仓库身份、commit、子模块、tag 和 LFS 摘要。制品与日志进入制品 Bucket，OCI 镜像进入独立 Registry Bucket。源码工作树、Git 对象与构建中间缓存只存在于任务临时目录。

主 Chart 部署内嵌规划协调器的 Control、Web、统一双副本网关、遥测和私有存储。Planning Operator Chart 部署 Planning Operator 与 PlanningPool。Image Operator Chart 部署 Image Operator 与 BuilderPool。Registry Chart 部署 Distribution。

本版只支持全新实例。规划 API、CRD、配置与数据库基线使用统一规划名称，不提供旧架构原地升级或协议兼容层。业务基线为 `2026112001`，规划基线为 `2026111901`。程序拒绝旧数据库基线，不自动重置数据库、删除旧实例或清理持久资源。旧实例由管理员另行退役。
