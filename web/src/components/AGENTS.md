# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 通过单一个人设置入口管理资料、安全和偏好，保留业务页面状态。
ai-settings.tsx: 配置模型与固定推理预算。
auth-shell.tsx: 渲染认证页面布局。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 展示 Release 内的构建、目标修订、日志与候选制品，返回所属版本。
build-log-history.tsx: 按 attempt 和步骤分页读取静态日志并保持阅读锚点。
build-planning.tsx: 展示应用分类、宿主、规划、澄清与持久对话入口。
planning-conversations.tsx: 按 generation 分页读取审计，重开补读终态消息。
build-table.tsx: 展示构建的 Release 归属、目标修订和执行状态。
build-wizard.tsx: 在 Release 内启动构建，提交补充要求与参数并继承版本合同。
common.tsx: 提供应用与宿主标签、共享字段、状态和制品验签表。
command-palette.tsx: 复用当前范围的业务与设置导航，支持项目切换并恢复触发焦点。
console.tsx: 组合工作区数据、请求生命周期和业务页面，交由独立组件呈现导航。
emissary-ota.tsx: 配置节点发布与升级目标。
forms.tsx: 提供提交状态和业务编辑表单。
lifecycle.tsx: 执行工作区归档、恢复和删除交互。
node-card.tsx: 展示设备资源行与状态。
node-detail-dialog.tsx: 组合节点详情弹窗。
node-detail-parts.tsx: 提供节点详情共享视图。
node-metadata.tsx: 展示节点能力与元数据。
nodes-workspace.tsx: 查询、筛选和配置节点。
preferences-controls.tsx: 为登录页和个人设置提供语言、主题选择。
providers.tsx: 组合账户、语言与主题上下文，动画默认开启并自动适配系统动效偏好。
release-detail.tsx: 组合版本工作台，编辑使用固定修订快照，已发布镜像分发使用固定版本标签。
release-form.tsx: 创建严格 SemVer 版本并编辑草稿目标合同，版本和源码引用创建后只读。
release-table.tsx: 展示版本、生命周期、源码、目标修订和构建次数。
settings.tsx: 配置令牌、源码凭据和审计。
signing-settings.tsx: 导入凭据、停用版本并在方案就绪后初始化项目默认选择。
space-switcher.tsx: 在顶栏切换租户、在项目侧栏切换项目，列表切换保留当前栏目。
tenant-chooser.tsx: 使用统一样式展示租户选择、创建和邀请入口。
tenant-members.tsx: 配置租户与项目成员。
workspace-management.tsx: 渲染规范设置分区及授权内容，导航由工作区侧栏提供。
workspace-navigation.tsx: 按租户、项目和设置范围渲染单一侧栏，复用定义生成顶栏面包屑。
ui/: 提供统一基础控件。

image-builds.tsx: 只读展示镜像候选、独立分发状态与平台日志，正式分发从 Release 发起。
image-catalog.tsx: 按项目查询正式镜像、所属版本与来源构建，复制摘要拉取引用。
image-credentials.tsx: 配置项目仓库凭据与 BuildKit Secret 引用。

[PROTOCOL]: Update this header on change, then check AGENTS.md
