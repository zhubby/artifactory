# 零侵入 AI 构建

<!--
[INPUT]: 读取源码、模型协议、项目设备和执行契约。
[OUTPUT]: 定义模型兼容、目标准入、失败分类和构建发布流程。
[POS]: 用户构建指南，连接控制台操作与运行时约束。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

节点启动元数据、资源查询 MCP、有序候选分配与有限重选的启用方式见 [AI 选机](node-placement.md)。选机开关按构建固定，旧任务继续使用原调度方式。

新构建只需仓库引用、构建目标和可选的平台约束。原仓库只读接入，平台不添加 `artifactory.yaml`、`.pi` 或 CI 文件，不创建分支、提交、标签或推送。构建说明、分析记录、执行计划及外置脚本保存在 Artifactory。

仓库支持 HTTPS、SSH 和用户指定的 HTTP 地址。HTTP 入口用于没有 HTTPS 服务的内网 Git。HTTP/HTTPS URL 禁止携带用户名、密码、查询参数或片段；认证信息使用独立凭据引用。相对子模块保留父仓库协议，只有协议、主机和端口一致时才继承父仓库凭据。

## 对话式发起构建

项目先创建 Release，填写严格 SemVer 与版本说明。项目侧栏的“版本”进入版本列表。Release 工作台包含概览、构建记录和交付物。

构建只能从草稿 Release 发起。构建向导显示固定版本，并独立选择分支或 Tag、构建要求、完整目标和参数。每次新构建独立解析并固定 SourceManifest；同版本构建可以采用不同源码和目标。Release 已封存后不能新建或重试构建。版本说明不充当 AI 构建指令，修改说明不影响构建。

向导通过 `GET /api/v1/projects/{id}/build-platforms` 读取项目获授权的原生设备平台。每个条目包含 `os`、`arch`、`device_count` 和 `online_count`。缺少对应系统或架构设备的原生目标无法选择；默认优先选择有在线设备的平台。离线、忙碌和暂时排空设备仍计入设备数量，吊销设备不计入。提交构建时，服务端再次检查每个原生目标，防止设备授权变化后继续创建。镜像目标不使用此原生设备列表过滤。

`GET /api/v1/projects/{id}/refs` 使用项目读取权限与该项目已配置的仓库凭据，通过只读 `git ls-remote --symref` 返回：

```json
{"default_ref":"refs/heads/main","branches":[{"name":"main","ref":"refs/heads/main","oid":"<object-id>"}],"tags":[]}
```

默认分支无法识别或尚不存在时 `default_ref` 为 `null`。注释标签的 `^{}` 条目不重复展示，标签 `oid` 是标签本身的对象 ID。查询最多 30 秒、输出最多 4 MiB；超限明确失败，不返回截断列表。未知项目返回 404，无读取权限返回 403，上游或格式错误返回 502，超时返回 504，错误使用脱敏的 `{ "error": "..." }` 响应。该接口不创建构建或源码清单，不修改仓库引用。

引用查询复用项目凭据和主机信任，但不继承控制平面用户的 askpass、SSH agent、默认身份或 `.netrc` 凭据。未配置仓库凭据时仅允许匿名访问；私有仓库查询失败时由用户刷新或明确选择手动输入，不尝试其他身份。

## 配置与运行

AI 规划只使用 Kubernetes Pod。独立 Planning Operator 创建 Pi Pod，规划协调器保存模型配置、累计预算、截止时间与审计记录。控制平面校验计划后，向原生 Emissary 或镜像 Operator 派发构建。

1. 安装主 Chart、Planning Operator 和 Image Operator。主 Chart 的 Control 内嵌规划协调器；配置 PlanningPool 的 `sourcePreparerImage`、`controlUrl`、规划镜像和临时存储限制。
2. 在“访问与安全 → AI 规划”新增模型配置。支持 OpenAI Chat Completions、Responses 和 Anthropic Messages。项目可以覆盖平台默认配置。
3. 注册并授权原生设备，或配置集群的 amd64/arm64 构建节点。任务通过绑定 Pod 的投影令牌领取材料，无需注册常驻规划 Worker。

模型名称由供应商定义，可填写私有别名。平台按接口协议和显式能力构造请求，不按模型名称推断能力。Chat Completions 的“接口兼容性”设置对应 `parameters.compat`：

| 参数 | 可选值 | 默认值 |
|---|---|---|
| `system_role` | `system`、`developer` | `system` |
| `thinking_format` | `none`、`reasoning_effort`、`enable_thinking`、`thinking` | `none` |
| `max_tokens_field` | `max_tokens`、`max_completion_tokens` | `max_tokens` |

三个参数独立选择。`thinking_level` 定义思考级别；`thinking_format` 定义供应商接受的参数格式，`none` 不发送思考参数。旧 Qwen 配置若需启用思考，必须显式选择 `enable_thinking` 并设置非 `off` 的思考级别。例如：

```json
{
  "thinking_level": "medium",
  "compat": {
    "system_role": "system",
    "thinking_format": "enable_thinking",
    "max_tokens_field": "max_tokens"
  }
}
```

Responses 和 Anthropic Messages 使用各自协议，不接受 `compat`。兼容配置仅接受表中的键和字符串枚举，不能注入认证或端点参数。修改模型配置后，新构建固定新的配置版本。

模型密钥使用主密钥加密落库。Pi 通过规划协调器模型代理调用供应商，供应商密钥留在规划协调器。源码凭据只提供给准备进程，不能写入 CR、Pod、仓库 URL、主容器或 AI 会话。

Compose 供服务开发、控制台和鉴权验证。完整构建链需要 Kubernetes API、任务 ServiceAccount 和 Operator。模型连通性检查应在规划协调器所在网络执行。

## 临时源码

每次构建创建独立临时 SourceTask。并发构建分别解析自己的引用。准备进程直接访问上游，以 CAS 固定引用对象和标签，再解析 commit、递归子模块 gitlink 和 LFS OID。完整 SourceManifest 与首次 PlanningRun 在同一事务提交。平台只保存 Manifest 元数据，不保存 Git bundle、源码包、仓库 checkout 或 Git/LFS 缓存。

分支、标签和子模块 URL 在准备后固定。同一次构建的所有规划重试、目标架构和人工重试读取同一 Manifest。新构建重新解析所选分支或 Tag。上游拒绝获取原对象时明确失败，不能重新跟随分支或替换标签。Manifest 包含路径、上游 URL、commit、标签对象与 LFS OID/大小，摘要按排序后的仓库列表计算，元数据总量最多 4 MiB。

Kubernetes 规划和镜像 Pod 使用 init container，将源码拉入任务专属 `emptyDir`。准备进程使用独立凭据卷与投影令牌；主容器只读取准备好的源码。TokenReview、Pod UID 和真实 Pod → CR 所有者链共同确定任务身份，不能仅凭标签领取材料。

原生 macOS、Windows 和 Linux Emissary 在每次 attempt 创建临时目录，经 mTLS 和有效租约领取材料，并直接从上游拉取。每次任务隔离可写的 HOME、TMP 和包管理器缓存，继续使用预装工具链与 SDK。进程树退出后删除源码与临时凭据；启动时清扫中断任务，清理失败暂停接单。

每步执行前校验已跟踪文件、固定提交、标签、子模块和 LFS 内容，变化以 `source_modified` 终止。允许未跟踪的生成文件。Windows 不比较 Unix 执行权限位，其余内容校验保持一致。

BuildKit 不导入或导出 Registry 构建缓存。最终镜像作为制品保存；源码工作目录和中间缓存随任务清理。S3 仅保存日志与制品，不提供源码下载接口。统一 Gateway 继续反向代理内建仓库和制品下载。

## 规划、执行与修订

```text
resolving → planning → queued → running → succeeded
               ↓                    ↓
          needs_input          replanning → queued
                                    ↓
                                  failed
```

Pi 工具仅能列目录、读文件、搜索、读取 Git 元数据、节点能力和失败日志、提交计划或请求补充。不提供编辑或通用 Shell，不加载仓库 Pi 配置、插件或钩子。试运行统一在 Rust Emissary 执行。每轮默认 10 分钟、20 次模型调用，管理员可调整；可通过 `requests_per_minute` 限制调用速率。规划协调器持久化调用预算、用量、截止时间与 proposal。Worker 丢失后增加 generation，开启新的模型会话，不回放旧 checkpoint，不重置累计预算。

规划开始时，模型收到本任务的调用上限和时限。每轮工具返回后显示剩余预算。剩余三次调用时，提示提交计划或提出必要澄清，预留校验修正机会。

构建页面在首个提案提交前读取实时规划摘要。最新活动运行显示协调器状态、generation、已预留调用次数和数值用量。业务已确认统计在独立字段保留。取消、终态和待澄清运行只显示已确认值。实时查询不可用时，页面保留业务记录并提示实时状态不可用。

审计列表使用本地化消息类型，以及文本首句、工具名或提案结果形成短摘要。展开后保留完整 JSON，分片分页读取不变。

`ExecutionPlan v1` 包含摘要、实际文件依据、外置脚本和经过现有 Recipe 验证的目标/步骤/制品规则。平台固定源码清单、模型配置版本及计划版本；attempt 引用自己的计划，历史版本不可覆盖。脚本位于 `ARTIFACTORY_SCRIPT_DIR`，命令工作目录为 checkout，`ARTIFACTORY_WORK_DIR` 为其父 attempt 目录。

有效计划自动执行。执行失败最多自动修订两次，共三轮；只修订失败目标，成功目标的执行与制品保留。修订不能改变 commit、目标集合、平台或扩大签名/节点池权限。源码凭据、模型认证及权限错误直接失败。所有阶段都可取消，提交计划与取消通过服务端事务排序，过期结果不能排队。

原生目标与镜像目标共用修订预算。镜像 Worker 用 `failure_kind` 报告正式失败类型；只有当前源码、当前目标计划的最新构建执行被分类为 `execution` 时才触发修订。规划器读取对应镜像执行 ID、错误与平台日志。Dockerfile 解析、RUN 退出和本地 COPY 输入错误属于执行失败；BuildKit 自身的认证、网络传输、镜像拉取或推送、基础设施及取消不消耗修订次数。RUN 内程序因网络故障非零退出，仍属于执行失败。旧 Worker 未分类的失败保持未知，不自动修订。外仓分发失败不进入构建修订。

构建详情按目标执行方式显示进度。原生目标显示节点、attempt、步骤和日志。镜像目标复用镜像执行卡片，显示当前状态与平台构建日志入口，不等待原生节点。源码解析或规划终止且未创建任务时，执行页不显示节点匹配。计划页分别说明正在规划、等待回答、失败或取消。

人工重试复用原提交及该目标原计划。改变源码或目标需在草稿 Release 内创建新构建。发布在 Release 工作台显式选择一次完整成功构建。失败和旧轮次产物不能混入交付。发布冻结、命名和镜像不可变约束见[项目版本交付](release-delivery.md)。

## 对话与审计

规划协调器按 run 和 generation 保存会话。完成消息在继续执行前获得持久化确认；异常中断保留已确认消息。大消息按块传输，proposal 与接受回执由规划协调器同一事务登记。日志、执行计划和制品继续持久化，对话引用的源码片段随对话保留。

控制台在构建规划页按需加载会话和消息。公开接口为 `GET /api/v1/builds/{id}/planning/conversations` 和 `GET /api/v1/builds/{id}/planning/conversations/{run}/{generation}/messages?after=&limit=`。两者都检查当前租户与项目读取权限，不随构建详情一次性返回完整对话。

## 验收

完整构建验收使用 `tests/operator/acceptance/run.py`，在隔离 Kubernetes 命名空间验证临时源码、Pi、原生构建和制品下载。真实供应商验收显式提供 `--real-model-env`，私密配置与证据保存在忽略目录，不能上传整个目录。

本版只支持全新实例，不提供旧数据库迁移或 Poll 兼容路径。业务与规划数据库使用新基线，程序拒绝旧库；安装不重置旧库，也不删除旧部署。旧实例的备份、停写与退役由管理员另行执行，详见[初始化与清理](planning-protocol.md#初始化与清理)。

跨平台编译不能替代真实 macOS、Windows 或双架构构建验收。实际执行范围记录在验收文档中。
