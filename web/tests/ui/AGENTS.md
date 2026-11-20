# web/tests/ui/
> L2 | Parent: ../AGENTS.md

ai-builds.spec.ts: 验证应用与宿主分类、规划、澄清和重开终态补读。
accessibility.spec.ts: 验证租户卡片、业务页面与覆盖面板的 WCAG 自动检查。
browser-audit.spec.ts: 验证侧栏设置草稿保护与退出重试。
build-source-edge.spec.ts: 验证三视口源码建议等宽对齐、搜索、键盘与光标、默认值、失败重试和迟到响应隔离。
build-history.spec.ts: 验证终态静态日志、分页锚点、尾部事件与发布状态轮询。
build-wizard.spec.ts: 验证严格 SemVer、Release 创建与修订、继承构建、选定交付、失败重试和权限。
console-visual.spec.ts: 验证多语言工作台、侧栏设置与个人设置布局。
emissary-ota.spec.ts: 验证节点升级配置。
errors.spec.ts: 验证错误状态和本地化。
dropdown-controls.spec.ts: 验证明暗主题与普通表单弹层齐宽对齐、焦点、必填空值、迟到默认值、字段事件、重置及长列表边界。
select-helpers.ts: 通过可见触发器和选项执行共享下拉选择。
fixtures.ts: 提供显式应用分类、租户数据、默认签名预览、HTTP 夹具及个人设置、侧栏入口。
interaction-visual.spec.ts: 验证切换器键盘访问、320px 长名称顶栏、短视口侧栏与个人设置。
lifecycle-controls.spec.ts: 验证 HTTP 操作入口、幂等键、隐藏操作历史、范围恢复与终态停止轮询。
loading-progress.spec.ts: 验证前台并发结束、失败释放、租户解析取消、轮询静默与未保存导航保护。
loading-motion.spec.ts: 采样骨架与进度条动画帧，验证系统减少动态效果、快速请求防闪及 SSE 静默。
images.spec.ts: 验证已发布 Release 分发、候选只读、凭据清空，以及慢轮询与终态日志补读保留输出。
monochrome-workspace.spec.ts: 验证中性色工作台、项目切换和导航状态。
motion.spec.ts: 验证默认动效、系统减少动态适配、历史开关忽略及偏好界面无动效控制。
node-details.spec.ts: 验证节点详情布局、分区查询和管理操作。
notifications.spec.ts: 验证浮层布局、顶栏访问、字段校验、弹窗焦点、计时暂停、来源隔离与轮询恢复。
node-pagination.spec.ts: 验证节点分页和筛选。
peripheral-visual.spec.ts: 验证认证页、邀请页和错误页的视觉边界。
preferences.spec.ts: 验证用户视觉和语言偏好。
readable-routes.spec.ts: 验证 slug 导航和作用域切换。
registration.spec.ts: 验证注册交互。
signing.spec.ts: 验证设置导航、导入清空秘密、异步默认选择、不可用方案和验签状态。
skeleton-loading.spec.ts: 验证节点首次读取骨架、失败退出加载和历史日志分页保留输出。
tenant-chooser.spec.ts: 验证租户卡片、HTTP 创建入口、幂等重试、搜索和读取失败恢复。
tenancy.spec.ts: 验证租户与项目权限界面。
workspace-hierarchy.spec.ts: 验证作用域侧栏、顶栏租户切换、设置别名、权限边界、草稿保护与生命周期。

[PROTOCOL]: Update this header on change, then check AGENTS.md
