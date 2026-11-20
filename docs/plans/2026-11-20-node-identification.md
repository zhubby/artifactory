<!--
[INPUT]: 用户确认的设备命名规则与当前节点契约。
[OUTPUT]: 约束设备识别、稳定别名、兼容上报与部署验收。
[POS]: 设备命名改进的实施和交付依据。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备命名与辨识度改进

## Global Constraints

- 注册默认使用系统主机名；已保存名称保持稳定。管理员可改名或使用当前主机名。
- 现有两台节点改为真实主机名；不重新注册，不更换 UUID、证书、节点池或任务关系。
- 所有操作反馈使用顶部 notify；首读骨架、前台顶部进度、后台轮询静默。
- 列表不能按节点额外请求元数据或遥测。保持原有租户与项目可见边界。
- 不更改 Telemetry Batch、Inventory、NetworkSample 的严格 JSON 契约或 protobuf 编号。
- SDK、签名材料、现有身份、未确认日志和遥测队列必须保留。完成后清理临时程序副本。
- 使用有意义的行为测试，记录先失败后通过的证据。修改后同步 L3/L2 和文档。
- 三个实现任务的文件边界互不重叠；共享接口在本计划中锁定。提交由主线程统一执行。

## Task 1: 控制平面名称与识别摘要

负责 crates/control-plane、crates/emissary-protocol 的名称校验辅助函数及其测试。不要改 web、crates/emissary 或 docs。

- 新增 PATCH /api/v1/nodes/{id}/name，body 仅有 name，成功返回 {"ok":true}。复用租户 owner/admin 会话、CSRF、可写租户与归属校验。普通成员、项目 admin、API Token 不能改名。
- 复用 nodes.name，无迁移。修改仅写名称，SQL 显式限制 tenant_id，保留其他字段。记录 node_renamed 审计，包含 old_name/new_name；心跳或重连不能覆盖。
- 在 emissary-protocol 的 nodes 模块导出 normalize_node_name(&str)，trim 后拒绝空值、控制字符、超过 100 UTF-8 字节。注册 Enroll 和改名共用该校验。函数错误类型沿用协议 anyhow::Result。不要改变已有名称。
- GET /nodes 新增可选 identity 摘要：hostname、device_model、device_vendor、cpu_model、memory_total_bytes。来源依次为 nodes.metadata 的 system.hostname、hardware.device_model、hardware.device_vendor、hardware.cpu_model、hardware.physical_memory_bytes。缺失字段省略；仅投影正确类型的值，不返回完整 metadata。
- identity 可见条件与现有 diagnostics 相同。项目受限视角仍返回空摘要。搜索增加架构、节点 UUID、可见的主机名/型号/CPU，不能用隐藏字段泄漏跨范围信息。保留列表分页和查询预算。
- 测试：改名权限/跨租户/CSRF/归档租户；校验中文和字节边界；审计、搜索；池/槽位/证书/任务保持；改名后心跳、同启动重连、新启动均不覆盖名称；大元数据列表仅投影摘要。运行必要数据库测试，不把缺少数据库视为通过。
- 先写针对缺失行为的失败测试，再实现。记录测试命令/结果到 task-1-report.md。更新所修改模块 AGENTS/L3。不要提交，不生成其他任务改动，不派生子代理。

## Task 2: Emissary 主机识别与网络快照

只负责 crates/emissary 及其测试/模块地图，不修改 control-plane、protocol、web 或 docs。

- 修复 doctor.hostname 在 runner 心跳重组 diagnostics 时丢失的问题。每次心跳保留缓存的 hostname，不能只修 hello。维持现有版本、指标与日志缓冲诊断。
- 启动 metadata 补充 system.hostname、hardware.device_model、hardware.device_vendor。主机名复用 doctor；型号/厂商复用 sysinfo::Product，不猜营销名、不采集序列号。CPU/内存沿用现有字段。
- 保持同一 startup 的 metadata 不可变；普通心跳和网络重连不做同步硬件探测。
- 后台 Collector 缓存本地接口地址，在 diagnostics.network_interfaces 上报 {sampled_at:UTC ISO8601,interfaces:[{name,addresses:[IP字符串]}],truncated:boolean}。不使用服务端 peer，不上报 MAC。
- 地址排序、去重，过滤回环和未指定地址，保留 VPN/私网/IPv6；按接口名稳定排序，最多 16 个接口、每个最多 8 个地址，整体 JSON 不超过 8 KiB，截断时 truncated=true。接口名超过128 UTF-8字节则跳过并标记截断。采集结果通过缓存供心跳读取。
- 不扩展严格 Telemetry Batch/Inventory/NetworkSample；旧离线批次的原始字节和摘要保持兼容。不需要新增依赖。
- 新注册 CLI 调用 Task1 提供的 artifactory_emissary_protocol::nodes::normalize_node_name，默认名继续为系统主机名，回退 emissary。若该函数暂未落盘，可先写测试并等待集成。
- 测试：hostname 端到端心跳与启动元数据；缺失型号；同启动重连快照稳定；多网卡/地址变化/乱序/去重/过滤/截断；离线旧队列和 wire compatibility。先失败后实现。
- 测试命令和结果写 task-2-report.md。更新 L3/L2。不要提交，不派生子代理。

## Task 3: 节点界面与重命名

只负责 web，包括类型、消息、组件、样式、夹具和浏览器测试，不修改 Rust 或 docs。

- Node.identity 可选，字段 hostname/device_model/device_vendor/cpu_model 为可选字符串，memory_total_bytes 为可选数值。旧节点缺字段保持兼容。
- Node.diagnostics.network_interfaces 可选，结构为 {sampled_at:string,interfaces:{name:string,addresses:string[]}[],truncated:boolean}。
- 节点列表主标题显示 node.name；身份行显示主机名（与标题不同时）、系统/架构、8位短节点ID；硬件行显示型号、CPU和内存，缺项省略、重复信息去重。不猜营销型号。保持状态、池、槽位和详情按钮，长文本在手机宽度不挤压按钮或溢出，支持查看完整值。
- 不为每张卡片额外请求数据。搜索复用现有 q API，提示支持名称、主机名、型号、CPU、ID。
- 详情标题附近提供清晰的重命名入口，只有现有 admin 且非只读范围可编辑。使用独立保存/取消表单和名称输入框，提供“使用当前主机名”。主机名缺失时禁用该动作并说明。客户端 trim/控制字符/100 UTF8字节校验与服务端一致。
- PATCH /nodes/{id}/name 仅提交 {name}；不回写其他设置。成功 notify 并刷新当前列表/详情；失败保留输入并通过 notify 报错。后台轮询不能重置编辑草稿；改名导致筛选不匹配时沿用现有关闭选中项行为。
- 概览显示独立主机名、型号/厂商与CPU信息；网络地址按接口显示 IPv4/IPv6、采样时间与截断提示，离线保留最后快照，不把某个IP标为唯一身份。读取时兼容旧 diagnostics.hostname。
- 全部文案提供中英；遵守骨架、顶部进度、notify、自定义下拉、键盘/焦点与未保存保护既有模式。新增组件保持单文件不超过800行，避免扩大已偏长的详情组件。
- 先写失败行为测试，再实现。覆盖改名成功/失败/只读、草稿不被轮询重置、使用主机名、UTF8校验、旧节点、重名短ID、长文本、手机/桌面、深浅主题、无逐卡请求。运行 typecheck、生产构建及专项 Playwright。
- 命令/结果写 task-3-report.md。更新 L3/L2。不要提交，不派生子代理。

## Task 4: 集成、文档与交付

- 主线程审查跨任务契约，执行后端数据库、协议、Emissary和Web专项回归，独立审查实现并修复真实问题。
- 同步设计规范、API、节点观测与操作文档；记录最终验收边界。
- 提交并合并 main、推送。固定源码构建控制平面与Web镜像，预览只允许目标镜像变化，保留Secret/PVC/服务配置后部署K8s。
- 固定同一源码构建两平台Emissary。先Mac灰度，再Windows；在空闲且队列保留的边界替换标准服务程序，不重新注册。
- 新上报主机名通过后，按精确节点ID将 ai-workspace 两台改名：Mac c30da6b0-52f4-403e-a03b-b7909b1635bc，Windows 05c69825-ffc7-46f4-88fe-2cdada414b38。
- 验证心跳、遥测、UUID/证书/池不变，真实页面名称/硬件/网络/搜索/只读/重命名稳定。清除本次临时凭据和构建/备份，保留必要回执。

## 进度

- [x] Task 1 控制平面和名称校验。
- [x] Task 2 Emissary。
- [x] Task 3 Web。
- [x] Task 4 集成和交付。

交付源码为 `e0d56c1d90aa55aab7e6dbd2b3fc01405f1e68e3`。
Helm revision 10、双设备升级与清理已验收。
详细结果见[设备识别上线记录](../releases/2026-11-20-node-identification-kubernetes.md)。

[PROTOCOL]: Update this header on change, then check AGENTS.md
