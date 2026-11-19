<!--
[INPUT]: 固定源码身份、Operator 执行观察和 Runtime 事务。
[OUTPUT]: 定义准备、领取、故障回报与恢复边界。
[POS]: 控制平面与独立规划 Runtime 的协议入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Runtime 与临时源码协议 v1

控制平面只保存固定源码清单。每个源码任务、规划 Pod、镜像 Pod 和原生执行任务直接访问 Git 上游。任务目录不作为后续任务的输入。平台持久化完成的 AI 对话、模型预算、计划、构建日志和制品。

## 状态归属

| 数据 | 写入方 | 存储 |
| --- | --- | --- |
| Build、源码任务、固定源码清单、计划接纳、设备执行、制品 | control-plane | 业务 PostgreSQL |
| 运行队列、有效 Pod、累计调用、原 deadline、持久提案 | planner-runtime | Runtime PostgreSQL |
| 按 run/generation 排序的完成对话、系统提示、工具调用与结果 | planner-runtime | Runtime PostgreSQL |
| Git checkout、Git/LFS 下载目录、认证 HOME | source-preparer / 原生执行进程 | 单任务临时目录 |
| AgentPool、AgentRun、SourceTask、ImageBuild | Operator | Kubernetes API |

业务数据库和 Runtime 数据库分别初始化。服务不能跨库查询。数据库只保存仓库地址、commit、冻结 tag 对象、LFS OID 与大小。凭据通过凭据引用传入；密钥不进入 URL、CRD 和审计消息。制品和内建 OCI 仓库继续使用独立对象存储。

## 源码任务

构建先进入 `resolving`。控制平面在 `source_tasks` 创建 `resolve` 任务。Operator 直接创建使用 `emptyDir` 的临时 Pod。source-preparer 通过投射令牌和实际 Pod 身份领取仓库地址及短期凭据。Control 核对 Pod→SourceTask 的完整 owner 身份与执行台账，不通过标签授权。

首次解析引用后，任务向控制平面提交 `pinned_ref_oid`。后续重试只能得到第一个已提交 OID。每个仓库的 tag 对象也按任务固定。preparer 直接拉取该对象，解析 submodule 和 LFS，然后提交排序后的 `SourceManifest`。清单摘要为 `sha256(serde_json::to_vec(repositories))`。

控制平面在同一事务中保存清单、切换 Build 为 `planning`、创建唯一 `planning_run`，并由触发器写入 Runtime outbox。重复提交相同任务结果返回已有结果。不同结果拒绝。成功任务再次领取时返回已提交结果，不再返回凭据。

引用选择使用独立 `refs` 任务。接口只等待引用元数据，不创建 Build，也不保留 Git checkout。任务结束后删除 Pod 和临时卷。

## Kubernetes 规划

1. Runtime 根据 pool/project 容量准入运行，返回 `run_id`、`generation` 与可选源码身份。
2. Agent Operator 先持久化执行意图，再直接创建 Pod，登记实际 Pod UID、Pod 名和 namespace。
3. source-preparer init container 调用 Runtime `/v1/prepare`，直接拉取固定源码到 `emptyDir`。认证材料仅位于 init 可见的内存卷。
4. 主 Worker 只读挂载源码和清单元数据。Worker acquire 成功后启动模型 deadline。
5. Pi 通过本地只读工具读取源码。节点能力和失败日志经 Runtime 访问控制平面。所有模型请求经过 Runtime 预算代理。
6. 任务终态后 Operator 删除 Pod，确认停止并保存证据后移除 finalizer。Pod 丢失时，Operator 报告完整旧执行身份，由 Runtime 撤权并决定新 generation。

placement 运行只读节点元数据，不准备源码。源码准备不 acquire 模型租约，不启动模型 deadline。执行登记尚未完成时 `/v1/prepare` 返回 409，preparer 有界重试。Kubernetes API 的 404、409、429、5xx 和网络故障返回 503。无效投射令牌和错误执行身份返回 403。

preparer 配置：

```text
ARTIFACTORY_SOURCE_PREPARE_URL=<runtime>/v1/prepare
ARTIFACTORY_SOURCE_IDENTITY={"run_id":"...","generation":1}
ARTIFACTORY_SOURCE_ROOT=/workspace/source
ARTIFACTORY_SOURCE_AUTH_ROOT=/run/source-auth
ARTIFACTORY_SOURCE_METADATA=/workspace/source-manifest.json
ARTIFACTORY_POD_TOKEN=<projected token path>
POD_UID / POD_NAME / POD_NAMESPACE
ARTIFACTORY_SERVICE_CA=<optional CA path>
```

`POST /v1/prepare` 输入为 `identity`、`pod` 和 `bootstrap_token`。Runtime 对 audience `artifactory-runtime` 执行 TokenReview，核对 SA `artifactory-pi` 以及完整 Pod 身份，再向控制平面 `/internal/runtime/source-material` 请求材料。控制平面和 Runtime 在材料读取后再次核验 generation、Pod 和准备状态。等待期间发生取消或执行替换时，旧 Pod 无法领取材料。材料返回 `PrepareResponse`；主 Worker 不接触上游凭据。

## Runtime 接口

| 路径 | 身份 | 作用 |
| --- | --- | --- |
| `/v1/intent` | 控制平面服务身份 | 不可变意图或高版本取消 tombstone |
| `/v1/queue` | Operator 服务身份 | 原子准入并返回有效运行 descriptor |
| `/v1/get`、`/v1/register` | Operator 服务身份 | 读取运行和登记 Pod |
| `/v1/execution-failed` | Operator 服务身份 | 以完整执行身份提交 init、Worker 或 Pod 终止 |
| `/v1/execution-lost` | Operator 服务身份 | 以完整执行身份撤销丢失 Pod，或结束被删除的 CR |
| `/v1/prepare` | Kubernetes 投射令牌 | 给 init 提供固定源码材料 |
| `/v1/acquire` | Kubernetes 投射令牌 | 获取 run grant 并启动首次模型 deadline |
| `/v1/heartbeat`、`/v1/report` | run grant | 续租、累计 usage 与完成消息审计 |
| `/model/{run}/{generation}/{uid}/v1/...` | run grant | 原子预留预算并代理模型 SSE |
| `/v1/tool`、`/v1/proposal` | run grant | 转发只读工具、保存提案与读取原回执 |
| `/v1/conversations` | 服务身份及 run/tenant/project | 列出 generation 会话摘要 |
| `/v1/conversation/messages` | 服务身份及 run/tenant/project/generation | 按 sequence 增量分页读取消息 |

Operator 通过受认证 HTTP 客户端读取和登记执行。Runtime 同时提供版本化 Tonic gRPC GetRun/RegisterExecution 接口。服务使用 TLS，HTTP 仅允许显式开发配置。共享客户端禁止重定向。源码清单上限为 4 MiB；服务 JSON 请求和响应上限为 8 MiB。完成消息超过单事件大小时按 Unicode 安全分片，并使用稳定事件 UUID 顺序持久化。

故障回报包含 `execution` 和固定枚举 `reason`。
`execution` 必须匹配 run、generation、Pod UID、Pod 名与 namespace。
`reason` 仅包含源码退出码、Worker 退出码或 Pod 失败类别。
容器日志和 Kubernetes message 不进入该回报。
Runtime 在同一事务中置为失败、清除 grant、结束会话并写入失败 outbox。
控制平面消费 outbox 后结束业务构建。
重复回报不重复写入事件，旧身份被拒绝。
已提交提案继续由原回执决定结果，Pod 退出不能覆盖已接受结果。

丢失回报包含 `run_id`、`generation`、`execution` 和 `reason`。`reason=pod_missing` 用于 Pod 消失或开始删除，要求完整执行身份与 Runtime 登记完全一致；事务撤销 grant，结束旧会话，并在预算允许时重新排队到下一 generation。重试最多三个 generation，累计调用与原 deadline 不重置。`reason=resource_deleted` 结束被删除 CR 对应的活动任务；仅 Runtime 尚未登记执行时允许 execution 为空。已有终态和持久提案不被覆盖。成功恢复后，重复旧 generation 回报被 fencing 拒绝；Operator 重新读取当前运行状态。旧 Pod 必须停止并完成清理后，Operator 才创建新 generation 的 Pod。未知旧 Pod 的停止证据仍由 Operator 保存，不能据此释放实际执行容量。

## 对话与故障恢复

完成的 system、user、assistant、tool 调用和 tool result 消息按 generation 保存。凭据精确值脱敏，代码片段保留。消息不写入可恢复执行状态。浏览器经项目读取权限校验后访问：

```text
GET /api/v1/builds/{build}/planning/conversations
GET /api/v1/builds/{build}/planning/conversations/{run}/{generation}/messages?after=0&limit=50
```

会话摘要包含 Pod、模型、源码摘要、开始和结束时间及状态。消息分页按条数和 1 MiB 响应预算截断。界面按需加载，提供继续读取和活跃会话增量轮询。

Pod 丢失后旧会话标记 `interrupted`。新 generation 重新读取同一固定源码并开启新会话，不重放历史对话。累计调用预算和原 deadline 保留。旧 Pod grant 因 generation fencing 失效。未知模型响应仍消费已预留次数。持久提案和回执独立恢复，防止确认丢失引起重复推理或重复接纳。

审计仅承诺已被 Worker 完成并成功提交的消息。进程在消息完成前被杀时，未完成流式片段不作为完成消息补造。提案提交与最终结果由 Runtime 事务写入审计，终态后无需 Worker 补写才能还原结果。

## 初始化与清理

业务数据库和 Runtime 数据库各保留一份全新初始化基线。原型重置不执行数据迁移，不保留 Poll Worker、Source 服务、源码 PVC 或旧下载路由。制品、Registry 对象和独立密钥按部署重置方案保留，设备重新注册。
