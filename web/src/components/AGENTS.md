# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 提供个人设置和跨导航操作通知，保存反馈不占面板布局。
ai-settings.tsx: 使用共享下拉配置模型与固定推理预算。
auth-shell.tsx: 渲染认证页面布局。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 展示版本内构建、独立源码与目标、日志与候选，统一操作通知，保留可访问持久状态。
build-log-history.tsx: 首读显示日志骨架，按 attempt 和步骤分页，追加旧日志时保留输出与阅读锚点。
build-planning.tsx: 展示应用分类、宿主、规划骨架、澄清与对话入口，终态补读静默。
planning-conversations.tsx: 首次展开使用骨架，按 generation 分页读取审计，后台轮询和终态补读静默。
build-table.tsx: 展示构建的 Release 归属、独立源码与目标和执行状态。
build-wizard.tsx: 在 Release 内启动独立构建，选择源码并提交必填要求、目标与参数。
common.tsx: 提供内容骨架、共享字段、持久状态、无布局错误通知桥及复制下载反馈。
command-palette.tsx: 复用当前范围的业务与设置导航，支持项目切换并恢复触发焦点。
console.tsx: 组合工作区数据、请求生命周期和业务页面，交由独立组件呈现导航。
emissary-ota.tsx: 使用设备字段对齐节点版本和升级阶段，配置发布、升级目标与节点池策略。
forms.tsx: 提供项目、节点、凭据表单，复用下拉并统一提交通知。
lifecycle.tsx: 使用 HTTP 兼容幂等键提交归档、恢复与删除，保留当前操作恢复入口。
node-card.tsx: 直接消费列表快照，展示名称、主机名、短 ID、硬件与资源状态。
node-detail-dialog.tsx: 组合固定身份标题、五个详情标签和遥测生命周期，保留独立重命名。
node-overview.tsx: 先展示四项设备摘要，再展示接口与标签，按需展开完整硬件和连接信息。
node-resources.tsx: 按指标、成对内存负载字段、设备表展示遥测，手机逐行显示，采样诊断默认折叠。
node-tools.tsx: 以单列工具行展示版本和可用状态，路径按需展开，签名配置独立分组。
node-operations.tsx: 展示升级、节点配置、折叠运行诊断与授权吊销，保留轮询和嵌套弹窗流程。
node-network-addresses.tsx: 在设备摘要后按接口对齐 IPv4/IPv6、采样时间和截断状态。
node-rename.tsx: 提供具名图标入口，只提交名称，按 Unicode White_Space 裁剪后校验 UTF8 与控制字符，保护草稿和焦点。
node-detail-parts.tsx: 提供分区、折叠区、对齐字段、时间与重试，零值可见且轮询保留展开状态。
node-metadata.tsx: 按需读取元数据，直接展示普通值，复杂值与快照信息折叠，保留刷新快照。
nodes-workspace.tsx: 复用 q 查询身份字段，刷新节点列表并关闭不再匹配的详情。
notifications.tsx: 顶部居中显示通知，避开手机顶栏与抽屉导航，桥接校验和弹窗焦点。
preferences-controls.tsx: 为登录页和个人设置提供自定义语言、主题下拉。
providers.tsx: 组合账户、语言、主题、顶部进度和全局通知，动效适配系统偏好。
release-detail.tsx: 组合版本说明、构建历史与单构建交付；发布弹窗固定候选，候选失效须重选；各状态可编辑说明，已发布镜像分发使用固定版本标签。
release-form.tsx: 创建版本并编辑多行说明，版本创建后只读；源码与目标归属构建引导。
release-table.tsx: 展示版本、生命周期、说明和构建次数。
settings.tsx: 配置令牌、源码凭据和审计。
signing-settings.tsx: 导入凭据、停用版本并在方案就绪后初始化项目默认选择。
space-switcher.tsx: 切换租户和项目，查询失败通过全局通知反馈。
tenant-chooser.tsx: 展示租户卡片与邀请，HTTP 创建使用共享幂等键，首读骨架且失败可重试。
top-progress.tsx: 根据并发前台计数延迟显示顶部细进度条，最后请求结束后淡出。
tenant-members.tsx: 配置租户与项目成员。
workspace-management.tsx: 渲染规范设置分区及授权内容，导航由工作区侧栏提供。
workspace-navigation.tsx: 渲染作用域侧栏与面包屑，版本查询错误通过通知反馈。
ui/: 提供统一基础控件。

image-builds.tsx: 只读展示镜像候选与增量平台日志，后台轮询和终态补读静默，正式分发从 Release 发起。
image-catalog.tsx: 按项目查询正式镜像、所属版本与来源构建，复制摘要拉取引用。
image-credentials.tsx: 配置项目仓库凭据与 BuildKit Secret 引用。

[PROTOCOL]: Update this header on change, then check AGENTS.md
