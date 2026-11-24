# docs/
> L2 | Parent: ../AGENTS.md

AGENTS.md: 定义文档入口、主题边界和成员职责。
README.md: 介绍 Artifactory 并引导读者选择入口。
SUMMARY.md: 定义 mdBook 导航顺序。
404.md: 提供文档缺页入口。
acceptance.md: 汇总实施状态和验收边界。
agent-ota-acceptance-2026-11-11.md: 记录旧 Agent OTA 验收。
ai-builds.md: 定义模型兼容、目标准入、两类执行失败修订与构建发布流程。
ai-builds-acceptance-2026-11-11.md: 记录零侵入 AI 构建验收。
ai-workspace-build-acceptance-2026-11-16.md: 记录固定源码的双平台真实构建、故障修复与内部发布证据。
ai-workspace-signing-acceptance-2026-11-17.md: 记录真实 Developer ID 托管签名、制品读回、材料清理及公证边界。
api.md: 定义 HTTP 输入、构建过程、安全规划摘要与项目权限边界。
artifactory-rename-2026-11-11.md: 记录产品命名迁移。
signing.md: 定义签名凭据、任务下发、原生验签与密钥轮换。
configuration.md: 定义任务执行计划的结构与限制。
design-system.md: 定义中性色、顶栏工具、正文动作、关注星标与导航骨架。
direct-pod-execution-2026-11-19.md: 记录直接 Pod 编排、执行围栏、启动等待和故障依赖隔离的验证边界。
control-ha.md: 定义控制平面多副本和容量验证。
emissary-ota.md: 定义 Emissary 升级与恢复流程。
frontend-experience.md: 定义导航首读、草稿、偏好、日志与 AI 审计阅读及验证。
helm.md: 定义最低 Kubernetes 1.28 安装、基线内追加迁移与持久身份保留。
image-builds.md: 定义独立镜像池准入、容器镜像生命周期、整合网关、池代理和交付边界。
image-builds-kubernetes-acceptance-2026-11-17.md: 记录镜像、独立 Operator、网关迁移与空闲连接验收。
image-builds-kubernetes-128-acceptance-2026-11-18.md: 记录 1.28 生命周期、正式部署与原生设备验收。
local-macmini-acceptance-2026-11-11.md: 记录本机和 Mac mini 的验收。
module-layout.md: 定义模块命名和依赖边界。
multitenancy.md: 定义租户权限和设备共享边界。
node-placement.md: 定义节点识别、网络诊断、能力查询与 AI 选机。
notifications.md: 定义顶栏收件箱、项目星标、事务事件、渠道订阅和通知运维。
operations.md: 定义初始账号、部署、兼容追加迁移、备份和故障恢复。
operator-operations.md: 定义临时源码、池级出口代理、内部直连和任务收尾。
protocol.md: 定义 Emissary 会话、完整心跳预算和状态迁移。
registration-acceptance-2026-11-11.md: 记录用户注册的本地验收。
planning-protocol.md: 定义源码提交固定、内嵌规划接口、预算引导与提案围栏。
ephemeral-source-acceptance-2026-11-18.md: 记录临时源码验证、存储回收与正式切换。
substrate-scheduling-assessment-2026-11-12.md: 评估可选调度后端的适用边界。
tasknotes-application-acceptance-2026-11-18.md: 记录 TaskNotes 真实应用的构建、网关下载、运行证据和环境阻塞。
tasknotes-platform-fixes-2026-11-18.md: 罗列 TaskNotes 暴露的平台缺陷、修复验证与剩余能力边界。
telemetry.md: 定义主机与构建的时序观测、私有双端口 Service 和设备 mTLS。
toasty-acceptance-2026-11-13.md: 记录 Toasty 数据迁移验收。
versioning.md: 定义统一版本与交付约束。
release-delivery.md: 定义版本元数据、四步构建确认、独立源码与目标、单次不可变交付。
workspaces.md: 定义两层概览、项目目录、导航骨架、页面动作与个人设置流程。
acceptance/: 保存子系统在真实环境中的验收记录。
brand/: 保存品牌规范和视觉资源。
fonts/: 保存文档构建生成的字体资产。
plans/: 保存实施方案与修复计划。
releases/: 保存版本交付、固定镜像与部署验收记录。
superpowers/: 保存设计说明和实现计划。
theme/: 提供 mdBook 样式与浏览器资源。

[PROTOCOL]: Update this header on change, then check AGENTS.md
