---
status: ready
priority: p1
issue_id: "009"
tags: [operators, buildkit, registry, gateway]
dependencies: []
---

<!--
[INPUT]: 镜像实施计划、本机回归与真实集群证据。
[OUTPUT]: 记录镜像交付完成项和待执行的环境验证。
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

代码实现已合并 main 并推送。四个 Helm release 已部署。
当前集群的 amd64 与内建仓库完整验证通过。
保留 arm64 硬件与实际外部仓库的后续验收。

## Acceptance Criteria

- [x] Agent Operator 兼容迁移并独立交付。
- [x] Image Operator 实现双架构构建和独立外部发布。
- [x] Registry 与制品接入 Pingora 反向代理。
- [x] 本机权限、恢复、协议、界面与交付清单回归通过。
- [x] 真实集群部署两个独立 Operator、Registry 与 Pingora。
- [x] 真实镜像任务取消清理与 Image Operator 切主恢复。
- [x] 真实集群通过 amd64 构建、Distribution 与镜像 Release 验收。
- [ ] 真实集群通过 arm64/amd64 双原生架构构建。
- [ ] 使用实际外部仓库验证发布与重试。

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

[PROTOCOL]: Update this header on change, then check AGENTS.md
