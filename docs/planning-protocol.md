<!--
[INPUT]: 固定源码身份、Worker 报告和规划协调器事务。
[OUTPUT]: 定义准备、故障脱敏、累计统计和提案围栏。
[POS]: 控制平面内嵌的规划协调器协议入口。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 规划协调器与临时源码协议 v1

控制平面只保存固定源码清单。每个源码任务、规划 Pod、镜像 Pod 和原生执行任务直接访问 Git 上游。任务目录不作为后续任务的输入。平台持久化完成的 AI 对话、模型预算、计划、构建日志和制品。

## 状态归属

| 数据 | 写入方 | 存储 |
| --- | --- | --- |
| Build、源码任务、固定源码清单、计划接纳、设备执行、制品 | control-plane | 业务 PostgreSQL |
| 运行队列、有效 Pod、累计调用、原 deadline、持久提案 | Control 内的规划协调器 | 规划协调器 PostgreSQL |
| 按 run/generation 排序的完成对话、系统提示、工具调用与结果 | Control 内的规划协调器 | 规划协调器 PostgreSQL |
| Git checkout、Git/LFS 下载目录、认证 HOME | source-preparer / 原生执行进程 | 单任务临时目录 |
| PlanningPool、PlanningRun、SourceTask、ImageBuild | Operator | Kubernetes API |

业务数据库和规划协调器数据库分别初始化。规划协调器使用独立连接池，通过业务 API 获取控制平面的授权与数据。源码清单只保存仓库地址、commit、冻结 tag 对象、LFS OID 与大小。凭据通过凭据引用传入；密钥不进入 URL、CRD 和审计消息。制品和内建 OCI 仓库继续使用独立对象存储。

## 源码任务

构建先进入 `resolving`。控制平面在 `source_tasks` 创建 `resolve` 任务。Operator 直接创建使用 `emptyDir` 的临时 Pod。source-preparer 通过投射令牌和实际 Pod 身份领取仓库地址及短期凭据。Control 核对 Pod→SourceTask 的完整 owner 身份与执行台账，不通过标签授权。

首次解析引用后，任务向控制平面提交 `pinned_ref_oid`。后续重试只能得到第一个已提交 OID。每个仓库的 tag 对象也按任务固定。preparer 直接拉取该对象，解析 submodule 和 LFS，然后提交排序后的 `SourceManifest`。清单摘要为 `sha256(serde_json::to_vec(repositories))`。

控制平面在同一事务中保存清单、切换 Build 为 `planning`、创建唯一规划记录，并由触发器写入规划 outbox。重复提交相同任务结果返回已有结果。不同结果拒绝。成功任务再次领取时返回已提交结果，不再返回凭据。

引用选择使用独立 `refs` 任务。接口只等待引用元数据，不创建 Build，也不保留 Git checkout。任务结束后删除 Pod 和临时卷。

## Kubernetes 规划

1. 规划协调器根据 pool/project 容量准入运行，返回 `run_id`、`generation` 与可选源码身份。
2. Planning Operator 先持久化执行意图，再直接创建 Pod，登记实际 Pod UID、Pod 名和 namespace。
3. source-preparer init container 调用规划协调器 `/v1/prepare`，直接拉取固定源码到 `emptyDir`。认证材料仅位于 init 可见的内存卷。
4. 主 Worker 只读挂载源码和清单元数据。Worker acquire 成功后启动模型 deadline。
5. Pi 通过本地只读工具读取源码。节点能力和失败日志经规划协调器访问控制平面。所有模型请求经过规划协调器预算代理。
6. 任务终态后 Operator 删除 Pod，确认停止并保存证据后移除 finalizer。Pod 丢失时，Operator 报告完整旧执行身份，由规划协调器撤权并决定新 generation。

placement 运行只读节点元数据，不准备源码。源码准备不 acquire 模型租约，不启动模型 deadline。执行登记尚未完成时 `/v1/prepare` 返回 409，preparer 有界重试。Kubernetes API 的 404、409、429、5xx 和网络故障返回 503。无效投射令牌和错误执行身份返回 403。

preparer 配置：

```text
ARTIFACTORY_SOURCE_PREPARE_URL=<planning-url>/v1/prepare
ARTIFACTORY_SOURCE_IDENTITY={"run_id":"...","generation":1}
ARTIFACTORY_SOURCE_ROOT=/workspace/source
ARTIFACTORY_SOURCE_AUTH_ROOT=/run/source-auth
ARTIFACTORY_SOURCE_METADATA=/workspace/source-manifest.json
ARTIFACTORY_POD_TOKEN=<projected token path>
POD_UID / POD_NAME / POD_NAMESPACE
ARTIFACTORY_SERVICE_CA=<optional CA path>
```

`POST /v1/prepare` 输入为 `identity`、`pod` 和 `bootstrap_token`。规划协调器对 audience `artifactory-planning` 执行 TokenReview，核对 SA `artifactory-planning-worker` 以及完整 Pod 身份，再向控制平面 `/internal/planning/source-material` 请求材料。控制平面和规划协调器在材料读取后再次核验 generation、Pod 和准备状态。等待期间发生取消或执行替换时，旧 Pod 无法领取材料。材料返回 `PrepareResponse`；主 Worker 不接触上游凭据。

## 规划协调器接口

Control 在 `/internal/planning` 挂载以下 HTTP 路由。Operator、Worker 与 init container 的基址为 `https://artifactory-gateway-internal:8445/internal/planning`。

`planning-coordinator` 是 Control 内嵌库，不创建独立 Deployment。配置 `ARTIFACTORY_PLANNING_URL` 启用协调器；该值是上述网关基址，`ARTIFACTORY_PLANNING_DATABASE_URL` 指向独立规划数据库。Control、Operator 共享 `ARTIFACTORY_SERVICE_TOKEN`，Control 另使用 `ARTIFACTORY_SERVICE_KEY` 密封规划凭据，并通过 `ARTIFACTORY_CONTROL_URL` 访问业务接口。

| 身份 | 当前名称 |
| --- | --- |
| Kubernetes API | `planning.artifactory.dev/v1alpha1` |
| CRD kind | `PlanningPool`、`PlanningRun`、`SourceTask` |
| 规划 ServiceAccount | `artifactory-planning-worker` |
| 投射令牌 audience | `artifactory-planning` |
| Operator Lease | `artifactory-planning-operator` |
| 池字段 | `PlanningPool.spec.coordinatorUrl` |

Control 的 `/internal/planning/event`、`/internal/planning/tool` 和 `/internal/planning/source-material` 是业务授权桥；下表的 `/v1/*` 属于协调器。两者共享内部网关入口，分别验证服务身份、任务租约或投射令牌。

| 路径 | 身份 | 作用 |
| --- | --- | --- |
| `/v1/intent` | 控制平面服务身份 | 不可变意图或高版本取消 tombstone |
| `/v1/queue` | Operator 服务身份 | 原子准入并返回有效运行 descriptor |
| `/v1/get`、`/v1/register` | Operator 服务身份 | 读取运行和登记 Pod |
| `/v1/execution-failed` | Operator 服务身份 | 以完整执行身份提交 init、Worker 或 Pod 终止 |
| `/v1/execution-lost` | Operator 服务身份 | 以完整执行身份撤销丢失 Pod，或结束被删除的 CR |
| `/v1/prepare` | Kubernetes 投射令牌 | 给 init 提供固定源码材料 |
| `/v1/acquire` | Kubernetes 投射令牌 | 获取 run grant 并启动首次模型 deadline |
| `/v1/heartbeat`、`/v1/report` | run grant | 续租、累计 usage、完成消息审计和安全终态错误 |
| `/model/{run}/{generation}/{uid}/v1/...` | run grant | 原子预留预算并代理模型 SSE |
| `/v1/tool`、`/v1/proposal` | run grant | 转发只读工具、保存提案与读取原回执 |
| `/v1/conversations` | 服务身份及 run/tenant/project | 列出 generation 会话摘要 |
| `/v1/conversation/messages` | 服务身份及 run/tenant/project/generation | 按 sequence 增量分页读取消息 |

Operator 使用服务令牌读取和登记执行。内部网关使用 TLS，客户端仅在显式开发模式下接受明文 HTTP。共享客户端禁止重定向。源码清单上限为 4 MiB；服务 JSON 请求和响应上限为 8 MiB。完成消息超过单事件大小时按 Unicode 安全分片，并使用稳定事件 UUID 顺序持久化。

故障回报包含 `execution` 和固定枚举 `reason`。
`execution` 必须匹配 run、generation、Pod UID、Pod 名与 namespace。
`reason` 仅包含源码退出码、Worker 退出码或 Pod 失败类别。
容器日志和 Kubernetes message 不进入该回报。
规划协调器在同一事务中清除 grant 并结束会话。Worker 已领取租约但无法确认结果时，以退出码 `75` 回报中断。规划协调器在原预算和截止时间内排队下一 generation。其他故障或预算耗尽进入失败终态并写入 outbox。
控制平面消费 outbox 后结束业务构建。
重复回报不重复写入事件，旧身份被拒绝。
已提交提案继续由原回执决定结果，Pod 退出不能覆盖已接受结果。

Worker 通过 `/v1/report` 的 `kind=failed` 和 `payload.error` 提交终态原因。Worker 先将认证失败、调用预算和规划截止时间转换为安全分类。规划协调器保留该原因，替换模型密钥、运行租约及其 JSON 转义值，将控制字符转换为空格，再按 UTF-8 边界限制为 2000 字节。空白或非字符串错误使用 `Pi planning failed`。清理后的错误与失败状态在同一事务写入 outbox，控制平面将其保存为构建和规划运行的错误，构建页面显示该原因。

Worker 失败报告必须通过当前租约、generation 和事件顺序校验。取消、已有终态和待接受提案不能被失败报告覆盖。已提交提案仍由其持久回执决定结果。

每个提案和失败 outbox 事件携带协调器保存的 `generation`、累计 `calls` 和 `usage` 快照。模型代理在发送请求前原子预留次数。供应商返回 4xx 或结果未知时，该次数仍被消耗。控制平面将累计统计投影到规划运行，供 `/api/v1/builds/{id}/plans` 和构建页面读取。待澄清、成功、失败和被拒绝提案都保留已消耗的调用统计。

统计仅更新活动运行。控制平面拒绝低于已接收 generation 的事件，累计次数和有效数字用量不能回退。重复回执不累加次数，取消和已有终态不接收新的统计。提案验证失败不回滚已保存的调用统计。

丢失回报包含 `run_id`、`generation`、`execution` 和 `reason`。`reason=pod_missing` 用于 Pod 消失或开始删除，要求完整执行身份与规划协调器登记完全一致；事务撤销 grant，结束旧会话，并在预算允许时重新排队到下一 generation。重试最多三个 generation，累计调用与原 deadline 不重置。`reason=resource_deleted` 结束被删除 CR 对应的活动任务；仅规划协调器尚未登记执行时允许 execution 为空。已有终态和持久提案不被覆盖。成功恢复后，重复旧 generation 回报被 fencing 拒绝；Operator 重新读取当前运行状态。旧 Pod 必须停止并完成清理后，Operator 才创建新 generation 的 Pod。未知旧 Pod 的停止证据仍由 Operator 保存，不能据此释放实际执行容量。

## 对话与故障恢复

完成的 system、user、assistant、tool 调用和 tool result 消息按 generation 保存。凭据精确值脱敏，代码片段保留。消息不写入可恢复执行状态。浏览器经项目读取权限校验后访问：

```text
GET /api/v1/builds/{build}/planning/conversations
GET /api/v1/builds/{build}/planning/conversations/{run}/{generation}/messages?after=0&limit=50
```

会话摘要包含 Pod、模型、源码摘要、开始和结束时间及状态。消息分页按条数和 1 MiB 响应预算截断。界面按需加载，提供继续读取和活跃会话增量轮询。

Pod 丢失后旧会话标记 `interrupted`。新 generation 重新读取同一固定源码并开启新会话，不重放历史对话。累计调用预算和原 deadline 保留。旧 Pod grant 因 generation fencing 失效。未知模型响应仍消费已预留次数。持久提案和回执独立恢复，防止确认丢失引起重复推理或重复接纳。

审计仅承诺已被 Worker 完成并成功提交的消息。进程在消息完成前被杀时，未完成流式片段不作为完成消息补造。提案提交与最终结果由规划协调器事务写入审计，终态后无需 Worker 补写才能还原结果。

## 初始化与清理

本版只支持全新实例。业务数据库基线为 `2026112101`，规划数据库基线为 `2026111901`。两库分别初始化；程序检测到旧迁移身份时拒绝启动，不执行数据迁移或自动重置。Telemetry 只读授权连接同样验证当前业务基线。

不提供旧 API 路由、CRD group/kind、环境变量或 Helm values 别名。新 Operator 不接管旧 CR 或旧 Pod，旧 Lease 与新 Lease 不构成共同选主。新旧实例必须使用各自的数据库、资源名称和命名空间，不能将旧任务或旧授权接入新实例。

程序和安装脚本不删除旧 Deployment、CRD、Pod、PVC、数据库、对象或密钥。管理员另行备份并退役旧实例；先停止新任务，排空或取消在途任务，确认进程停止和上传票据失效，再按旧实例的资源清单操作。保留旧对象不等于迁入新业务列表，设备须向新实例重新注册。
