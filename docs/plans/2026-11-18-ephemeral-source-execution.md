---
title: 临时源码与任务独立拉取
status: active
date: 2026-11-18
---

# 临时源码与任务独立拉取

平台持久化源码元数据、AI 对话、日志、执行计划和制品。
源码工作目录、Git/LFS 缓存和 BuildKit 中间缓存随任务清理。
AI 工具引用的源码片段随对话保存，最终镜像作为制品保存。

## 实施单元

- [x] 统一 SourceManifest，新增共享 checkout 库和 source-preparer。
- [x] Agent Operator 用临时 SourceTask 固定提交和查询 refs。
- [x] AI 和镜像任务用 init container、emptyDir 和任务身份领取凭据。
- [x] 原生设备每次拉取，删除持久缓存，补齐异常清理。
- [x] Runtime 按 generation 保存对话，重试开启新会话并保留累计预算和 proposal。
- [x] 控制平面替换源码模型，提供任务材料、固定提交与对话查询接口。
- [x] 删除 Source/Poll 服务、协议、缓存、部署及旧数据库迁移。
- [x] 删除 Registry 构建缓存，任务终态后确认 Pod 和临时目录清理。
- [x] 更新控制台、交付镜像、Helm、文档和验收。
- [ ] 完成测试、代码审查和实际环境验收。

## 不变量

首次解析以 CAS 固定 ref 对象和提交，完整 Manifest 提交后开始规划。
AI、所有架构和重试使用同一 Manifest，不能重新跟随分支。
源码凭据不进入持久任务对象、仓库 URL 或主容器。
完整完成消息持久化；异常中断保留已确认消息。
proposal 与回执幂等，Pod 丢失不能重复生成任务。
只保留 Kubernetes 规划；原生设备继续执行构建。
不保留兼容分支、双读、旧字段别名或数据迁移程序。

## 清理与部署

源码准备默认 15 分钟，受剩余任务期限限制。
ImageBuild 整次终态且结果、日志已持久化后回收所有 Job/Pod。
终态 Job TTL 为 300 秒，不能提前清理仍用于判断架构完成的 Job。
原生设备确认进程退出后清理，清理失败暂停接单。
保留已安装工具链和 SDK，临时配置及可写缓存按任务隔离。

用户已选择重置现有原型环境。
重置前阻断写入口，停止旧任务，确认在途写入与票据失效。
业务与 Runtime 使用新初始化基线，设备重新注册。
保留制品和 Registry 物理对象、PKI 与主密钥。
旧业务目录及授权不恢复，物理对象不自动重新出现在控制台。
旧 Source 存储按专属资源与前缀清理，Registry 缓存通过安全 GC 清理。

## 验收

覆盖 HTTPS/SSH、子模块、跨源凭据、LFS、分支移动及固定对象不可获取。
覆盖 Pod 丢失、消息重发、跨项目鉴权、proposal ACK 丢失和预算不重置。
覆盖失败、取消、超时、OOM、Operator 重启、原生进程强杀与重启清理。
完成 macOS、Windows、amd64/arm64 镜像、内外仓库推送及制品下载验收。
检查无 Source/Poll 常驻资源、源码 PVC、源码包及 Registry 缓存 manifest。

[PROTOCOL]: Update this header on change, then check AGENTS.md
