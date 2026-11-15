# 租户与项目工作台 Kubernetes 交付记录

日期：2026-11-15，Asia/Shanghai。范围为 Kubernetes `1.28.3` 的 `artifactory` 命名空间。

## 实现

功能实现：`366e325a6bd21bb720bf5c30bf33692e37c9556d`；生产后台消费修复与最终运行镜像源码：`dc1594baeddf33c45d0233d83cb81b84a380a003`。实现已快进合入并推送 main，核对本地、远端跟踪与实时远端引用一致。

提供 `/tenants` 租户大厅、租户项目首页和项目工作台；构建、创建、制品、发布与管理均进入项目路径。顶部两个选择器支持搜索，切换项目保留列表栏目，从详情、创建或管理页切换则返回目标项目首页。旧页面直接返回 404，不增加兼容路由。改名保持原有 slug，构建详情继续使用项目内编号。

保留单侧栏，窄屏使用具备焦点约束的抽屉；右下角个人入口提供资料、安全与偏好覆盖面板。语言、主题和动效立即生效，不卸载业务页面。请求按空间取消并拒绝迟到响应，未保存表单在离开和浏览器历史跳转时确认。

账号接口仅接受 Session，写入要求 CSRF。显示名称持久化，用户名只读；改密校验当前密码并注销全部会话，登录和改密通过账号行锁与已验证凭据复核，避免生成仍有效的旧凭据会话。

新增生命周期状态、账号级操作、幂等键、排空、租约、检查点和标识墓碑。归档与删除关闭新任务入口，等待已接受工作完成，不自动取消工作。删除先保存清单，再依次清理 Source、Runtime、Telemetry、对象与 Control 记录；外部 I/O 不持有数据库事务锁。等待阶段可取消，清理开始后只能查询或重试。共享对象保留其他引用，上传授权到期与迟到写入宽限后复核。恢复租户保留单独归档的项目。

## 本地验证

- Rust workspace 库和二进制单元测试：44 通过、3 项数据库或性能测试按声明忽略；忽略项不计作已运行的数据库验收。
- 全仓 fmt、Clippy `--locked --all-targets -- -D warnings`、数据访问依赖检查、35 项 Python 脚本测试及版本契约通过。
- Control 数据库完整首轮 90 项通过、1 项失败；修正 Source refs 错误文案后，对应回归通过。共享发布对象并发保留、账号并发改密、排空与跨租户隔离等定向回归通过；这不是一次完整数据库运行全绿。
- 生命周期生产启动回归首先复现任务停留在 waiting，再补充独立自动消费循环并通过真实数据库回归；该修复经过 Control 全目标 Clippy 和 fmt。最终源码 CI 另通过全仓 Clippy。
- Source 使用真实 PostgreSQL 和 S3 清理；Runtime 使用真实数据库验证退休；Telemetry 使用真实 TimescaleDB、列存及 45 天聚合验证删除、迟到回放和无关数据保留。实际对象存储故障后重试清理、租约到期接管和旧 generation 拒绝写入均有数据库回归证据。
- Next 生产构建、TypeScript 通过。UI 完整轮次为 233 通过、3 失败、1 跳过；修复等待时序、手机管理页 fieldset 最小宽度及翻译键后，27 项受影响场景全部通过。全部 236 个可执行场景有通过证据，桌面抽屉场景按条件跳过；不是单次完整轮次全绿。
- 文档浏览器测试 10 项通过。视觉矩阵覆盖桌面、平板、手机、中英双语与深浅主题，共 72 张工作台截图；减少动效、实际 WebGL 暂停与恢复另有测试。

## 不可变镜像

[deployment-images 43685870523](https://github.com/zhubby/artifactory/actions/runs/43685870523) 成功。核对 GHCR 与集群镜像端点的索引、amd64 manifest、config 及 OCI 源码 SHA，并在三个节点完成九个临时 Pod 的实际预拉取。临时预拉取 Pod 已删除。

| 组件 | 索引摘要 |
| --- | --- |
| Control Plane / Source / Telemetry | `sha256:02dd05787bf672021f4a0608df9a565b3a5ddf7b36febb10bd684d62b7a4ae3b` |
| Agent Worker | `sha256:0d5af769c04370ba56a43f3e5aebafa33d16ada6d74688ba18efa094aac7768a` |
| Web | `sha256:e0821d56879f03f8d2e4db90e6c47e5017f0e4f63c7aea56d0df615d5a808f9b` |

所有引用同时携带完整源码 SHA 标签与摘要。Helm 渲染前后只改变五个第一方工作负载的镜像引用，保持三个 Control 副本和其余配置。现场核对七个第一方运行容器的实际 image ID，均对应最终源码。

## 维护、备份与迁移

14:24:38 CST 开始维护，停止 Web、Worker、Control、Source 和 Telemetry，保留 PostgreSQL、S3 与 Gateway。停写后生成 Control、Telemetry 两份真实 `pg_dump`，保存 SHA256 manifest。备份保存在本机受限目录 `.local/k8s-workspace-20261115/backup-20261115T062511Z/`。

14:25:26 CST 完成两个独立恢复库的 TimescaleDB 恢复前处理、串行 `pg_restore`、恢复后处理，以及计数、原身份和迁移版本校验。临时恢复库随后删除，原备份保留。没有重建业务数据库、变更持久卷或轮换凭据。

14:25:27 CST 应用 Control 迁移 `2026111504 / 0005_account_settings.sql` 与 `2026111505 / 0006_workspace_lifecycle.sql`。Telemetry 新迁移 `2026111508` 已落库，完整清单为 `2026111304`、`2026111403`、`2026111508`。当前安装使用共享 Control 数据库的嵌入式 Source，未配置独立 Runtime；独立 Source `2026111506`、Runtime `2026111507` 的迁移与清理行为在隔离服务中验证，不冒充现场迁移。

14:27:09 CST 初次恢复 Web，Helm revision 为 `5`。随后真实 API 验收发现生命周期操作一直 waiting、generation 为零：生产 `scheduler::start()` 的独立循环没有调用测试使用的 `scheduler::tick()`。修复为每个副本启动可取消的独立生命周期消费循环，不阻塞命令、调度或 housekeeping；先复现、再通过生产启动回归，构建并部署修正镜像。原先持久等待的线上归档操作在修正部署后自动完成，没有手工调用 tick 或修改其状态。

14:40:54 CST 确认最终 Helm revision `6`、状态 `deployed`，三个 Control 副本就绪。该修复无需追加迁移。Web 滚动替换一度因旧、新 Pod 重叠达到现有 CPU quota 而受阻；临时使用 `maxSurge=0 / maxUnavailable=1` 完成替换后，恢复并核对原更新策略，保持一个 Web 副本。ResourceQuota 没有变更。后续 Control 重启期间也出现短暂 quota 创建拒绝，Deployment 自行收敛至三个就绪副本。

集群写入使用 `artifactory` 命名空间内的受限 deployer。迁移后恢复需要匹配应用镜像与已验证备份；本次没有通过回退旧镜像来绕过已应用的新 schema。

## 现场验收

| 范围 | 实际结果 |
| --- | --- |
| 账号 | 真实 API 修改显示名称并读回；错误当前密码保留会话；改密使两个既有会话及旧密码失效 |
| 归档恢复 | 真实租户改名保持 slug；归档阻止新增项目，历史列表可读；恢复租户后单独归档的项目保持 archived |
| 排空与撤销 | 临时 `workspace_tasks/source_refs` 记录构造已接受任务阻塞；操作显示阻塞项，拒绝并发新增构建；取消等待后恢复 active；仅移除本次 fixture |
| 真实页面 | 无模拟 API 响应，验证大厅、项目首页、项目搜索与列表栏目保留、管理深链接、刷新、前进后退及两个租户的独立标签页；观测请求租户头全部正确 |
| 个人面板 | 切换语言和主题后业务草稿保留；拒绝离开后仍在原表单，再确认离开；资料从真实账号 DTO 读取；手机抽屉与全屏个人面板通过 |
| 旧入口与文档 | 被移除的租户扁平页面及全局构建创建页面返回 HTTP 404；新工作台文档匿名返回 HTTP 200 |
| 既有入口 | 保留原 `http://127.0.0.1:38100` 转发，另行验证该入口的 Session、三台原设备查询和匿名工作台文档正常；本次浏览器验收使用独立的临时转发 |
| 永久删除 | 两个无设备的临时租户并发删除；相同幂等键返回同一操作；三个项目清理后租户列表为空、旧 slug 查询 404；操作记录仍可由发起账号查询 |
| 重启不复活 | 删除后重建一个 Control Pod，再滚动替换全部三个副本，临时租户没有重新出现；完成记录仍可查询 |
| 排空期间更换副本 | 第三个临时租户保留一个 source_refs 任务 fixture，归档持续 waiting 时替换全部三个 Control Pod；替换后阻塞项和新增任务限制仍有效，移除 fixture 后由新进程自动完成归档，再恢复并通过 API 永久删除 |
| 运行与内部服务 | 三个 Control 分布于三个节点；逐副本 readiness、同一 Session 和设备查询通过；Source readiness 与 Telemetry metrics 均验证私有 CA 和必要鉴权 |
| 设备与 Worker | 原有三台设备及一个 Worker 在线；ID、证书摘要、撤销状态保持不变。Mac、Linux 主机遥测 fresh；Windows 的遥测新鲜度未通过，详见下方边界 |
| 持久资源 | 四个 PVC 全部 Bound 且 UID 保留；凭据、PKI Secret 内容与 UID 不变；Gateway StatefulSet UID 与规格不变 |
| 非目标资源 | 139 项选定资源的 UID 与规格摘要全部保持一致；该检查不等于全量集群对象及运行状态审计 |
| 清理 | 三个临时租户及其四个项目通过生命周期 API 删除；临时账号与其会话删除。八张原业务表计数恢复为一个租户、一个用户、三台设备、一个 Worker，项目、构建、制品、发布均为零；保留最小生命周期操作与墓碑 |

14:51:46 至 14:56:46 CST 完成五分钟稳定性观察，六次采样全部通过：控制平面持续 3/3 Ready，10 个 Pod 全部 Ready 且容器零重启，既有三台设备和一个 Worker 持续在线；认证 API、原租户空项目列表及匿名文档持续可用。该结论不包含三台主机遥测全部 fresh。

观察完成后注销本次管理员验收会话，确认原 Cookie 请求 `/auth/me` 返回 401，并删除本地会话文件。临时账号恢复凭据也已移除。本次 PostgreSQL 与 Web 临时转发关闭，原 38100 入口保留。最后读回没有 waiting、running 或 failed 的生命周期操作，也没有遗留任务 fixture；留下三个租户墓碑、四个项目墓碑和九条操作记录。

## CI 与验收边界

[最终运行源码的 platform-contracts 43685858286](https://github.com/zhubby/artifactory/actions/runs/43685858286) 已结束：版本契约、Compose TLS 及六个平台的 native-emissary job 全部通过；Linux 中 fmt、全仓 Clippy、依赖检查和脚本测试通过，Control 数据库回归为 92 通过、1 失败、1 过滤。失败仍为既有 OpenSSH fixture 对 `/tmp` ownership / modes 的拒绝，本机对应回归通过。后续 Web 与端到端步骤因此跳过，不能计为 CI 已运行通过。

Kubernetes job 在应用部署前的 kind 初始化阶段因 kubeadm `v1beta3` 配置不兼容失败，未进入应用部署。完整源码 CI 不为全绿；本次目标集群验收、容器镜像构建与上述 CI 结果分别记录。

本机更新后的原生全链路 `web/tests/console.spec.ts` 没有在本次 seeded native harness 上执行。真实集群验收使用空临时工作台和明确标记的任务 fixture，没有真实模型调用、设备原生构建或大规模吞吐验收。空工作台没有已签发上传或 attempt，因此线上删除不构成上传过期、迟到上传或 S3 故障注入证据；这些边界由隔离 PostgreSQL、S3 和服务回归覆盖。

设备遥测检查不能表述为三台全部 fresh。Windows 保持控制心跳在线，但主机采样在 14:48:41 CST 后没有更新；历史查询仍正常，Mac 和 Linux 持续有新样本。只读 SSH 诊断因现有连接失效、public-key 鉴权被拒而未建立，未确认设备端或连接链路根因，也没有更换设备二进制、重启服务、改变设备身份或撤销状态。此项作为未通过的现场遥测验收保留，不并入 Pod/API 稳定性结论。

三个副本指应用控制平面；PostgreSQL、S3、Source、Telemetry 和 Worker 仍为单副本。手机截图使用 Chromium 视口模拟，不代表实机 iOS 验收。备份沿用现有保留策略。

私有配置、备份、日志、截图与校验记录保存在本机忽略目录 `.local/k8s-workspace-20261115/`；凭据、会话、数据库连接串与 PKI 不进入 Git。桌面与手机真实页面截图已检查，完整视觉矩阵保存在 `.local/workspace-evidence/`。

本记录和计划完成状态在部署验收后追加到 main；运行镜像仍对应上述 `dc1594b`，后续交付记录提交仅修改文档。
