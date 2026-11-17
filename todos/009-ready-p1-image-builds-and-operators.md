---
status: ready
priority: p1
issue_id: "009"
tags: [operators, buildkit, registry, gateway]
dependencies: []
---

## Problem Statement

平台需要独立镜像构建服务、内建 OCI 仓库和统一反向代理入口。

## Findings

实施前，Operator 共用多角色镜像，执行器仅接受 native。
实现复用固定源码、S3、租户授权和控制副本协调。

## Proposed Solutions

执行已批准的 `docs/superpowers/plans/2026-11-17-image-builds-and-operators.md`。

## Recommended Action

代码实现和本机验证已完成。按实施计划执行剩余真实集群验收。

## Acceptance Criteria

- [x] Agent Operator 兼容迁移并独立交付。
- [x] Image Operator 实现双架构构建和独立外部发布。
- [x] Registry 与制品接入 Pingora 反向代理。
- [x] 本机权限、恢复、协议、界面与交付清单回归通过。
- [ ] 真实集群通过双架构构建、Distribution 和外部仓库验收。

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

[PROTOCOL]: Update this header on change, then check AGENTS.md
