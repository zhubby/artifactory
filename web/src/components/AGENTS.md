# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 提供当前账户和个人设置。
ai-settings.tsx: 配置模型与固定推理预算。
auth-shell.tsx: 渲染认证页面布局。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 订阅活跃构建，展示应用分类、宿主、终态日志与制品。
build-log-history.tsx: 按 attempt 和步骤分页读取静态日志并保持阅读锚点。
build-planning.tsx: 展示应用分类、宿主、规划、澄清与持久对话入口。
planning-conversations.tsx: 按 generation 分页读取审计，重开补读终态消息。
build-table.tsx: 展示项目构建列表。
build-wizard.tsx: 组合引用查询、三步构建引导和默认签名身份预览。
common.tsx: 提供应用与宿主标签、共享字段、状态和制品验签表。
command-palette.tsx: 按权限搜索项目与页面，关闭后恢复焦点。
console.tsx: 组合工作区路由和业务页面。
emissary-ota.tsx: 配置节点发布与升级目标。
forms.tsx: 提供提交状态和业务编辑表单。
lifecycle.tsx: 执行工作区归档、恢复和删除交互。
node-card.tsx: 展示设备资源行与状态。
node-detail-dialog.tsx: 组合节点详情弹窗。
node-detail-parts.tsx: 提供节点详情共享视图。
node-metadata.tsx: 展示节点能力与元数据。
nodes-workspace.tsx: 查询、筛选和配置节点。
preferences-controls.tsx: 切换语言、主题和动效偏好。
providers.tsx: 组合账户、语言与视觉偏好上下文。
release-detail.tsx: 展示不可变发布、原生制品与镜像摘要。
settings.tsx: 配置令牌、源码凭据和审计。
signing-settings.tsx: 导入凭据、停用版本并在方案就绪后初始化项目默认选择。
space-switcher.tsx: 切换租户与项目工作区。
tenant-chooser.tsx: 使用统一样式展示租户选择、创建和邀请入口。
tenant-members.tsx: 配置租户与项目成员。
workspace-management.tsx: 组合租户与项目设置，包括应用签名。
ui/: 提供统一基础控件。

image-builds.tsx: 展示日志与完整仓库摘要引用，提供独立发布操作。
image-catalog.tsx: 按项目查询镜像与来源构建，复制摘要拉取引用。
image-credentials.tsx: 配置项目仓库凭据与 BuildKit Secret 引用。

[PROTOCOL]: Update this header on change, then check AGENTS.md
