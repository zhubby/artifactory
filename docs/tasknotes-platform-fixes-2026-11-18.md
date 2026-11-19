<!--
[INPUT]: TaskNotes 验收现场、平台源码和隔离回归结果。
[OUTPUT]: 罗列平台缺陷、修复证据与剩余边界。
[POS]: 连接真实应用验收与本轮代码修复。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# TaskNotes 暴露的平台问题与修复

本轮修复基于 [TaskNotes 真实应用验收](tasknotes-application-acceptance-2026-11-18.md)。
应用固定提交保持 `d56401a5c86547db0d27456b4737b478fde9376c`。
没有修改应用源码、锁文件或参考任务。
修复保持 Kubernetes 1.28 的普通 init 与普通应用容器结构。
没有增加旧字段兼容或数据迁移代码。

## 已修复的平台缺陷

| 优先级 | 问题与影响 | 根因 | 修复 |
| --- | --- | --- | --- |
| P1 | 规划源码拉取失败后占住唯一槽位，后续任务持续排队 | Agent Operator 忽略 init 终态，Runtime 未收到失败 | 检查 init、Worker 和 Pod 终态；Runtime 校验完整执行身份，原子写失败与 outbox，撤销 grant 并释放槽位；Control 消费失败事件结束构建 |
| P2 | iOS 缺少应用分类，Darwin 目标被套用 macOS 签名，页面只显示宿主 | Worker 工具 schema 漏 iOS；后端与签名策略混用宿主和应用类别 | AI 每个目标显式声明应用平台，非应用使用 null；按显式平台选择签名方案；页面分别显示应用类别与宿主 |
| P2 | 镜像驱逐、OOM、源码失败与推送失败都只显示通用失败 | Operator 清理覆盖原因；Control 丢弃回报文本；Publisher 没有日志通道 | 删除 Pod 前保存白名单原因、容器名和退出码；Control 复核 CR 身份后保存原因；Worker 与 Publisher 写脱敏诊断日志和摘要 |
| P2 | 构建池预算在辅助容器重复分配，实际请求量与编译额度不直观 | 同一 CPU、内存与磁盘配置分配给源码 init、Runner 和 BuildKit | source、runner、buildkit、publisher 分别配置；Runner 存储请求额外覆盖 init 结束后保留的源码卷 |

规划失败回报包含 run、generation、Pod UID、Pod 名与 namespace。
旧身份被拒绝，重复故障只产生一条失败事件。
待接纳提案与已接受结果不被 Pod 退出覆盖。
取消后的业务构建不被延迟失败回执改写。
源码准备期间显示 Provisioning，Worker acquire 后显示 Running。

镜像错误先清除完整 HTTP(S) URL，再替换已知任务秘密。
未知 URL 的 userinfo 与签名查询串不写入新增诊断日志。
诊断日志上限为 16 KiB，单行摘要上限为 1 KiB。
原始 Kubernetes message 不写入公开错误。
失败 CR 与成功 CR 均检查 UID、完整 spec 与 observedGeneration。
发布令牌只写 publish 日志，构建令牌只写授权架构日志。
后续清理不会覆盖已保存的失败原因。

## 资源契约

BuilderPool 使用 `resources.source`、`resources.runner`、`resources.buildkit`、`resources.publisher`。
池级 cpu、memory、ephemeralStorage 字段已删除。
Rust 默认值、两份 CRD、Helm values 和验收夹具保持一致。
详细参数见[镜像构建指南](image-builds.md#构建节点和私有-ca)。

BuildKit 默认值保持 1 CPU、2 GiB、20 GiB。
Runner 默认使用 250m CPU、256 MiB、1 GiB 工作卷。
源码卷默认 10 GiB，退出 init 后仍占磁盘。
默认平台 Pod 合计申请 1.25 CPU、2304 MiB 内存与 31 GiB 临时存储。
磁盘预算包含保留源码，不把 init CPU 与内存重复加到运行阶段。

TaskNotes 当时部署的池覆盖为 500m、768 MiB、3 GiB。
它低于产品默认 BuildKit 预算。
Debian 构建层在临时 8 GiB 卷中也发生驱逐。
Alpine 在 1 CPU、2 GiB、8 GiB 的临时参数下通过。
该结果不证明低配池或所有基础镜像均能通过。

## 剩余平台能力与恢复边界

1. **出口代理没有池级配置。** 本轮验收通过临时工具镜像注入代理变量。NO_PROXY 曾遗漏内建仓库公开地址，导致推送被代理拒绝。这是验收网络配置错误；平台仍需统一的出口代理配置能力。配置必须覆盖源码准备、Worker、BuildKit 和 Publisher，并将 Control、Runtime、内建仓库及认证地址纳入绕过列表。
2. **无 Pod 的规划 Job 失败仍靠超时恢复。** Job 从未创建 Pod，或 Operator 停机期间 Job 被 TTL 完全删除时，没有可登记的 Pod UID。该路径继续使用现有约 900 秒 provisioning 超时与 generation 重试，不属于本轮永久占槽修复的覆盖范围。
3. **iOS 分类不等于 iOS 托管签名。** 本轮仅修分类与默认方案匹配。iOS 设备导出、provisioning profile 和商店分发仍由项目脚本与本机签名授权提供。
4. **失败后的收尾日志存在竞态。** BuildKit daemon 先退出时，Operator 能先写失败终态。Runner 随后上传的收尾日志会被当前 LIVE 授权策略拒绝。白名单 Kubernetes 原因仍保留，但 daemon 详细尾部日志可能丢失。后续需要只允许追加日志、不改变终态的有界排空窗口。

这些边界不通过隐藏的重试、跳过 TLS 校验或旧协议兼容来绕过。

## 环境阻塞与未验收范围

| 项目 | 当前证据 | 后续动作 |
| --- | --- | --- |
| Windows WinUI | VS C++ 安装实例不完整，核心测试通过，完整 ZIP 与窗口未通过 | 完成 SDK 安装并重新验收；不绕过官方包摘要校验 |
| iOS 实际运行 | Xcode 构建与下载通过；Mac mini 没有 Simulator Runtime | 准备磁盘与 Runtime 后验证模拟器启动和联机业务 |
| TaskNotes OCI arm64 | 本轮只指定 linux/amd64 | 在原生 arm64 构建节点另行构建并读回 OCI 图 |
| Linux/macOS 联机业务 | GTK 窗口和 macOS 应用启动通过 | 对下载制品继续验证联机写入、离线与重连 |

SDK 命令可执行不证明工具链完整。
平台节点能力探测是启动时快照。
安装工具后须重启节点进程再刷新能力。
这两项属于已定义的节点契约，没有列为本轮新增缺陷。

## 修复验证

验证使用独立本机 PostgreSQL 17，未连接集群业务数据库。

- Runtime 的 21 项数据库回归通过，覆盖并发幂等、完整身份、旧 generation、pending 提案、取消和槽位释放。
- 控制平面新增真实事务回归通过：失败同时结束 planning_run 与 build；并发重放只保存一次；取消保持终态。
- 镜像相关 24 项数据库回归通过，覆盖失败身份围栏、清理后诊断与发布日志授权。
- 镜像 Operator 回归覆盖 Evicted、source init 退出、OOMKilled、保留源码的磁盘预算，以及 URL 协议名与秘密重叠时的脱敏。
- Worker 18 项测试、配方 9 项、Web 类型检查与中英文浏览器 3 项回归通过。
- 5 个变更 Rust crate 的 all-targets/all-features Clippy 通过，警告视为错误。
- Workspace 格式检查、镜像离线验收夹具 14 项和 Kubernetes 1.28.3 Helm lint 通过。
- 现有集群实测版本为 v1.28.3；BuilderPool 与 ImageBuild CRD 通过服务端 dry-run，没有持久修改集群。

最终回归发现 EOF 测试夹具在 Runner 确认 Ready 前关闭 socket。
夹具增加显式关闭屏障，分别验证启动前拒绝与启动后取消。
两项用例各重复 20 轮，共 40 次通过。

本轮验证证明代码与隔离回归结果。
修复尚未推送或部署到现有集群。
前轮成功的应用构建不能替代这版服务的重新部署验收。
