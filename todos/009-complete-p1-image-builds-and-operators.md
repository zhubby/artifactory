---
status: complete
priority: p1
issue_id: "009"
tags: [operators, buildkit, registry, gateway]
dependencies: []
---

<!--
[INPUT]: 镜像实施计划、本机回归与真实集群证据。
[OUTPUT]: 记录镜像交付完成项和真实环境验证范围。
[POS]: todos/ 的镜像构建交付状态。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

## Problem Statement

平台需要独立镜像构建服务、内建 OCI 仓库和统一反向代理入口。

## Findings

实施前，Operator 共用多角色镜像，执行器仅接受 native。
实现复用固定源码、S3、租户授权和控制副本协调。

## Proposed Solutions

执行已批准的 `docs/superpowers/plans/2026-11-17-image-builds-and-operators.md`。

## Recommended Action

四个 Helm release 已部署到现有 Kubernetes 1.28.3。
原生 amd64 与 arm64、显式仿真、外仓和超时分别完成真实验收。
正式集群只有 amd64 节点，arm64 使用 node239 显式仿真。
原生 arm64 来自独立 1.28 环境，不混记为正式节点能力。
完整结果见 `docs/image-builds-kubernetes-128-acceptance-2026-11-18.md`。

## Acceptance Criteria

- [x] Agent Operator 按新契约独立交付，不保留兼容或迁移代码。
- [x] Image Operator 实现双架构构建和独立外部发布。
- [x] Registry 与制品接入 Pingora 反向代理。
- [x] 本机权限、恢复、协议、界面与交付清单回归通过。
- [x] 真实集群部署两个独立 Operator、Registry 与 Pingora。
- [x] 真实镜像任务取消清理与 Image Operator 切主恢复。
- [x] 真实集群通过 amd64 构建、Distribution 与镜像 Release 验收。
- [x] 独立 1.28 环境分别通过 arm64/amd64 原生构建，正式集群通过显式仿真。
- [x] 使用实际认证外部仓库验证发布与重试。
- [x] 独立 1.28 故障矩阵验证真实运行中任务的 DeadlineExceeded 与清理。

## Work Log

- 2026-11-17：用户批准方案。工作区初始干净，创建实施分支。
- 2026-11-17：完成独立 Operator、镜像生命周期、仓库、网关和控制台。
- 2026-11-17：修复内部重试越权、调度饥饿和交付瞬时失败。
- 2026-11-17：通过 11 项镜像数据库、6 项界面和 40 项脚本测试。
- 2026-11-17：通过 CRD 一致性、Chart、发布矩阵和前端生产构建。
- 2026-11-17：Rust 工作区默认测试、全目标 Clippy 和格式检查通过。
- 2026-11-17：Docker Hub 拉取超时，真实集群验收保留未勾选。
- 2026-11-17：修复审查的 7 项问题，补齐排空、超时、孤立日志和外部引用回归。
- 2026-11-17：用户要求合并 main、推送并部署 Kubernetes 验证。
- 2026-11-17：已部署主平台、Registry 和两个独立 Operator；原生制品下载通过。
- 2026-11-17：Image Operator 47.9 秒完成切主；运行中任务取消 4.38 秒完成清理。
- 2026-11-17：修复 Distribution 3 的 RFC 7638 密钥 ID，独立真实仓库探针返回 200。
- 2026-11-17：独立仓库身份补齐本 Bucket 的 WriteAcp，原生身份与匿名拒绝边界保留。
- 2026-11-17：原生 amd64 构建与候选推送成功，OCI Index 逐摘要读回、HEAD/Range 和 Release 均通过。
- 2026-11-17：四个 release 为主平台 15、Registry 2、Image Operator 2、Agent Operator 3；16 个工作负载就绪。
- 2026-11-17：临时代理、端口转发和诊断工作负载已清理，保留成功项目与镜像供复查。
- 2026-11-18：完成普通源码 init、Runner/BuildKit 双普通容器和 Rust PID 1 监督。
- 2026-11-18：独立 1.28 完成十项真实故障、原生双架构、显式仿真和内外仓。
- 2026-11-18：正式 1.28.3 完成切换、双副本网关、三种镜像模式和三台原生设备验收。
- 2026-11-18：修复镜像任务 CA Secret 投影，只挂载公开 ca.crt，正式容器已核对。
- 2026-11-18：取消与受控失败资源归零；旧源码物理目录、Registry 缓存和临时验收资源已清理。

[PROTOCOL]: Update this header on change, then check AGENTS.md
