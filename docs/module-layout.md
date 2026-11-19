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
| `crates/source-checkout` | `artifactory-source-checkout` | 固定源码清单、直接 Git/LFS 拉取与核验 |
| `crates/source-preparer` | `artifactory-source-preparer` | 任务 Pod 的源码解析及 init container 入口 |
| `crates/planner-runtime` | `artifactory-planner-runtime` | Control 内嵌库，提供 AI 预算、租约、模型请求与规划恢复 |
| `packages/agent-worker` | `artifactory-agent-worker` | 每个规划 Pod 的本地只读源码工具与 AI 循环 |
| `operators/agent-operator` | `artifactory-agent-operator` | SourceTask 与 AgentRun 临时 Pod |
| `operators/image-operator` | `artifactory-image-operator` | 每个平台的临时 BuildKit 与 Publisher Pod |
| `crates/image-protocol` | `artifactory-image-protocol` | 镜像目标与执行材料契约 |
| `crates/image-registry` | `artifactory-image-registry` | OCI 图校验与镜像发布 |
| `crates/gateway` | `artifactory-gateway` | 统一反向代理和设备 TLS 透传 |
| `crates/emissary` | `artifactory-emissary` | 原生设备任务、临时 checkout 与制品上传 |
| `web` | `artifactory-console` | Next.js 多租户控制台 |

两个 Operator 是独立 crate 和 Deployment，均位于 `operators/`。共享服务与库位于 `crates/`。Node.js 规划主容器只通过 Runtime 领取任务，每次 Pod 的 init container 提供本次源码。

Control 只保存仓库身份、commit、子模块、tag 和 LFS 摘要。制品与日志进入制品 Bucket，OCI 镜像进入独立 Registry Bucket。源码工作树、Git 对象与构建中间缓存只存在于任务临时目录。

主 Chart 部署内嵌规划 Runtime 的 Control、Web、统一双副本网关、遥测和私有存储。Agent Operator Chart 部署 Agent Operator 与 AgentPool。Image Operator Chart 部署 Image Operator 与 BuilderPool。Registry Chart 部署 Distribution。

项目使用全新初始化数据库基线。原型数据重置后重新创建业务记录，不支持旧架构数据库升级或旧协议双路径。
