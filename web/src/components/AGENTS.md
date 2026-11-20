# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 提供个人设置和跨导航操作通知，保存反馈不占面板布局。
ai-settings.tsx: 配置模型与固定推理预算。
auth-shell.tsx: 渲染认证页面布局。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 展示版本内构建、目标修订、日志与候选，统一操作通知，保留可访问持久状态。
build-log-history.tsx: 首读显示日志骨架，按 attempt 和步骤分页，追加旧日志时保留输出与阅读锚点。
build-planning.tsx: 展示应用分类、宿主、规划骨架、澄清与对话入口，终态补读静默。
planning-conversations.tsx: 首次展开使用骨架，按 generation 分页读取审计，后台轮询和终态补读静默。
build-table.tsx: 展示构建的 Release 归属、目标修订和执行状态。
build-wizard.tsx: 在 Release 内启动构建，提交补充要求与参数并继承版本合同。
common.tsx: 提供内容骨架、共享字段、持久状态、无布局错误通知桥及复制下载反馈。
command-palette.tsx: 复用当前范围的业务与设置导航，支持项目切换并恢复触发焦点。
console.tsx: 组合工作区数据、请求生命周期和业务页面，交由独立组件呈现导航。
emissary-ota.tsx: 配置节点发布与升级目标。
forms.tsx: 提供项目、节点、凭据表单，统一提交成功与错误通知。
lifecycle.tsx: 提交归档、恢复与删除；设置只保留当前操作撤销与重试，反馈通过顶部通知。
node-card.tsx: 展示设备资源行与状态。
node-detail-dialog.tsx: 组合节点详情弹窗。
node-detail-parts.tsx: 提供节点字段、格式化时间与失败重试空态，错误详情通过通知。
node-metadata.tsx: 按需读取节点元数据，首读显示详情骨架，刷新保留快照并提供失败重试。
nodes-workspace.tsx: 查询、筛选和配置节点。
notifications.tsx: 顶部居中显示通知，避开手机顶栏与抽屉导航，桥接校验和弹窗焦点。
preferences-controls.tsx: 为登录页和个人设置提供语言、主题选择。
providers.tsx: 组合账户、语言、主题、顶部进度和全局通知，动效适配系统偏好。
release-detail.tsx: 组合版本工作台，编辑使用固定修订快照，已发布镜像分发使用固定版本标签。
release-form.tsx: 创建严格 SemVer 版本并编辑草稿目标合同，版本和源码引用创建后只读。
release-table.tsx: 展示版本、生命周期、源码、目标修订和构建次数。
settings.tsx: 配置令牌、源码凭据和审计。
signing-settings.tsx: 导入凭据、停用版本并在方案就绪后初始化项目默认选择。
space-switcher.tsx: 切换租户和项目，查询失败通过全局通知反馈。
tenant-chooser.tsx: 展示紧凑租户卡片、创建和邀请，首次读取使用骨架，失败提供重试。
top-progress.tsx: 根据并发前台计数延迟显示顶部细进度条，最后请求结束后淡出。
tenant-members.tsx: 配置租户与项目成员。
workspace-management.tsx: 渲染规范设置分区及授权内容，导航由工作区侧栏提供。
workspace-navigation.tsx: 渲染作用域侧栏与面包屑，版本查询错误通过通知反馈。
ui/: 提供统一基础控件。

image-builds.tsx: 只读展示镜像候选与增量平台日志，后台轮询和终态补读静默，正式分发从 Release 发起。
image-catalog.tsx: 按项目查询正式镜像、所属版本与来源构建，复制摘要拉取引用。
image-credentials.tsx: 配置项目仓库凭据与 BuildKit Secret 引用。

[PROTOCOL]: Update this header on change, then check AGENTS.md
