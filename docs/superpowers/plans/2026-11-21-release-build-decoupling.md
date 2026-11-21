<!--
[INPUT]: 已确认的 Release 简化设计与现有构建、源码及正式交付约束。
[OUTPUT]: 定义版本元数据与构建配置解耦的实施步骤和验证边界。
[POS]: 替代旧 Release 目标合同的实施方案；历史部署验收保持原始事实。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Release 与构建解耦实施方案

**目标：** Release 业务字段仅包含不可变 SemVer 版本与可编辑版本说明。
**架构：** Build 独立保存源码和目标快照。正式交付封存一个成功 Build。
**技术栈：** Rust 2024、Toasty 0.11.0、PostgreSQL 17、Next.js 16、React 19。
**设计依据：** 用户在当前聊天确认的方案。

## 全局约束

- Release 使用严格 SemVer，同项目唯一，创建后不改版本号。
- notes 为可留空多行文本，各发布状态均可编辑，不写入冻结交付清单。
- 每次新 Build 独立选择 ref、goal、target_requirements、targets 和 parameters。
- Build 仍属于 Release；本次构建快照项目仓库与凭据引用。
- 新 Build 独立解析源码；已有 Build 的执行重试保持原源码。
- 正式发布只选一个成功 Build。整个 Release 的任务和 Pod 停止后封存。
- 封存后禁止新增构建和执行重试。发布失败只重试同一交付。
- 正式文件和镜像均必须来自选中 Build。版本标签、发布租约与摘要准入继续生效。
- 仅更新初始化基线并使用隔离数据库验收。不部署或重置未指定的运行实例。
- 变更同步 L3、L2 和 L1 文档。

## Task 1: 后端模型、源码归属和正式交付

- [x] 创建 Release 只接收 `{version,notes?}`；修改只接收 `{notes}`。
- [x] 构建创建接收 `{ref,goal,target_requirements,targets,parameters?}`，保留现有验证。
- [x] Release 移除源码、目标和 spec_revision；Build 保留自己的不可变输入。
- [x] SourceTask 使用 build_id 归属；移除冗余 release_id；pin、取消、失败和提交只作用于所属 Build。
- [x] 发布依据所选 Build 源码与目标；交付清单 schema_version 为 2。
- [x] 先插入 delivery，再关联文件和镜像；数据库检查精确 Build 归属。
- [x] 保留版本注入、租约、摘要围栏与 AI 计划自身的修订机制。
- [x] 初始化基线改为 2026112101；同步 DTO、触发器及相关 Rust 测试。
- [x] 验证源码隔离、取消隔离、版本说明编辑、发布完整性和跨构建制品拒绝。

## Task 2: 控制台与浏览器验证

- [x] Release 表单只保留版本号和说明，列表与概览移除源码与合同修订。
- [x] BuildWizard 接收 Project，复用源码引用查询和目标编辑控件。
- [x] Build 响应和列表展示本次来源与目标；发布候选显示引用、SHA 和目标摘要。
- [x] 目标类型与校验器改为构建领域命名；说明不充当 goal。
- [x] 中英文文案、夹具、未保存输入、权限和响应式测试同步更新。
- [x] 运行 TypeScript 检查与相关 Playwright 桌面、平板、手机测试。

## Task 3: 文档、集成检查和审查

- [x] 更新 Release/API/部署基线文档及 GEB 地图；历史验收保留。
- [x] 运行 Rust 单元测试、隔离数据库合同测试和受影响脚本验证。
- [x] 独立审查源码归属、SQL 不可变约束、交付事务及控制台接口一致性。
- [x] 修复审查发现并复验，记录最终检查证据。

## 默认值

notes 省略时保存空字符串，按普通多行文本展示。构建 ref 默认采用仓库默认分支；用户可另选分支、Tag 或输入引用。保留现有目标输入结构与限制。元数据更新不产生构建目标修订。

## 实施与验证结果

- 后端单元与源码回归：67 项通过。
- 隔离 PostgreSQL/S3 集成回归：115 项通过；设备群负载项按环境约定过滤。
- 初始化基线与旧基线拒绝：2 项通过。
- Clippy（control-plane 全 target）、变更 Rust 格式与 diff 空白检查通过。
- Python 脚本 58 项、Operator 离线验收 38 项、镜像离线验收 23 项通过；Go 源码夹具通过。
- 前端类型检查、webpack 生产编译通过。共享依赖软链导致 standalone 跟踪路径警告；该产物不用于部署。
- 浏览器覆盖三视口版本说明、独立构建、发布与来源边界；扩展回归覆盖语言、主题、无障碍、日志和导航。具体命令与计数记入实施台账。
- 文档构建与链接检查通过：93 章、95 页面。依赖边界检查通过。
- 两轮独立审查通过。修复发布弹窗被轮询自动改选构建的问题，并增加先红后绿回归。

代码位于 `codex/release-build-decoupling`。未部署、未重置既有实例、未提交或推送。初始化基线仅用于新数据库，旧基线继续显式拒绝。
