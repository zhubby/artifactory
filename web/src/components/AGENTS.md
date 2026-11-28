# web/src/components/
> L2 | Parent: ../AGENTS.md

account.tsx: 保持单实例账户菜单、设置草稿和焦点，退出后发出活动会话清理信号。
ai-settings.tsx: 分组配置模型连接、调用预算与接口兼容字段，目录提供首读重试和键盘滚动。
auth-shell.tsx: 组合品牌玻璃雕塑、Standard 认证表单和顶栏账户插槽，避免账户浮层遮挡品牌说明。
brand-mark.tsx: 渲染 Artifactory 品牌标记。
build-detail.tsx: 组合过程、执行、计划、独立 AI 审计与制品；首次展示过程，轮询保留用户标签。
build-process.tsx: 展示当前 Pod 的源码子步骤、真实阶段和失败分类；保留事件展开状态。
build-log-history.tsx: 首读显示日志骨架，按 attempt 和步骤分页，追加旧日志时保留输出与阅读锚点。
build-planning.tsx: 组合当前计划、稳定历史选择与澄清草稿，保留实时统计回退。
build-plan-target.tsx: 结构化呈现原生顺序步骤与 OCI 参数，按计划身份对齐执行并标明沿用成功来源。
planning-conversations.tsx: 搜索已加载审计动作并筛选异常，保留轮次、分页、所选证据与阅读位置，提供手机阅读跳转。
planning-audit-content.tsx: 安全渲染任务上下文、正文、源码和日志，区分证据类型并复制当前原文。
build-table.tsx: 以可键盘滚动的表格展示构建归属、源码、目标和状态，提供具名详情入口。
build-wizard.tsx: 组合四步弹窗、授权设备默认目标、答案草稿和最终确认提交。
build-target-picker.tsx: 按项目授权设备禁选原生平台，高级设置遵守同一设备约束。
common.tsx: 提供内容骨架、正文标题动作与名称附件，统一字段、通知、下载反馈和可键盘滚动的制品表。
command-palette.tsx: 复用当前范围导航，保留项目切换和焦点，不抢占终端输入快捷键。
projects-workspace.tsx: 基于授权快照呈现项目焦点、搜索、排序、生命周期筛选和双视图。
console.tsx: 按工作空间与项目隔离状态，组合顶栏、交付筛选工具层、节点池、设备终端路由、权限骨架与星标。
dashboard.tsx: 读取完整窗口指标和近期活动，范围切换使用共享下拉，后台刷新保留内容。
dashboard-charts.tsx: 按需加载 Recharts，以等比例圆角渐变柱和柔和曲线呈现构建结果与成功周转，保留断点和孤立样本。
emissary-ota.tsx: 确认后发起固定产品版本升级，提供历史空态和键盘滚动，保留失败重试、取消、并发和暂停。
node-installation-settings.tsx: 按系统管理员权限配置全局设备公开地址，首读可重试。
node-enrollment-dialog.tsx: 固定产品统一版本，展示平台安装命令、单用凭证与过期状态。
forms.tsx: 提供项目、节点、凭据表单，复用下拉并统一提交通知。
lifecycle.tsx: 分别等待影响与当前操作，使用 HTTP 幂等键提交归档、恢复与删除。
node-card.tsx: 直接消费列表快照，以 Standard 卡片呈现身份、硬件、槽位和等比例容量。
node-detail-dialog.tsx: 组合固定身份标题、五个详情标签和遥测生命周期，保留独立重命名。
node-overview.tsx: 先展示四项设备摘要，再展示接口与标签，按需展开完整硬件和连接信息。
node-resources.tsx: 展示当前资源采样，按需加载分钟趋势，按设备 ID 查找图形设备型号。
node-resource-trends.tsx: 读取分钟历史，以圆端渐变曲线呈现 CPU 和已用内存，保留缺失断点。
node-tools.tsx: 以单列工具行展示版本和可用状态，路径按需展开，签名配置独立分组。
node-operations.tsx: 组合链路观测、有限操作、终端、升级与授权吊销。
finite-node-operations.tsx: 按当前管理员权限发起有限探测，保留幂等请求、取消和分页结果。
node-telemetry-status.tsx: 从心跳缓存解释队列进展、持久清单冲突、永久拒收、采样裁剪、磁盘占用与未知值。
emissary-update-progress.tsx: 按实际启动器能力显示真实字节、阶段和排空计数。
node-network-addresses.tsx: 在设备摘要后按接口对齐 IPv4/IPv6、采样时间和截断状态。
node-rename.tsx: 提供具名图标入口，只提交名称，按 Unicode White_Space 裁剪后校验 UTF8 与控制字符，保护草稿和焦点。
node-detail-parts.tsx: 提供分区、折叠区、对齐字段、时间与重试，零值可见且轮询保留展开状态。
node-metadata.tsx: 按需读取元数据，直接展示普通值，复杂值与快照信息折叠，保留刷新快照。
nodes-workspace.tsx: 按节点池精确筛选并保留查询地址，组合安装弹窗，关闭不再匹配的详情。
node-pools-workspace.tsx: 以 Standard 卡片呈现设备总数与在线计数，保留池操作和设备入口。
notifications.tsx: 顶部居中显示通知，避开手机顶栏与抽屉导航，桥接校验和弹窗焦点。
preferences-controls.tsx: 为登录页和个人设置提供自定义语言、主题下拉。
prism-interactions.tsx: 连接导航、设备与通知标签的弹簧背板和按压，跟随功能玻璃指针并响应辅助偏好。
prism-capsule.ts: 保留速度的弹簧装饰层，连续改变位置与尺寸，收敛后停止调度。
providers.tsx: 组合账户、语言、主题、顶部进度和全局通知，动效适配系统偏好。
release-detail.tsx: 组合版本说明、构建历史与单构建交付；发布弹窗固定候选，候选失效须重选；各状态可编辑说明，已发布镜像分发使用固定版本标签。
release-form.tsx: 创建版本并编辑多行说明，版本创建后只读；源码与目标归属构建引导。
release-table.tsx: 以可键盘滚动的表格展示版本和构建次数，按需展开完整版本说明。
settings.tsx: 配置令牌、源码凭据和审计，展示权限与有效状态，按需展开完整审计身份，保留首读重试。
signing-settings.tsx: 等待签名预览和必需凭据后开放配置，按项目身份保留默认方案草稿。
space-switcher.tsx: 切换工作空间和项目，按视口约束菜单并保留键盘导航。
tenant-chooser.tsx: 会话确认后展示 Prism 工作空间目录，保留骨架、重试、幂等创建与邀请；进入后显示项目。
top-progress.tsx: 根据并发前台计数延迟显示顶部细进度条，最后请求结束后淡出。
tenant-members.tsx: 分开呈现成员与邀请表单、列表和空态，首读显示骨架并提供失败重试。
workspace-management.tsx: 按权限渲染具名设置分区，统一阅读宽度，必需资源就绪后挂载表单并隔离分区状态。
workspace-navigation.tsx: 渲染含节点池的作用域侧栏和设备终端面包屑，权限首读骨架与失败重试。
ui/: 提供统一基础控件。

header-tools.tsx: 工作区、租户大厅与通知页按文档、通知铃铛、账户插槽排列顶栏工具。
image-builds.tsx: 只读展示镜像候选与增量平台日志，后台轮询和终态补读静默，正式分发从 Release 发起。
image-catalog.tsx: 按项目查询正式镜像、所属版本与来源构建，提供键盘滚动和摘要拉取引用复制。
image-credentials.tsx: 分区配置镜像凭据和 BuildKit Secret，展示空态、公开身份与完整引用，保留首读重试和秘密清空。

notification-provider.tsx: 在账户边界维护单 SSE 连接，合并失效并静默补读摘要。
notification-center.tsx: 顶栏铃铛与独立页面共用抽屉和未读数，后台重验保留原阅读窗口及滚动。
notification-settings.tsx: 配置站内偏好、邮箱订阅及项目名称旁的关注星标，保存失败保留既有状态。
notification-webhooks.tsx: 在管理员设置配置端点、一次密钥、测试与投递重试。

terminal-workspace.tsx: 提供独立设备终端、显式创建结束、权限状态和粘贴确认。
terminal-view.tsx: 按需加载 xterm，清除屏幕及播报缓存，反馈字节消费并隔离终端键盘。

[PROTOCOL]: Update this header on change, then check AGENTS.md
