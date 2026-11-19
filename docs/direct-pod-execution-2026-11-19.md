<!--
[INPUT]: 直接 Pod Operator、Runtime 执行围栏和最低 Kubernetes 1.28。
[OUTPUT]: 记录任务编排变更、启动状态等待、验证证据和验收边界。
[POS]: 一次性任务直接 Pod 实施记录；当前操作契约见 Operator 运维。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 直接 Pod 执行记录：2026-11-19

规划、源码与镜像任务均由 Operator 直接创建 Pod。平台不创建 Kubernetes Job，不保留 Job 兼容解析或数据库迁移代码。业务数据库中的 `jobs` 是构建目标，与 Kubernetes Job 无关。

## 执行边界

| 任务 | 直接 owner | 重试归属 | 执行证据 |
| --- | --- | --- | --- |
| AI 规划 | AgentRun | Runtime generation，最多三代 | 固定项目、创建意图、Pod UID、原 deadline |
| 源码 resolve / refs | SourceTask | Operator，最多三次且共享总 deadline | CR UID、单调 attempt、Pod UID |
| 各架构镜像与 Publisher | ImageBuild | 原 execution 内不自动重建 | 最多三个 Pod 的 UID 和停止台账 |

全部任务使用 `restartPolicy: Never`。镜像 Pod 保持源码 init、Runner 与 BuildKit 两个普通容器，最低支持 Kubernetes 1.28，不依赖原生 sidecar。1.28 及以上版本使用同一执行实现。规划 Worker 入口改为 `pod-main.js`。

CR finalizer 先于 Pod 创建。Pod 自带停止确认 finalizer。Operator 按 UID 删除任务，确认声明的容器全部停止，先保存 `stopped=true`，再移除 Pod finalizer。Pod 全部清理后，Operator 主动释放终态 CR 的 finalizer，保留审计记录。阶段名称或 DELETE 成功均不单独作为进程已停止的证明。

节点失联、Pod 身份替换、API 对象无故消失，以及已经开始但尚未确认的创建请求，都保留未知执行与容量。规划同时限制池和项目的实际执行数。已停止的平台 Pod 释放执行槽，允许 Publisher 准入；停止证据仍保留到整次镜像执行收尾。

Agent 在创建前保存 `createStarted`。ImageBuild 在 `podOwnership.creating` 保存单个待创建 Pod 名。观察到实际 Pod 后才绑定 UID 并清除预留。创建前重新读取 CR 的 UID、版本和取消状态，拒绝旧协调快照。

外部删除规划 Pod 时，Operator 通过 `/v1/execution-lost` 撤销旧授权。Runtime 保留原 deadline 和累计调用，在预算允许时重新排队。旧 Pod 未停止并完成清理前，新 generation 不创建 Pod。删除 AgentRun 则结束活动任务，持久 outbox 通知 Control。

## 凭据围栏

Control 使用 TokenReview、实际 Pod UID、命名空间、ServiceAccount 与直接 Pod→CR owner 共同授权。标签不能领取材料。

SourceTask 在业务数据库固定 CR UID。材料领取按任务行锁单调绑定 attempt、Pod 名与 UID。pin 和 commit 在同一锁序下复核身份，拒绝低代请求与同代 Pod 替换。镜像材料要求完整不可变 spec 与 `podOwnership` 台账匹配。

源码业务错误已提交时，Control 直接结束任务，再取消 CR。取消写回前可能短暂出现下一次 Pod attempt；该 attempt 不能获得新的源码凭据。

## 自动化验证

- Agent Operator：22 项测试，覆盖直接 owner、持久预留、迟到创建、项目配额、源码重试、Pod 删除恢复、旧快照围栏和终态 finalizer 释放。
- Image Operator：29 项单元测试与 21 项 API 生命周期测试，覆盖创建预留、旧快照、UID 替换、未知容量、init 失败、Publisher 准入和终态 finalizer 释放。
- Control：31 项单元测试通过，另有 10 项真实 PostgreSQL 源码回归与 24 项镜像数据库回归通过。
- Runtime：25 项真实 PostgreSQL 测试通过，覆盖丢失恢复、generation、模型调用与 deadline 耗尽的终态。
- Worker：18 项测试通过。脚本 41 项、Operator 验收夹具 25 项、镜像验收夹具 19 项离线回归通过。
- Go 模型与 Git/LFS 验收夹具测试通过。
- 新生命周期验收入口的八项版本下限、安全与 Runtime 替身契约测试通过。
- 受影响 Rust crate 的 Clippy、格式检查与 Helm 1.28 渲染通过。
- 新 CRD 通过现有 Kubernetes 1.28.3 的服务端 dry-run；未修改该集群的 CRD 或工作负载。

实际 Pod 生命周期验证使用隔离 Kubernetes 1.28.13 和 1.37.0。复跑入口为 `tests/operator/pods/run.py`，该入口只接受专属 kind 集群。

## 版本下限契约

平台最低支持 Kubernetes 1.28。四个 Helm Chart 统一声明 `>=1.28.0-0`，无版本上限。已验证 1.27 被拒绝，1.28、1.29、1.33 和 1.37 渲染通过。

生命周期验收入口按版本下限校验，不再限定 `v1.28.*`。离线回归覆盖 1.28、高版本及带发行版后缀的版本，并拒绝低于 1.28 或格式错误的版本。

完整 CI 以 Kubernetes 1.28.13 为基线，执行源码、规划、镜像、故障与双架构验收。集群配置明确关闭 `SidecarContainers`。完整业务链的验收结果以对应提交的 GitHub CI 记录为准。

基线 CI `44458636619` 暴露清理夹具的启动竞态。Runtime 已记录首次模型调用时，Kubernetes 仍返回没有 `containerID` 的容器状态。`cleanup.py` 现在按台账 UID 重新读取 Pod，等待名为 `pi` 的容器进入 `running` 并发布 CRI ID，再核对节点侧物理身份。等待期间 UID 替换立即中止；启动超时执行测试资源恢复。清理夹具六项与验收夹具全部 31 项离线回归通过，覆盖状态缺失、容器等待、旁车排序、身份替换与启动超时。本次修复未重跑真实节点故障注入。

## Kubernetes 1.28 下限实测

隔离集群为 ARM64，禁用 `SidecarContainers`。使用真实 API Server、kubelet 和新 Operator 进程。任务容器使用生命周期夹具，不执行 Git 拉取、模型推理或 BuildKit 编译。

以下九个场景通过：

1. SourceTask 第一次非零退出后显式重试；Pod UID 变化，总 deadline 保留。
2. 规划主容器非零退出，Operator 上报 Runtime 并清理 Pod。
3. 规划源码 init 非零退出，未启动主容器不阻塞收尾。
4. Agent Operator 重启接管原 Pod，取消后完成清理。
5. 取消正在执行的 SourceTask，停止 Pod 并回收临时材料。
6. 镜像平台 Pod 成功后才创建 Publisher，全部任务完成后清理。
7. Image Operator 重启接管原 UID，取消后完成清理。
8. kubelet 失联期间保留 Pod UID、清理保护与容量；恢复后继续清理和准入。
9. 规划 Pod 已调度但处于 ErrImagePull，未创建容器；取消后正常释放。

全程未创建 Job。测试命名空间按任务 CR、Pod、命名空间的顺序自主清理。完整首轮证据位于本机 `.deploy/direct-pods-k128/reviewed/evidence.json`。

最终二进制分两轮再次通过以上九个场景，并逐场景断言终态 CR 的 finalizer 已释放且不会重新安装。两轮均未创建 Job，命名空间均正常删除。证据分别位于 `.deploy/direct-pods-k128/final-binaries/evidence.json` 和 `.deploy/direct-pods-k128/final-faults/evidence.json`。验收记录包含每次 Operator 启动的 SHA256；两轮均使用同一组最终二进制。

## Kubernetes 1.37 高版本实测

同一组 Operator 二进制在隔离 ARM64 Kubernetes 1.37.0 再次通过以上九个场景。集群使用版本默认功能开关，没有设置 `SidecarContainers`。验收入口接受该高版本，没有选择其他执行实现。

证据位于本机 `.deploy/direct-pods-current/verified/evidence.json`。九项均通过，未创建 Job，终态 CR 均释放 finalizer，命名空间正常删除，没有执行或清理失败。两端验收记录中的 Operator SHA256 一致。

两端实测均使用生命周期夹具替代 Git、AI 和 BuildKit 业务进程。该结果证明 Pod 编排、取消、重启接管、失联容量与收尾行为；完整业务链以基线 CI 实际运行结果为准。

## 运维取舍

创建请求已被持久标记开始，但实际未发出时发生取消，Operator 仍保守保留预留。这需要管理员确认节点已隔离、进程已停止后处置。不要通过清空台账或移除 finalizer 绕过物理停机确认。

首次直接删除测试命名空间暴露了 Lease 与 CR finalizer 的卸载顺序竞争。已完成任务主动释放 CR finalizer，避免历史干净任务阻塞空闲命名空间删除。实例仍有任务时，先取消并删除任务 CR，等待 Pod 收尾，再卸载 Operator 和命名空间；控制器已被卸载时无法继续确认物理停机。

本轮验证不等同于 TaskNotes 完整构建发布验收。实际构建、私有 Git、Registry 推送和双架构 RUN 的完整验收入口仍是 `tests/operator/acceptance/run.py` 与 `tests/images/run.py`。

当前安装、资源与故障处理契约见 [Operator 运维](operator-operations.md)、[Runtime 协议](runtime-protocol.md) 和 [镜像构建](image-builds.md)。
