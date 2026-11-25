<!--
[INPUT]: 源码复用测试、Kubernetes 1.28 实跑与部署读回。
[OUTPUT]: 记录固定镜像、源码一致性和生产切换边界。
[POS]: 本轮源码准备复用、生产构建与接单恢复的验收边界。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 源码准备与规划复用验收 · 2026-11-25

源码准备复用已实现。隔离原生与混合构建验收通过。
生产组件已分阶段升级，两库追加迁移和资源身份检查通过。
真实模型构建、浏览器与公开制品校验通过。
构建入口已恢复，两个 Gateway Pod 与公开入口均已验证。

## 源码与镜像身份

产品提交为 `6f4352b72b0472fc6bf517b838f5ad442d59bf53`。
冻结源码快照 SHA-256 为 `25f8b121412db905068dc095ac0c852935dc298654decd5ac8a0cbe28fddf896`。
六个产品镜像均来自该快照，按不可变摘要交付。
Control 镜像同时交付内嵌 Coordinator 与 Telemetry。

| 组件 | 镜像摘要 |
| --- | --- |
| Control | `sha256:beec8f6dcd820556581930f377209785601717e25021fbb755c0fb0f48bbafac` |
| Gateway | `sha256:2476b338e093ee9260001fb4916e5276b98217ef5b7c407ecea19ce389656fbe` |
| SourcePreparer | `sha256:e8ed4309de7e12ef191aa7cf50aaf9a52b440f1174ca853d30502e048880f099` |
| Planning Operator | `sha256:a6762fec2bb26282a29d4266994babe0cc12a366a8dfec643a12f3c95c4d5a38` |
| Planning Worker | `sha256:0d5e1ab9991b866a1dda875190aa7f8232a11b7ae76a1e577f71e11f8d381061` |
| Web | `sha256:7584f35f6de9fcc232316f6137f8a74198ada00bda1c7d077f4061d58a54b8a5` |

匿名读取验证了镜像来源、平台与摘要。
六个镜像共 40 个 config/layer 完整 GET、大小与 SHA-256 校验通过。
节点曾报告一层缺失，远端完整读取未复现，节点重试后恢复。
未执行补传，也未变更镜像摘要；该次节点报错的根因未确认。

CRD 补充提交为 `01802a445797dd76601ac4bd6853593d99b5bf96`。
它修正 CRD 生成器和生成 schema 的不可变规则。
运行控制器不调用 CRD 生成函数，六个产品镜像仍使用上述提交。
此补充单独交付，未覆盖产品镜像来源记录。

## 行为变化

发起构建时，轻量 resolve 只解析引用并固定 root commit。
此时不 fetch、checkout 或下载 LFS。
初始 PlanningRun 先建立，Build 保持 resolving。

每个规划 Pod 的普通 init 在最终源码目录完成一次检出。
它保留完整 Git 历史、祖先 tags、嵌套子模块和 LFS 语义。
AI Worker 只读复用同一 `emptyDir`，不再准备第二棵源码树。
本次未引入源码缓存、源码 PVC 或源码 S3。

Coordinator 持久化 candidate 与 outbox。
Control 原子写入清单、Build 绑定和回执。
清单绑定且本代 Execution 本地 ready 后，才允许 acquire。
首次 acquire 启动模型 deadline，准备阶段不消耗模型调用预算。
准备预算为每代 900 秒，最多三个 generation。
替换 Pod 使用同一固定清单，重新证明本代本地 ready。
失败修订建立新的规划 Pod，并重新准备该 Pod 的源码。
本轮没有跨 Pod 或跨规划轮次共享源码目录。

后续原生与镜像任务按固定清单独立检出。
过程页面读取持久化源码阶段和当前 Execution。
缺失事实时不推断 ready。
AI 审计仅在计划之后的独立标签展示。
接口和运行约束见[规划协议](../planning-protocol.md)与[Operator 运维](../operator-operations.md)。

## 本地与自动化验证

| 范围 | 结果 |
| --- | --- |
| Control | lib 120 项；integration 169 项均有通过覆盖 |
| Coordinator | 数据库 34 项、baseline 2 项、lifecycle 3 项、auth 1 项 |
| Planning Operator | 23 项 |
| Source checkout/preparer | 源码链路 20 项及 prepare 回归 |
| Gateway、Telemetry | 8 项、2 项 |
| Planning Worker | 39 项 |
| Web | TypeScript 通过；63 项浏览器回归通过 |
| 镜像安装夹具 | 25 项离线回归通过 |
| 静态检查 | 变更 crate 的全目标 clippy `-D warnings`、fmt、模块与数据访问边界、diff 通过 |

Web 回归覆盖双语、明暗主题、三个视口、320px 和 axe。
自动化页面使用测试夹具，不能代替生产页面验收。

Control integration 首轮有一项终端负载失败。
宿主负载下降后，该单项独立通过。
本轮未降低阈值，未为通过测试修改终端产品代码。

审阅修正了迟到观测和取消等待锁的时间围栏。
终态前产生的观测只补历史，不推进业务状态。
旧代事实不覆盖当前代摘要。
完成时间采用取得业务锁后的真实时钟时间。
真实数据库回归先复现拒绝，再验证三种观测可补齐且状态保持 canceled。

安装夹具补齐 BuilderPool 的 `get` 权限和 Gateway 网络身份标签。
回归执行实际安装函数并渲染 Registry 网络策略。
分别移除权限或标签后，对应回归检出缺陷。
生产原有权限和标签已满足要求，没有放宽生产网络策略。

## Kubernetes 1.28 CRD 验证

验证版本为 Kubernetes `v1.28.3`。
原 CEL 整对象比较在 spec 含 null 时误拒绝元数据更新。
修正后逐项比较七个 spec 字段，并显式比较可空字段的存在性和值。

26 项身份不可变矩阵与 31 项兼容矩阵全部通过。
覆盖 pending、placement、fixed、旧资源缺省、null/缺省切换、finalizer 与 `/status`。
身份变化均拒绝，等价身份的元数据更新均接纳。
测试使用的临时 CRD 已删除，生产 CRD UID 保持。
此结果不推断其他 Kubernetes 版本的运行验收。

## 隔离原生链路

隔离命名空间使用独立数据库和存储。
模型端为确定性夹具，执行链为真实 Pi、Control 与 Rust Emissary。
该结果不替代生产模型供应商验收。

构建 ID：`10455e1f-644b-4b35-a3ec-4bc14a6266f0`。
清单摘要：`188204294f0b62f9d64210826acb4713e622b0627f696cdc0b4a40443fd0f400`。
最终回执 `passed: true`，清理错误为空。

- 轻量解析只保存 root commit；准备阶段 calls=0、deadline=null。
- init 最终目录含两层嵌套子模块与已还原 LFS。
- 清单绑定和本代 local-ready 均早于首次 acquire。
- init 退出后开始推理；源码只读且凭据隔离。
- 真实原生构建成功，公开 Gateway 的 GET、HEAD、Range 与制品摘要一致。
- Operator leader 删除后完成选举；Pod 替换保留 deadline 与已消耗调用数。
- 第二代重新核验同一清单，不复用旧代 ready。
- 取消撤销授权并停止 Pi；Runtime 重启保留取消和预算。
- 终态无任务 Pod、源码凭据 Secret 或任务 PVC；历史、会话和制品保留。

## 隔离原生与镜像混合链路

构建 ID：`51b19dec-f218-471e-952a-2d89a1bce050`。
清单摘要：`033d57c0fc33815e864a7e896fad4c10e614a985dfa2a99ab7b3e84f6df0854c`。
最终回执 `result: passed`，清理错误为空。

原生与镜像任务均成功，并保持规划阶段清单身份。
镜像 init 的运行摘要匹配新版 SourcePreparer。
Source、BuildKit 与 Worker 均以 0 退出。
源码只读、临时目录与 BuildKit 凭据隔离均通过。

原生证明制品的三个仓库 commit 和 LFS SHA-256 与清单一致。
专用 Release 发布后，完整 OCI 图、config、全部 layer 摘要、HEAD 与 Range 校验通过。
镜像中读取到已还原 LFS 和嵌套子模块字节。
真实 BuildKit RUN 标记为 `executed-linux-amd64`。
OCI 根摘要为 `sha256:9e089ce78ffa3cff1a60456fe3f2822ce4aab62fb1e565561af521110268f723`。
临时读取 Token 已撤销，任务材料已回收。

构建成功后的首次验收误把容器配置 ID 当作运行镜像摘要。
脚本改用 `image_id`，并复核已有构建，没有再次发起构建。
早先的 DNS、权限和网络标签失败记录保留，不改写为通过。

## 生产切换

生产版本为 Kubernetes `v1.28.3`。
按[Helm 切换流程](../helm.md)先升级 Gateway，再暂停接单并排空。
逐 Gateway Pod 验证构建、重试和澄清返回 503 与 Retry-After 60。
读取和取消路径仍可达 Control。
业务、Coordinator、任务 Pod 与 CR 的活动计数均为零后，才备份两库。
两份备份均完成目录读取、完整解码与 SHA-256 校验。

更新 CRD 后，依次升级 Planning Operator、两个池的 SourcePreparer、Control、Telemetry 和 Web。
PlanningPool 同时升级 Worker。
Control 与 Coordinator 两库均追加 `2026112501`，没有重置原迁移基线。
Helm 收敛仅允许固定镜像、池内 SourcePreparer 和接单字段差异。
暂停阶段 revision 为 artifactory 33、planning-operator 6、image-operator 3。

`2026-11-25T05:32:13Z` 的技术读回通过：

- Deployment 全部就绪；目标组件运行镜像摘要与固定交付一致。
- 两库迁移身份和两个池镜像正确。
- 原 Namespace、PVC、Secret、Service 与池资源的成员和 UID 保持。
- 非 Helm Secret 集合完全一致，逐项 UID 和内容摘要保持。
- 原 BuilderPool 的 workerImage 和 buildkitImage 保持。
- 入口仍暂停，活动计数全为零。

## 生产真实模型与自主修订

专用项目使用独立 Release，未覆盖正式版本。
[查看真实构建](http://10.169.1.239:30820/tenants/ai-workspace/projects/source-reuse-acceptance-20261125133222-a6b5a3/releases/eaf3ed4d-2eed-42c5-bfed-984abd3dd9a1/builds/1)。

构建 ID 为 `033fb3d2-8cb4-4a5a-8403-3fe4f14c8b35`。
固定源码 commit 为 `d56401a5c86547db0d27456b4737b478fde9376c`。
清单摘要为 `e1acb7db26cdd53ba3959a481bbd1e1cbe4cde4db7a6ecb62c72133a82fa60d5`。
最终状态为 succeeded，耗时 7 分 26 秒。

模型为真实供应商的 `qwen4-max`。
首次规划保存 56 条记录，失败修订保存 9 条记录。
两轮均成功，使用相同的固定源码清单。
首次目录 ready 为 `05:32:45.925Z`，AI 开始为 `05:33:21.854Z`。
修订使用新的规划 Pod，重新准备并确认本地 ready。
后台日志保存 584 个 chunk。
原生执行设备为已授权的 `codehubdeMac-mini.local`，darwin/arm64。

首轮实际构建成功，验证脚本在签名输出管道退出 141。
模型读取失败日志并识别 `pipefail` 与 `grep -q` 引发 SIGPIPE。
模型将完整输出捕获到变量，再用 Bash 模式匹配验证。
摘要、架构、版本、源码零改动及大小上限检查保持。
平台接纳修订后，构建与验证均成功，无人工修改计划。

三个制品经公开 Gateway 全量下载，并核对大小与 SHA-256：

| 制品 | 字节数 | SHA-256 |
| --- | ---: | --- |
| SHA256SUMS | 194 | `768f29e3a5f710b37430d226ce9f5e1e8fb3bf722798d0d3bc568c6849bc57cc` |
| manifest.json | 461 | `4100ec7bf81b2371b63a415358602e2cd1184f5a10e574587f60ee64ea8e6bb6` |
| tasknotes-macos-arm64.zip | 258147 | `4c183cc395402b72b224ee26b71803df189b3ea7bf9f37a8b139b2bf628613b2` |

该构建验证原生执行、ad-hoc 产物及源码复用流程。
私有验收请求未传 application_platform，冻结合同该字段为空。
模型先提交 macos 分类被拒，之后按冻结合同提交空值。
该请求与 macOS 应用目标存在分类不一致，诊断记录保留。
此结果不作为 macos 应用分类、托管签名或公证准入验收。
下一次应用分类验收需显式传入 macos，保留现有构建合同。

## 生产浏览器验收

已操作真实生产页面，独立于自动化夹具：

- 过程页展示六个真实源码步骤、Pod、generation、ready 与 AI 开始时间。
- 标签顺序为过程、执行、计划、AI 审计、制品。
- 计划页不重复展示审计。
- 首次审计可查看 VERSION 的实际读取结果、参数与原始记录。
- 修订审计可查看读取失败日志、SIGPIPE 分析与已接纳计划。
- 执行页订阅真实设备日志，制品页展示三项成功产物。

私有证据保存过程、审计、修订分析和制品截图。
截图与浏览器回执按摘要留存，不包含登录凭据。

## 恢复接单与最终读回

恢复前再次确认业务、Coordinator 和任务资源均已排空。
功能回执核对隔离链、生产模型、浏览器和技术证据后生成。
首次 Helm 恢复因接单字段归属冲突失败，revision 34 记录保留。
按带 UID/resourceVersion 前置条件的补丁恢复接单字段。
随后 Helm 收敛成功，没有使用强制覆盖或旧镜像回滚。
最终 revision 为 artifactory 35、planning-operator 6、image-operator 3。
Registry 仍为 revision 1，四个 Release 均为 deployed。

`2026-11-25T05:46:38Z` 至 `05:46:52Z` 最终读回确认：

- 所有 Deployment 完全就绪，六组件固定镜像和运行摘要正确。
- 两库迁移、池镜像与原资源集合及 UID 保持预期。
- 非 Helm Secret 集合、逐项 UID 与内容摘要保持。
- 原 BuilderPool workerImage/buildkitImage 保持。
- 45 个保护资源的完整配置仅存在准确授权字段差异。
- 网络、RBAC、存储和其他组件配置保持。
- Gateway 接单暂停值为 false，活动任务计数为零。

两个 Gateway Pod 分别验证构建、重试、澄清路径不再返回暂停 503。
读取、取消和健康路径均可达。
公开入口 healthz 返回 200，控制台返回 200。
未认证 API 返回 401；无 CSRF 的虚构构建 POST 返回 403。
这些探测未创建额外构建，且均无 Retry-After。
生产真实成功构建在暂停期间经内部授权入口发起。

迁移后禁止只回滚旧 Control、Coordinator 或 SourcePreparer。
恢复旧协议需再次暂停、排空，并协调两库与整套版本。

## 隔离资源清理

两个隔离命名空间按固定 UID 和资源版本回收。
先核对全部构建终态并保存脱敏资源清单与证据摘要。
仅删除本轮命名空间及对应 TokenReview ClusterRole/Binding。
保留标准 finalizer，使用正常回收，不删除生产或全局 CRD。
`2026-11-25T05:48:19Z` 读回确认两个命名空间均不存在。
四个对应 ClusterRole/Binding 均不存在。
命名空间清单、历史失败与成功证据及镜像回执保留在私有目录。
