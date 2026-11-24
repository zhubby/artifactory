# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 通过固定宿主与响应式插槽保持单实例账户菜单、个人设置草稿和焦点。
ai-settings.tsx: 使用共享下拉配置任意模型、调用预算与独立接口兼容字段。
auth-shell.tsx: 渲染认证页面布局。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 组合过程、计划、执行与制品；首次展示过程，轮询保留用户标签。
build-process.tsx: 展示真实阶段、当前动作、失败分类和有界事件，保留展开状态。
build-log-history.tsx: 首读显示日志骨架，按 attempt 和步骤分页，追加旧日志时保留输出与阅读锚点。
build-planning.tsx: 组合当前计划、稳定历史选择与澄清草稿，保留实时统计回退和按需审计。
build-plan-target.tsx: 结构化呈现原生顺序步骤与 OCI 参数，按计划身份对齐执行并标明沿用成功来源。
planning-conversations.tsx: 按规划轮次读取审计，展示关联动作和详情，保留选择、分页与静默补读。
planning-audit-content.tsx: 安全渲染任务输入、审计正文、源码、日志与搜索结果，提供参数和原文详情。
build-table.tsx: 展示构建的 Release 归属、独立源码与目标和执行状态。
build-wizard.tsx: 组合四步弹窗、授权设备默认目标、答案草稿和最终确认提交。
build-target-picker.tsx: 按项目授权设备禁选原生平台，高级设置遵守同一设备约束。
common.tsx: 提供内容骨架、正文标题动作与名称附件，统一字段、通知及下载反馈。
command-palette.tsx: 复用当前范围的业务与设置导航，支持项目切换并恢复触发焦点。
console.tsx: 由租户布局持续挂载，校验地址并按租户与项目隔离工作区，组合顶栏、权限骨架与项目星标。
dashboard.tsx: 读取完整窗口指标和近期活动，范围切换使用共享下拉，后台刷新保留内容。
dashboard-charts.tsx: 按需加载 Recharts，呈现当前构建结果和成功构建周转时长，保留空样本断点。
emissary-ota.tsx: 等待节点池和发布选项后开放表单，历史首读可重试，轮询保留池策略草稿。
forms.tsx: 提供项目、节点、凭据表单，复用下拉并统一提交通知。
lifecycle.tsx: 分别等待影响与当前操作，使用 HTTP 幂等键提交归档、恢复与删除。
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
settings.tsx: 配置令牌、源码凭据和审计，首读失败提供重试。
signing-settings.tsx: 等待签名预览和必需凭据后开放配置，按项目身份保留默认方案草稿。
space-switcher.tsx: 切换租户和项目，查询失败通过全局通知反馈。
tenant-chooser.tsx: 展示租户卡片与邀请，HTTP 创建使用共享幂等键，首读骨架且失败可重试。
top-progress.tsx: 根据并发前台计数延迟显示顶部细进度条，最后请求结束后淡出。
tenant-members.tsx: 配置租户与项目成员，邀请首读显示列表骨架并提供失败重试。
workspace-management.tsx: 按权限渲染设置分区，必需资源就绪后挂载表单，按分区隔离状态。
workspace-navigation.tsx: 渲染作用域侧栏与面包屑，权限首读显示分组骨架，失败可重试。
ui/: 提供统一基础控件。

header-tools.tsx: 工作区、租户大厅与通知页共用文档、账户插槽和最右铃铛。
image-builds.tsx: 只读展示镜像候选与增量平台日志，后台轮询和终态补读静默，正式分发从 Release 发起。
image-catalog.tsx: 按项目查询正式镜像、所属版本与来源构建，复制摘要拉取引用。
image-credentials.tsx: 配置仓库凭据和 BuildKit Secret，列表首读显示骨架并提供失败重试。

notification-provider.tsx: 在账户边界维护单 SSE 连接，合并失效并静默补读摘要。
notification-center.tsx: 顶栏铃铛与独立页面共用抽屉和未读数，后台重验保留原阅读窗口及滚动。
notification-settings.tsx: 配置站内偏好、邮箱订阅及项目名称旁的关注星标，保存失败保留既有状态。
notification-webhooks.tsx: 在管理员设置配置端点、一次密钥、测试与投递重试。

[PROTOCOL]: Update this header on change, then check AGENTS.md
