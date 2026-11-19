---
title: 模块边界与规划命名
type: refactor
status: completed
date: 2026-11-19
---

<!--
[INPUT]: 已批准的四项模块调整与完整规划身份改名。
[OUTPUT]: 固定职责边界、新实例约束和完成清单。
[POS]: 当前模块重构的实施记录；验收后更新状态。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->

# 模块边界与规划命名

用户确认执行四项架构调整。
改名覆盖代码、构建产物、部署资源和规划协议身份。
本次只支持新实例，不执行旧环境数据迁移。

## 实施单元

| 调整 | 结果与依赖边界 |
| --- | --- |
| 拆分 `runtime-protocol` | `planning-protocol` 只包含规划数据契约；`service-transport` 提供 HTTP/TLS、服务认证、密封凭据、代理与有界指标 |
| 解除镜像模块对节点程序的依赖 | `execution-support` 提供进程、事件、私有文件和流式脱敏；Image Operator 直接依赖它和 `source-checkout`，不依赖 Emissary |
| 统一 AI 规划命名 | `planner-runtime` → `planning-coordinator`；`agent-operator` → `planning-operator`；`agent-worker` → `planning-worker` |
| 合并节点契约 | 将 `telemetry-model` 并入 `emissary-protocol::{telemetry,nodes}`；保留 `signing` 与生成 RPC 的根导出 |

规划协调器继续内嵌 Control，不新增独立服务。
SourceTask 继续由 Planning Operator 编排。
节点会话恢复、OTA 和签名执行留在 Emissary。
BuildKit 监督入口保持最小依赖与独立 feature 边界。
现有 OCI 客户端名称沿用 `oci-registry-client`。

- [x] 完成契约拆分并迁移全部消费者。
- [x] 抽取共享执行原语，迁移进程、脱敏和文件权限回归。
- [x] 完成规划目录、包名、命令、镜像与 Chart 改名。
- [x] 合并遥测契约并删除旧 crate 与依赖。
- [x] 更新根地图、模块地图、文件契约与现行手册。

## 规划身份与持久化

| 接口或身份 | 当前契约 |
| --- | --- |
| API 基址 | `/internal/planning` |
| 业务授权桥 | `/internal/planning/event`、`tool`、`source-material` |
| 环境变量 | `ARTIFACTORY_PLANNING_URL`、`ARTIFACTORY_PLANNING_DATABASE_URL` |
| CRD | `planning.artifactory.dev/v1alpha1`；`PlanningPool`、`PlanningRun`、`SourceTask` |
| 规划 Pod 身份 | ServiceAccount `artifactory-planning-worker`，audience `artifactory-planning` |
| Operator Lease | `artifactory-planning-operator` |
| 业务初始化基线 | `2026111902` |
| 规划初始化基线 | `2026111901` |

规划数据库表、通知和指标使用规划名称。
Node.js 模型客户端身份同步改为 planning worker。
不保留旧路由、配置别名、CRD 兼容层或旧包壳。
Emissary Protobuf、遥测 JSON 字节与摘要保持原契约。
数据库、HTTP、执行支持与规划协议之间不得形成依赖环。

## 部署边界

程序拒绝旧数据库基线，不执行自动重置。
新 Operator 不接管旧 CR、Pod 或 Lease。
安装不删除旧部署、CRD、PVC、数据库、对象或密钥。
新旧实例使用独立数据库、资源名称和命名空间。
旧实例由管理员独立备份、排空和退役。
设备向新实例重新注册，旧业务列表和授权不导入。
Kubernetes 最低版本保持 1.28，不新增版本分支。

本实施记录不代表已提交、发布或变更现有部署。
集群验证只使用本次新建的隔离 kind。
历史数据处置不属于本次执行。

## 验证清单

- [x] Rust 工作区 locked check、格式和相关测试通过。
- [x] 依赖图无环；Image Operator 不再依赖 Emissary。
- [x] 纯规划协议不引入 HTTP、数据库或密码学依赖。
- [x] BuildKit 监督入口在关闭 Operator feature 后通过构建。
- [x] 进程树取消、管道 EOF、私有文件权限与跨块脱敏通过。
- [x] 规划代次围栏、丢失恢复、预算、截止时间与幂等回执通过。
- [x] 新业务库和规划库初始化通过；旧基线拒绝且数据不变。
- [x] 遥测 JSON 字节、内容摘要、元数据摘要和 RPC 编码通过。
- [x] 节点新鲜度、授权、遥测重放与历史查询回归通过。
- [x] Planning Worker 类型检查与单元测试通过。
- [x] CRD 生成与签入内容一致，Helm lint/template 通过。
- [x] Kubernetes 1.28 资源契约、版本脚本和交付矩阵通过。
- [x] 文档目录、内部链接、当前称谓和 GEB 地图检查通过。

## 验证结果

- 工作区全部目标 check、clippy `-D warnings` 与格式检查通过。
- 普通 Rust 测试 219 项通过；隔离数据库与服务测试 170 项通过。
- 两套旧基线拒绝测试均确认旧数据和迁移记录不变。
- Worker 类型检查与 19 项测试通过；Python 工具测试 94 项通过。
- 四个 Chart lint 与 Kubernetes 1.28/1.37 模板渲染通过。
- 规划 CRD 生成结果与 Operator、Chart 两份交付内容一致。
- Kubernetes 1.28 和 1.37 各 9 个 Pod 生命周期场景通过。
- 1.37 同时保留旧 SourceTask CRD，确认新组的查询和取消不受影响。
- 依赖方向、数据访问边界与全项目版本检查通过。
- 文档验证覆盖 82 章节、84 HTML、10657 链接与资源。

数据库测试只创建临时库和随机 schema。
对象测试只使用本次随机 Bucket。
测试结束已删除临时数据库和 Bucket。
Pod 生命周期证据保存在 `.local/module-refactor/`。
两套测试命名空间均在验收结束后正常收尾。
本次创建的两套 kind 集群已删除。
这些场景用 fixture 进程替代 Git、AI 和 BuildKit。
真实业务镜像构建未在本次重新运行。
Windows、Linux musl 与实体设备仍由原生 CI 或专用设备验收。
现有实例、持久卷、凭据和业务数据未变更。
