<!--
[INPUT]: 已确认的构建过程与源码诊断设计。
[OUTPUT]: 记录实施顺序、验证命令和现场交付边界。
[POS]: 本轮连续实施与 Kubernetes 验收清单。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 构建过程实施计划

> 执行方式：按项目要求在当前线程顺序实施。

**Goal:** 源码故障可诊断，观察者可还原构建过程。
**Architecture:** 源码阶段增量持久保存，其余阶段复用业务历史。
**Tech Stack:** Rust、PostgreSQL、Next.js、React、Playwright。
**Spec:** [设计合同](../specs/2026-11-24-build-observability.md)。

## 约束

最低 Kubernetes 1.28；保留数据、Secret、PVC 和设备身份。
Git stderr 仅分类；新文件同步 L3 和 AGENTS.md。

## 1. 源码诊断与进度

- [x] 添加错误分类测试。
  `assert_eq!(classify_stderr(b"Could not resolve host"), GitFailureKind::Dns)`。
- [x] 运行测试确认新接口不存在，再实现分类。
  `rtk cargo test -p artifactory-source-checkout --offline`。
- [x] 增加进度契约和 `source_tasks.progress` 兼容迁移。
- [x] 测试 `record_source_progress(id, execution, progress)`。
  重放不重复；旧执行和取消后写入被拒绝。
- [x] Source Preparer 上报阶段，提交枚举诊断。
- [x] 运行源码、数据库和迁移测试。

## 2. 受权过程查询

- [x] 实现 `GET /builds/{id}/activity`。
- [x] 聚合源码、规划、设备尝试、步骤和归档时间。
- [x] 显式字段投影，最近事件上限为 500。
- [x] 测试权限、撤权、历史缺失和两类执行归属。

## 3. 过程页面

- [x] 先增加源码失败回归。
  `await expect(page.getByText("AI 规划尚未开始", {exact:true})).toBeVisible()`。
- [x] 新建过程组件、类型和样式，过程成为首次默认标签。
- [x] 展示阶段、当前动作、位置和可展开时间线。
- [x] 保留用户选择，后台静默刷新，终态补读。
- [x] 更新中英文、API 和设计文档。
- [x] 运行类型检查、生产构建和三视口回归。

## 4. 固定交付与真实验收

- [ ] 提交代码并构建固定 AMD64 镜像。
- [ ] 备份数据库，预演目标镜像。
- [ ] 先上线 Control 迁移，再上线源码准备与 Web。
- [ ] 复核身份、迁移、摘要和健康状态。
- [ ] 发起真实构建，核验源码失败与后续未开始。
- [ ] 核验历史执行和制品记录，记录线上验收。

本轮按用户指示跳过代理配置。
GitHub 网络限制单独记录，用真实失败构建验证诊断。
