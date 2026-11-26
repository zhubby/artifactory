# web/tests/ui/
> L2 | Parent: ../AGENTS.md

ai-builds.spec.ts: 验证应用与宿主分类、规划、具名消息、澄清和切回审计标签的终态补读。
accessibility.spec.ts: 验证租户卡片、业务页面与覆盖面板的 WCAG 自动检查。
browser-audit.spec.ts: 验证侧栏设置草稿保护与退出重试。
build-source-edge.spec.ts: 验证构建弹窗三视口源码建议等宽对齐、搜索、键盘与光标、默认值、失败重试和迟到响应隔离。
build-history.spec.ts: 验证终态静态日志、分页锚点、尾部事件与发布状态轮询。
build-detail-design.spec.ts: 验证当前计划身份、历史选择轮询、执行归属、澄清草稿、日志标签生命周期与双语主题响应式无障碍。
build-execution-states.spec.ts: 验证无设备尝试的镜像执行、混合目标切换和无任务、无计划的终态说明。
build-process.spec.ts: 验证本代 ready、started 门槛，旧代隔离、源码失败和双语主题可读性。
build-guide.spec.ts: 验证四步弹窗、混合目标、草稿与失败重试、最终确认、中英文明暗主题和无障碍。
build-device-targets.spec.ts: 验证项目授权平台禁选、在线默认、高级约束和失败重试。
build-wizard.spec.ts: 验证严格 SemVer、Release 说明、独立源码与目标、轮询稳定的单构建交付、候选失效重选、失败重试和权限。
console-visual.spec.ts: 验证多语言概览、独立项目目录、侧栏设置与个人设置布局。
dashboard.spec.ts: 验证聚合、范围、请求隔离、等宽图表、空态、孤立样本及双语主题无障碍。
emissary-ota.spec.ts: 验证固定产品版本、手动升级提示、签名发行状态和历史权限。
emissary-manual-update.spec.ts: 验证管理员确认发起、条件围栏、状态变化、失败恢复和多视口无障碍。
errors.spec.ts: 验证错误状态和本地化。
dropdown-controls.spec.ts: 验证明暗主题与普通表单弹层齐宽对齐、焦点、必填空值、迟到默认值、字段事件、重置及长列表边界。
select-helpers.ts: 通过可见触发器和选项执行共享下拉选择。
filter-layout.spec.ts: 验证筛选同行、尺寸断点、长名称与滚动焦点边界。
fixtures.ts: 提供 Release、构建目标、节点池计数、节点安装、设备平台与概览 HTTP 夹具。
interaction-visual.spec.ts: 验证切换器键盘访问、顶栏通知焦点、320px长名称与短视口侧栏。
lifecycle-controls.spec.ts: 验证 HTTP 操作入口、幂等键、隐藏操作历史、范围恢复与终态停止轮询。
loading-progress.spec.ts: 验证前台并发结束、失败释放、租户解析取消、轮询静默与未保存导航保护。
loading-motion.spec.ts: 采样骨架与进度条动画帧，验证系统减少动态效果、快速请求防闪及 SSE 静默。
images.spec.ts: 验证已发布 Release 分发、候选只读、凭据清空，以及慢轮询与终态日志补读保留输出。
monochrome-workspace.spec.ts: 验证快捷导航、应用分类、草稿保护与日志状态。
model-compatibility.spec.ts: 验证任意模型名、兼容配置回显、协议切换与密钥留空保存。
planning-feedback.spec.ts: 验证实时与已确认统计、终态优先、旧审计身份和镜像确认说明。
planning-audit.spec.ts: 验证独立标签顺序与按需加载、工具关联、分页、切离停轮询和阅读状态。
planning-audit-data.spec.ts: 验证调用和提案身份、完整分片、原文保留与预算提示分类。
motion.spec.ts: 验证默认动效、系统减少动态适配、历史开关忽略及偏好界面无动效控制。
node-details.spec.ts: 验证节点详情布局、分区查询和管理操作。
terminal.spec.ts: 验证真实 xterm、字节 ACK、重连、撤权屏幕及播报清除、粘贴与 320px 焦点。
node-detail-design.spec.ts: 验证设备摘要、图形设备型号、分钟趋势断点、范围、重试和窄屏布局。
node-identification.spec.ts: 验证身份、重名短 ID、重命名权限、草稿、UTF8、Unicode 空白与接口快照。
notifications.spec.ts: 验证浮层布局、顶栏访问、字段校验、弹窗焦点、计时暂停及交互期间的来源隔离与轮询恢复。
node-pagination.spec.ts: 验证节点分页和筛选。
node-pools.spec.ts: 验证节点池导航、管理员增改删、失败草稿、设备筛选和双语窄屏无障碍。
node-installation.spec.ts: 验证固定同版本安装、过期、关闭秘密清理和系统地址权限。
node-installation-commands.spec.ts: 隔离执行 Shell 与 PowerShell，验证传参、摘要、进程策略与 CA 保留。
peripheral-visual.spec.ts: 验证认证页、邀请页、独立项目目录空态和错误页的视觉边界。
preferences.spec.ts: 验证用户视觉和语言偏好。
readable-routes.spec.ts: 验证项目目录、概览构建深链接、slug 导航和作用域切换。
registration.spec.ts: 验证注册交互。
signing.spec.ts: 验证设置导航、导入清空秘密、异步默认选择、不可用方案和验签状态。
skeleton-loading.spec.ts: 验证节点首次读取骨架、失败退出加载和历史日志分页保留输出。
tenant-chooser.spec.ts: 验证会话确认与登录跳转、租户卡片、创建幂等、搜索和读取失败重试。
tenancy.spec.ts: 验证租户隔离、独立项目目录权限、项目创建与资源授权。
workspace-hierarchy.spec.ts: 验证节点池、概览与项目目录侧栏、租户切换、设置别名、权限、草稿和生命周期。

navigation-actions.spec.ts: 验证正文动作、全局工具顺序、320px边界和星标失败恢复。
navigation-followup.spec.ts: 验证账户跨断点焦点和草稿、星标读取重试及明暗骨架截图。
navigation-readiness.spec.ts: 验证权限首读骨架、范围隔离、失败重试与已就绪导航保留。
settings-first-read.spec.ts: 验证设置首读依赖门控、失败重试、分区隔离和轮询草稿保留。

notification-center.spec.ts: 验证顶栏入口、嵌套抽屉焦点、账户独立请求、修订收据、水位已读与邮箱确认。

notification-followup.spec.ts: 验证顶栏抽屉、完整范围、实时阅读窗口、渠道偏好、邮箱返回与 WCAG AA。

[PROTOCOL]: Update this header on change, then check AGENTS.md
