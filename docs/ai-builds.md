# 零侵入 AI 构建

<!--
[INPUT]: 读取源码清单、临时 Worker 与 Emissary 的执行契约。
[OUTPUT]: 定义仓库接入、AI 规划、构建与内部发布流程。
[POS]: 用户构建指南，连接控制台操作与运行时约束。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

节点启动元数据、资源查询 MCP、有序候选分配与有限重选的启用方式见 [AI 选机](node-placement.md)。选机开关按构建固定，旧任务继续使用原调度方式。

新构建只需仓库引用、构建目标和可选的平台约束。原仓库只读接入，平台不添加 `artifactory.yaml`、`.pi` 或 CI 文件，不创建分支、提交、标签或推送。构建说明、分析记录、执行计划及外置脚本保存在 Artifactory。

仓库支持 HTTPS、SSH 和用户指定的 HTTP 地址。HTTP 入口用于没有 HTTPS 服务的内网 Git。HTTP/HTTPS URL 禁止携带用户名、密码、查询参数或片段；认证信息使用独立凭据引用。相对子模块保留父仓库协议，只有协议、主机和端口一致时才继承父仓库凭据。

## 对话式发起构建

首页、项目详情及构建列表的“发起构建”统一进入 `/tenants/{tenant}/projects/{project}/builds/new`。当前项目已预选，引导按「源码 → 目标与平台 → 确认」三步收集信息。返回编辑保留答案，配置摘要始终可回看；切换项目时按现有未保存保护处理草稿。

- 源码可搜索真实分支、标签，或显式切换为手动输入引用与 commit。分支、标签提交完整引用以区分同名条目。查询失败提供刷新与手动输入，不自动使用 `HEAD`，也不猜测默认分支。
- 目标支持常用模板和自由描述。平台默认由 AI 根据源码及授权节点能力选择，也可手动多选；无需在引导中选择模型、凭据或具体节点。
- 默认只构建。最终确认页可开启内部 Release 并填写版本，或展开高级参数 JSON；参数名称与字符串值遵循已有执行参数约束。
- 只有最终确认提交才调用创建构建接口。引导本身不调用模型，不拉取源码；提交后仍执行原有源码固定、AI 规划、澄清、执行及发布流程。列表中的 OID 是预览，不是提前固定的源码清单。
- 草稿只在页面内存中保留；刷新、取消或离开后不恢复。语言、主题及动效切换不清空草稿。提交失败保留答案，提交期间禁止重复操作，不自动重发请求。

`GET /api/v1/projects/{id}/refs` 使用项目读取权限与该项目已配置的仓库凭据，通过只读 `git ls-remote --symref` 返回：

```json
{"default_ref":"refs/heads/main","branches":[{"name":"main","ref":"refs/heads/main","oid":"<object-id>"}],"tags":[]}
```

默认分支无法识别或尚不存在时 `default_ref` 为 `null`。注释标签的 `^{}` 条目不重复展示，标签 `oid` 是标签本身的对象 ID。查询最多 30 秒、输出最多 4 MiB；超限明确失败，不返回截断列表。未知项目返回 404，无读取权限返回 403，上游或格式错误返回 502，超时返回 504，错误使用脱敏的 `{ "error": "..." }` 响应。该接口不创建构建或源码清单，不修改仓库引用。

引用查询复用项目凭据和主机信任，但不继承控制平面用户的 askpass、SSH agent、默认身份或 `.netrc` 凭据。未配置仓库凭据时仅允许匿名访问；私有仓库查询失败时由用户刷新或明确选择手动输入，不尝试其他身份。

## 配置与运行

AI 规划只使用 Kubernetes Job。独立 Agent Operator 创建 Pi Job，Runtime 保存模型配置、累计预算、截止时间与审计记录。控制平面校验计划后，向原生 Emissary 或镜像 Operator 派发构建。

1. 安装主 Chart、Agent Operator、Runtime 和 Image Operator。配置 AgentPool 的 `sourcePreparerImage`、`controlUrl`、规划镜像和临时存储限制。
2. 在“访问与安全 → AI 规划”新增模型配置。支持 OpenAI Chat Completions、Responses 和 Anthropic Messages。项目可以覆盖平台默认配置。
3. 注册并授权原生设备，或配置集群的 amd64/arm64 构建节点。任务通过绑定 Pod 的投影令牌领取材料，无需注册常驻规划 Worker。

模型密钥使用主密钥加密落库。Pi 通过 Runtime 模型代理调用供应商，供应商密钥留在 Runtime。源码凭据只提供给准备进程，不能写入 CR、Job、仓库 URL、主容器或 AI 会话。

Compose 供服务开发、控制台和鉴权验证。完整构建链需要 Kubernetes API、任务 ServiceAccount 和 Operator。模型连通性检查应在 Runtime 所在网络执行。

## 临时源码

提交构建后，控制平面创建临时 SourceTask。准备进程直接访问上游，以 CAS 固定引用对象和标签，再解析 commit、递归子模块 gitlink 和 LFS OID。完整 SourceManifest 与首次 PlanningRun 在同一事务提交。平台只保存 Manifest 元数据，不保存 Git bundle、源码包、仓库 checkout 或 Git/LFS 缓存。

分支、标签和子模块 URL 在准备后固定。所有规划重试、目标架构和人工重试读取同一 Manifest。上游拒绝获取原对象时明确失败，不能重新跟随分支或替换标签。Manifest 包含路径、上游 URL、commit、标签对象与 LFS OID/大小，摘要按排序后的仓库列表计算，元数据总量最多 4 MiB。

Kubernetes 规划和镜像 Job 使用 init container，将源码拉入任务专属 `emptyDir`。准备进程使用独立凭据卷与投影令牌；主容器只读取准备好的源码。TokenReview、Pod UID 和真实 Pod → Job → CR 所有者链共同确定任务身份，不能仅凭标签领取材料。

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

Pi 工具仅能列目录、读文件、搜索、读取 Git 元数据、节点能力和失败日志、提交计划或请求补充。不提供编辑或通用 Shell，不加载仓库 Pi 配置、插件或钩子。试运行统一在 Rust Emissary 执行。每轮默认 10 分钟、20 次模型调用，管理员可调整；可通过 `requests_per_minute` 限制调用速率。Runtime 持久化调用预算、用量、截止时间与 proposal。Worker 丢失后增加 generation，开启新的模型会话，不回放旧 checkpoint，不重置累计预算。

`ExecutionPlan v1` 包含摘要、实际文件依据、外置脚本和经过现有 Recipe 验证的目标/步骤/制品规则。平台固定源码清单、模型配置版本及计划版本；attempt 引用自己的计划，历史版本不可覆盖。脚本位于 `ARTIFACTORY_SCRIPT_DIR`，命令工作目录为 checkout，`ARTIFACTORY_WORK_DIR` 为其父 attempt 目录。

有效计划自动执行。执行失败最多自动修订两次，共三轮；只修订失败目标，成功目标的执行与制品保留。修订不能改变 commit、目标集合、平台或扩大签名/节点池权限。源码凭据、模型认证及权限错误直接失败。所有阶段都可取消，提交计划与取消通过服务端事务排序，过期结果不能排队。

人工重试复用原提交及该目标原计划；改变目标需新建构建。发布必须由有 release 权限的人显式请求，只收集每个成功目标最终有效 attempt 的制品。失败/旧轮次产物不能混入发布。

## 对话与审计

Runtime 按 run 和 generation 保存会话。完成消息在继续执行前获得持久化确认；异常中断保留已确认消息。大消息按块传输，proposal 与接受回执由 Runtime 同一事务登记。日志、执行计划和制品继续持久化，对话引用的源码片段随对话保留。

控制台在构建规划页按需加载会话和消息。公开接口为 `GET /api/v1/builds/{id}/planning/conversations` 和 `GET /api/v1/builds/{id}/planning/conversations/{run}/{generation}/messages?after=&limit=`。两者都检查当前租户与项目读取权限，不随构建详情一次性返回完整对话。

## 验收

完整构建验收使用 `tests/operator/acceptance/run.py`，在隔离 Kubernetes 命名空间验证临时源码、Pi、原生构建和制品下载。真实供应商验收显式提供 `--real-model-env`，私密配置与证据保存在忽略目录，不能上传整个目录。

本次原型切换使用新业务与 Runtime 初始化基线，不提供旧数据库迁移或 Poll 兼容路径。重置前阻断写入口并停止旧任务，等待在途票据失效。保留 S3/Registry 物理对象、PKI 与主密钥；旧列表和授权不恢复，设备重新注册。

跨平台编译不能替代真实 macOS、Windows 或双架构构建验收。实际执行范围记录在验收文档中。
