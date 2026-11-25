<!--
[INPUT]: 当前手册、实施计划和带日期验收记录。
[OUTPUT]: 定义 mdBook 章节及历史记录入口。
[POS]: 文档站导航索引；新增页面同步列入此处。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 目录

[Artifactory 文档](README.md)

# 使用与配置

- [零侵入 AI 构建](ai-builds.md)
- [多租户、项目权限与共享设备](multitenancy.md)
- [租户与项目工作台、个人设置及生命周期](workspaces.md)
- [通知中心与外部渠道](notifications.md)
- [节点元数据、MCP 与 AI 选机](node-placement.md)
- [安装并加入节点](node-installation.md)
- [设备终端](terminals.md)
- [应用签名与凭据下发](signing.md)
- [执行配方与历史配置](configuration.md)
- [控制台语言、主题与动效](frontend-experience.md)
- [界面设计规范](design-system.md)

# 部署与运维

- [直接 Pod 执行记录](direct-pod-execution-2026-11-19.md)

- [部署与运维](operations.md)
- [Helm / Kubernetes 部署](helm.md)
- [统一版本与发布](versioning.md)
- [项目版本交付](release-delivery.md)
- [控制平面多副本与设备容量](control-ha.md)
- [Kubernetes Planning Operator 运维](operator-operations.md)
- [集群镜像构建、内建仓库与网关](image-builds.md)
- [Emissary OTA 升级](emissary-ota.md)
- [Emissary 时序观测](telemetry.md)

# 接口与架构

- [模块布局与命名](module-layout.md)
- [模块边界与规划命名实施方案 · 2026-11-19](plans/2026-11-19-module-boundaries.md)
- [Release 中心版本交付实施方案 · 2026-11-20](plans/2026-11-20-release-centered-delivery.md)
- [HTTP API v1](api.md)
- [Emissary 协议与状态机](protocol.md)
- [规划协调器与源码准备协议](planning-protocol.md)

# 验收记录

- [实施与验收总览](acceptance.md)
- [源码准备与规划复用 · 2026-11-25](acceptance/2026-11-25-source-planning-reuse.md)
- [设备终端本地验收 · 2026-11-25](acceptance/2026-11-25-device-terminals.md)
- [Windows 清理与在线恢复 · 2026-11-25](acceptance/2026-11-25-windows-cleanup.md)
- [真实模型全流程 · 2026-11-22](acceptance/2026-11-22-model-workflows.md)
- [TaskNotes 真实多平台应用 · 2026-11-18](tasknotes-application-acceptance-2026-11-18.md)
- [TaskNotes 平台问题与修复 · 2026-11-18](tasknotes-platform-fixes-2026-11-18.md)
- [临时源码执行 · 2026-11-18](ephemeral-source-acceptance-2026-11-18.md)
- [Kubernetes 1.28 镜像构建 · 2026-11-18](image-builds-kubernetes-128-acceptance-2026-11-18.md)
- [集群镜像构建与独立 Operator · 2026-11-17](image-builds-kubernetes-acceptance-2026-11-17.md)
- [新版 UI 恢复与旧代码清理 · 2026-11-17](acceptance/ui/2026-11-17-restore-and-remove-legacy.md)
- [AI-Workspace macOS 托管证书签名 · 2026-11-17](ai-workspace-signing-acceptance-2026-11-17.md)
- [AI-Workspace 双平台真实构建 · 2026-11-16](ai-workspace-build-acceptance-2026-11-16.md)
- [工作树备份比对与源码恢复 · 2026-11-16](acceptance/2026-11-16-worktree-recovery.md)
- [全站 UX/UI 重构 · 2026-11-15](acceptance/ui/2026-11-15.md)
- [真实浏览器巡检与修复 · 2026-11-15](acceptance/ui/2026-11-15-browser-audit.md)
- [交互与视觉复查 · 2026-11-16](acceptance/ui/2026-11-16-interaction-visual.md)
- [弹窗入场跳位修复 · 2026-11-16](acceptance/ui/2026-11-16-modal-motion.md)
- [节点元数据、MCP 与 AI 选机 · 2026-11-14](acceptance/nodes/2026-11-14.md)
- [控制平面 HA 与 Toasty · 2026-11-14](acceptance/control-ha/2026-11-14-toasty.md)
- [控制平面 HA · SQLx 阶段归档](acceptance/control-ha/2026-11-14.md)
- [Toasty 数据访问迁移 · 2026-11-13](toasty-acceptance-2026-11-13.md)
- [Emissary 六平台 · 2026-11-13](acceptance/emissary/2026-11-13-six-platforms.md)
- [Rust Emissary · 2026-11-13](acceptance/emissary/2026-11-13.md)
- [真实供应商链路 · 2026-11-13](acceptance/provider/2026-11-13.md)
- [TimescaleDB 遥测 · 2026-11-13](acceptance/telemetry/2026-11-13.md)
- [Operator 与 Kubernetes Pi · 2026-11-12](acceptance/operator/2026-11-12.md)
- [本机 / Mac mini · 2026-11-11](local-macmini-acceptance-2026-11-11.md)
- [AI 构建 · 2026-11-11](ai-builds-acceptance-2026-11-11.md)
- [用户自助注册 · 2026-11-11](registration-acceptance-2026-11-11.md)
- [Agent OTA · 2026-11-11](agent-ota-acceptance-2026-11-11.md)

# 设计与计划归档

- [设备交互终端实施计划 · 2026-11-24](plans/2026-11-24-001-feat-interactive-terminals-plan.md)
- [节点安装与加入实施计划 · 2026-11-25](plans/2026-11-25-node-installation.md)

- [设备命名与辨识度改进 · 2026-11-20](plans/2026-11-20-node-identification.md)
- [Kubernetes 1.28 一次性镜像构建方案 · 2026-11-18](plans/2026-11-18-001-refactor-kubernetes-128-build-plan.md)
- [临时源码与任务独立拉取 · 2026-11-18](plans/2026-11-18-ephemeral-source-execution.md)
- [设计基线 · 2026-11-09](superpowers/specs/2026-11-09-artifactory-design.md)
- [设计契约审阅 · 2026-11-09](superpowers/specs/2026-11-09-artifactory-design-review.md)
- [AI 构建实现计划 · 2026-11-11](superpowers/plans/2026-11-11-ai-builds.md)
- [Operator 架构设计 · 2026-11-12](superpowers/specs/2026-11-12-kubernetes-agent-operator-design.md)
- [Operator 落地计划 · 2026-11-12](plans/2026-11-12-001-feat-kubernetes-agent-operator-plan.md)
- [可选调度后端评估 · 2026-11-12](substrate-scheduling-assessment-2026-11-12.md)
- [Toasty 数据访问迁移计划 · 2026-11-13](plans/2026-11-13-toasty-data-access.md)
- [多租户实施与审查修复 · 2026-11-15](plans/2026-11-15-multitenancy.md)
- [可读租户、项目与构建路由计划 · 2026-11-15](superpowers/plans/2026-11-15-readable-routes.md)
- [租户与项目工作台实施计划 · 2026-11-15](superpowers/plans/2026-11-15-workspace-hierarchy.md)
- [中性色控制台重构计划 · 2026-11-15](superpowers/plans/2026-11-15-monochrome-console.md)
- [托管应用签名实施计划 · 2026-11-16](superpowers/plans/2026-11-16-managed-signing.md)
- [镜像构建与独立 Operator 实施计划 · 2026-11-17](superpowers/plans/2026-11-17-image-builds-and-operators.md)
- [Release 元数据与独立构建实施计划 · 2026-11-21](superpowers/plans/2026-11-21-release-build-decoupling.md)
- [构建过程与源码诊断设计 · 2026-11-24](superpowers/specs/2026-11-24-build-observability.md)
- [构建过程实施计划 · 2026-11-24](superpowers/plans/2026-11-24-build-observability.md)
- [源码准备与规划复用实施计划 · 2026-11-25](superpowers/plans/2026-11-25-source-planning-reuse.md)

# 品牌与变更记录

- [Artifactory 0.2.0](releases/0.2.0.md)
- [0.2.0 Kubernetes 试部署记录](releases/0.2.0-kubernetes-trial.md)
- [多租户 Kubernetes 升级记录 · 2026-11-15](releases/2026-11-15-multitenancy-kubernetes.md)
- [可读路由 Kubernetes 升级记录 · 2026-11-15](releases/2026-11-15-readable-routes-kubernetes.md)
- [租户与项目工作台 Kubernetes 升级记录 · 2026-11-15](releases/2026-11-15-workspace-hierarchy-kubernetes.md)
- [中性色控制台 Kubernetes 升级记录 · 2026-11-15](releases/2026-11-15-monochrome-console-kubernetes.md)
- [托管应用签名 Kubernetes 升级记录 · 2026-11-16](releases/2026-11-16-managed-signing-kubernetes.md)
- [模块重构 Kubernetes 新实例部署 · 2026-11-19](releases/2026-11-19-module-boundaries-kubernetes.md)
- [Release 中心版本交付 Kubernetes 记录 · 2026-11-20](releases/2026-11-20-release-delivery-kubernetes.md)
- [加载反馈与租户大厅 Kubernetes 上线记录 · 2026-11-20](releases/2026-11-20-loading-tenant-kubernetes.md)
- [自定义下拉与 HTTP 操作 Kubernetes 上线记录 · 2026-11-20](releases/2026-11-20-dropdown-http-kubernetes.md)
- [设备识别 Kubernetes 上线记录 · 2026-11-20](releases/2026-11-20-node-identification-kubernetes.md)
- [设备详情 Kubernetes 上线记录 · 2026-11-21](releases/2026-11-21-device-detail-kubernetes.md)
- [Release 与构建解耦 Kubernetes 上线记录 · 2026-11-21](releases/2026-11-21-release-build-decoupling-kubernetes.md)
- [四步构建引导 Kubernetes 上线记录 · 2026-11-21](releases/2026-11-21-guided-build-kubernetes.md)
- [通知中心 Kubernetes 上线记录 · 2026-11-23](releases/2026-11-23-notification-center-kubernetes.md)
- [构建过程与源码诊断 Kubernetes 上线记录 · 2026-11-24](releases/2026-11-24-build-observability-kubernetes.md)
- [AI 审计阅读 Kubernetes 上线记录 · 2026-11-24](releases/2026-11-24-ai-audit-reading-kubernetes.md)
- [设备终端 Kubernetes 上线记录 · 2026-11-25](releases/2026-11-25-device-terminals-kubernetes.md)
- [节点安装与三节点终端 · 2026-11-25](releases/2026-11-25-node-installation-terminals.md)
- [统一 0.2.3 与三节点手动 OTA · 2026-11-25](releases/2026-11-25-manual-ota-kubernetes.md)
- [Artifactory 装配 A](brand/README.md)
- [Artifactory 更名记录 · 2026-11-11](artifactory-rename-2026-11-11.md)

# 文档维护地图

- [文档模块地图](AGENTS.md)
- [品牌模块地图](brand/AGENTS.md)
- [验收模块地图](acceptance/AGENTS.md)
- [界面验收模块地图](acceptance/ui/AGENTS.md)
- [交付记录模块地图](releases/AGENTS.md)
- [文档主题模块地图](theme/AGENTS.md)
- [设计与计划模块地图](superpowers/AGENTS.md)
- [设计合同模块地图](superpowers/specs/AGENTS.md)
- [实施计划模块地图](superpowers/plans/AGENTS.md)
- [原型实施计划地图](plans/AGENTS.md)
