<!--
[INPUT]: 已批准的镜像方案、源码实现与真实集群验收。
[OUTPUT]: 记录实施完成项与环境验收边界。
[POS]: 独立 Operator、镜像仓库和 Pingora 的交付计划。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

# 镜像构建与独立 Operator 实施计划

> 用户已批准。实现已合并并推送 `main`。

## 已确定的边界

- `operators/agent-operator` 提供 `artifactory-agent-operator`。
- `operators/image-operator` 提供 `artifactory-image-operator`。
- 每个 Operator 使用独立镜像、Chart、RBAC 和选主 Lease。
- 保留 Agent CRD 身份、Source PVC、既有字段管理身份与 Helm release。
- BuildKit 读取固定源码，原生架构优先，显式允许模拟。
- 各平台先提交临时镜像，再合并 Index 并发布最终标签。
- Distribution 提供内建 OCI 仓库，S3 使用独立 Bucket。
- 控制平面提供仓库令牌、镜像元数据和发布授权。
- Pingora 反向代理内建仓库、制品和控制入口。
- 设备入口透传 mTLS。S3 不对客户端暴露。
- 外部仓库只作为发布目标。按 digest 复制，失败可单独重试。

## 实施检查表

- [x] 拆分 Agent Operator 与多角色运行镜像。
- [x] 增加镜像构建契约与持久化状态。
- [x] 实现 Image Operator、BuildKit Job 和发布任务。
- [x] 接入 Registry Token、镜像元数据和外部发布。
- [x] 实现 Pingora HTTP、制品票据和 TCP 入口。
- [x] 增加独立 Chart、镜像 CI、控制台和运维说明。
- [x] 执行本机回归、构建、Chart 与网关协议验收。
- [x] 完成真实集群 amd64 镜像与内建仓库端到端验收。

## 验收

验证两个 Operator 独立选主；固定源码与执行身份；取消和超时；
双架构 Index；外部发布重试；跨租户拒绝；大文件流式传输；
Range/HEAD 与分段上传；设备 mTLS 和副本恢复。

真实集群先备份数据库与部署参数，再升级现有平台。
构建验收使用隔离项目，不修改既有项目的规划后端。

## 本机验证记录

- Rust 工作区默认测试、全目标 Clippy 和格式检查通过。
- 依赖真实服务的 ignored 测试没有计入默认测试结果。
- Agent Operator：11 项资源、选主与清理回归通过。
- Image Operator：9 项节点选择、容器生命周期、CRD 与日志回归通过。
- OCI 协议与客户端：6 项完整图复制、内容校验和损坏拒绝回归通过。
- PostgreSQL：11 项隔离镜像回归通过，包含 40 个执行的轮转认领。
- Registry Token：RSA 证书签名及多仓库 scope 的离线验证通过。
- Pingora：8 MiB 流式上传、Range/HEAD、仓库分段上传通过。
- 网关隔离：内部 TLS、双 CA 与设备 mTLS 透传通过。
- 控制台：桌面和移动端共 6 项镜像界面测试通过。
- 前端 TypeScript 检查和 Next.js 生产构建通过。
- AI Worker：16 项测试通过。运维和验收脚本：40 项测试通过。
- 4 个 Chart 严格 lint，以及主 Chart 的隔离数据入口配置通过。
- Actionlint 和十个镜像的发布矩阵一致性检查通过。
- 文档检查通过：73 章、8813 个本地链接和资产。

## 环境验收范围

目标集群为 Kubernetes 1.28.3，六台节点均为 amd64。
固定摘要的依赖镜像缓存解决 Docker Hub 直连超时。
详细证据见[集群验收记录](../../image-builds-kubernetes-acceptance-2026-11-17.md)。

- [x] 在真实 Distribution 上验证令牌、推送和摘要拉取。
- [x] 在 Kubernetes 上构建原生 amd64 镜像并发布 OCI Index 与 Release。
- [ ] 在 Kubernetes 上执行 rootless BuildKit 双原生架构构建。
- [ ] 在实际外部仓库上验证完整复制与失败重试。
- [x] 运行中的镜像任务取消后清理 Pod、Job 与任务 Secret。
- [x] Image Operator 双副本切主并恢复正常续租。
- [ ] 在真实运行的镜像任务上验证超时。

四个独立 Helm release 已部署。原数据库、PVC 和设备 CA 保留。
最终检查的 16 个工作负载全部就绪。
镜像 Index、manifest、config 与 layer 的摘要读回通过。
镜像 HEAD、Range 与项目 Release 摘要一致性通过。
临时代理、端口转发、探针和保留的诊断 Job 已清理。
arm64、外部仓库和运行中超时仍保留为环境验收。

[PROTOCOL]: Update this header on change, then check AGENTS.md

## 审查修复

- 排空保留已接受镜像任务的认领、源码和回执权限。
- LFS 能力只注入原生目标，持久化前复验计划。
- 日志先转换 UTF-8，再按字符边界切分至 16 KiB。
- Buildctl 单独配置仓库认证 CA，并保留系统根证书。
- 截止期限归为失败，完整平台结果保留发布重试资格。
- 删除清单记录镜像日志项目和五分钟写入宽限期。
- 外部发布固定公开仓库来源，凭据停用不改变历史引用。

2026-11-17：以上修复通过 11 项镜像数据库回归、6 项镜像界面回归、Rust 工作区默认测试、全目标 Clippy、格式、前端生产构建及 40 项脚本测试。数据库默认忽略的环境测试另行执行；这里的 11 项为显式运行结果。
