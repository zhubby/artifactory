# 可读路由 Kubernetes 升级记录

日期：2026-11-15，Asia/Shanghai。范围为现有 Kubernetes `1.28.3` 的 `artifactory` 命名空间。

## 实现

路由实现提交：`e347c3a161d1f2c0d5769559eaf0f2806fed8169`；最终运行镜像源码提交：`28614c90fa5e4bbfa92a81b88fe537c147d5ba38`，包含 Telemetry 当前迁移清单校验修复。

控制台直接使用 `/tenants/default`、`/tenants/default/projects/desktop-app`、`/tenants/default/projects/desktop-app/builds/128` 和 `/tenants/default/settings`。租户 slug 全局唯一，项目 slug 在租户内唯一，均由创建名称生成并独立保存；修改显示名称不会改变链接。构建编号由数据库按项目原子分配，从一开始且不复用已删除编号。

删除旧 `/t/{uuid}` 页面和全局 catch-all，不保留重定向或 UUID 页面回退。原内部 UUID API 继续承担资源操作与授权；可读查找仍检查当前成员资格、项目授权和 Token scope。浏览器先解析租户 slug，再发送内部 UUID 请求头，并在每次异步等待前后检查租户上下文与取消信号。

## 验证

- Rust fmt、控制平面 Clippy（`-D warnings`）、库测试（13 通过，1 项性能测试忽略）通过。
- 新路由数据库回归覆盖升级保持身份、slug 稳定性与冲突、权限及编码标识符、并发构建编号；租户回归 18 项通过。
- 完整控制平面数据库测试首轮 79 项通过、3 项失败：两个 Runtime 用例缺少要求的测试服务令牌，一个重连锁用例在同时运行浏览器时错过两秒观察窗口。补齐独立随机令牌并单独重跑三项全部通过；82 项均有通过证据，不能表述为一次完整运行全绿。
- Next 类型生成、TypeScript 和生产构建通过。
- 独立生产服务上的 Playwright 回归 228 项全部通过（桌面、平板、手机各 76 项，18.5 分钟）。先前复用旧预览服务引发的静态 JavaScript 500 已通过独立端口和明确 base URL 排除。
- 现场启动校验发现 Telemetry 手工迁移版本列表遗漏新增版本。先在隔离 PostgreSQL 复现失败，再改为直接使用 Control 的迁移清单；两项 Telemetry 库测试（含真实只读角色授权与历史分页）、fmt 和 Clippy 全部通过。新校验拒绝缺少迁移或未知版本，不再回退到旧历史。

## 镜像

[deployment-images 43668476107](https://github.com/zhubby/artifactory/actions/runs/43668476107) 成功。核对了 GHCR 和集群镜像端点的索引、amd64 manifest、config 与 OCI 源码 SHA，并通过九个临时 Pod 完成三个节点上的实际预拉取。

| 组件 | 索引摘要 |
| --- | --- |
| Control Plane / Source / Telemetry | `sha256:0317e3e5d873f4591b35205b867595e254f13692b4c70cec4596f3af45dc8f26` |
| Agent Worker | `sha256:a2e2b93d4ca06d1140df97daedd9c7b92856eec11c897c432aee6ba0b53d1977` |
| Web | `sha256:f5874fbd78b986c397735dd7267ac1c8565eb2f34a9d5293c9bb3d6ff2938c30` |

Helm 渲染前后仅五个第一方容器镜像引用发生变化；控制平面副本数为三，Gateway、PostgreSQL、对象存储和配置未纳入变更。

## 协调升级与恢复

维护从 10:28:13 CST 开始，停止 Web、Worker、Control、Source 和 Telemetry，保留 PostgreSQL、S3 与 Gateway。停写后生成真实 `pg_dump`、可读性检查及 SHA256 manifest，再追加应用 `2026111503 / 0004_readable_routes.sql`。没有重建业务数据库或更换资源 UUID。

10:29:01 CST 完成独立数据库的 TimescaleDB 恢复前处理、串行 `pg_restore`、恢复后处理和计数／身份校验；确认备份保留的是升级前已存在的租户 schema。随后删除临时恢复库，保留受限目录中的备份和校验记录。

初次内部启动因 Telemetry 遗漏新迁移版本而失败，Web 保持关闭。补齐当前迁移清单校验、通过隔离回归并构建修正镜像后，10:43:44 CST 通过内部验收，10:44:08 CST 确认 Helm revision `4`、状态 `deployed`，恢复 Web。升级没有使用 `--atomic` 将已迁移数据库接回旧版本；恢复应配对使用备份和匹配的应用镜像。

本次安装未配置独立 Runtime，不涉及 Runtime 数据回填。所有集群写入均使用 `artifactory` 命名空间内的受限 deployer；没有移动版本标签、覆盖发布资产或修改集群配置。

## 现场验收

| 范围 | 结果 |
| --- | --- |
| 可读标识符 | 真实 API 创建租户／项目返回 slug；同名冲突加后缀，不同租户的同名项目独立解析；构建按项目从一编号 |
| 稳定链接 | 项目改名后原 slug、项目链接和构建链接保持有效 |
| 权限 | 非成员无法解析租户；项目显式授权前访问被拒绝；合法授权和百分号编码 slug 可读取；移除成员后 Session 和 Token 访问立即被拒绝；Token 不能用于账户级租户解析 |
| 真实浏览器 | 深链接、刷新、项目到构建导航、前进后退、两个租户的独立标签页通过；观测到的业务请求均携带正确内部租户 UUID，无模拟 API 响应 |
| 默认入口 | 既有 `/tenants/default` 与设置页正常；现有入口 `http://127.0.0.1:38100` 的旧 `/t/UUID` 路径为 HTTP 404，匿名文档正常 |
| 控制与内部服务 | 3/3 Control Ready，分布于三个节点；逐副本 readiness、同一 Session 及设备查询通过；Source 和 Telemetry TLS 校验通过 |
| 设备与 Worker | 既有 3 台设备和 1 个 Worker 在线，ID、证书摘要及撤销状态不变；三台设备的遥测均为 `fresh: true` |
| 镜像 | 7 个运行中的第一方容器与最终源码的不可变镜像身份一致 |
| 持久资源 | 4 个 PVC 全部 Bound，UID 保留；凭据和 PKI Secret 内容及 UID 不变；Gateway StatefulSet UID 与规格不变 |
| 非目标资源 | 139 项选定资源的 UID 与规格摘要全部保持一致；该审计不等于全量集群对象或运行状态审计 |
| 清理 | 临时用户、租户、项目、构建、邀请和 Token 均已清理；业务表数量恢复至 1 租户、1 用户、3 设备、1 Worker，项目／构建／制品／发布均为零；预拉取临时 Pod 已删除 |

验收使用没有设备和 Worker 的独立临时租户；临时构建已取消，没有向既有设备分配构建任务，也未产生执行 attempt。

10:47:48 至 10:52:48 CST 完成五分钟稳定性观察，六次采样全部通过：控制平面持续 3/3 Ready，10 个 Pod 全部 Ready 且容器零重启，既有 3 台设备和 1 个 Worker 持续在线；认证 API、原租户空项目列表及匿名文档持续可用。观察结束后注销临时管理员验收会话，确认原 Cookie 请求 `/auth/me` 返回 401，并删除本地会话文件。

## CI 与验证边界

[最终源码的 platform-contracts 43668535882](https://github.com/zhubby/artifactory/actions/runs/43668535882) 中，版本契约、Compose TLS 和六个平台的 native-emissary job 通过。Linux 数据库回归为 81 项通过、1 项失败；失败仍是既有 OpenSSH fixture 对 `/tmp` ownership / modes 的拒绝，本机对应回归已通过。Kubernetes job 在 kind 初始化阶段因生成的 kubeadm `v1beta3` 配置不兼容而失败；未进入应用部署，也没有据此更改目标集群配置。完整源码 CI 不为全绿。

三个副本指应用控制平面；PostgreSQL、S3、Source、Telemetry 和 Worker 仍为单副本。本次不包含故障注入、万台设备压测、真实模型调用或设备原生构建验收。

私有配置、备份、日志、截图与校验记录保存在本机忽略目录 `.local/k8s-readable-routes-20261115/`，没有将登录会话、Token、数据库连接串或 PKI 写入 Git。

本记录和计划完成状态在部署验收后追加到 main；运行镜像仍对应上述 `28614c9`，后续交付记录提交仅修改文档。
