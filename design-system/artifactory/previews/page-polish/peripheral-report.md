# Web 页面族审查

2026-11-28。代码基线 ca5d2c8，包含本轮共享工作区改动。审查地址 http://127.0.0.1:3100。所有业务请求使用 Playwright 隔离夹具，无真实业务写入。

## 已修复缺陷

邀请页已有登录账户时，AuthShell 未注册账户插槽。单实例账户退回 fixed top-right，覆盖右上品牌说明。

- `web/src/components/auth-shell.tsx` 复用 AccountSlot，把已有账户放入认证顶栏 flex 布局；未登录仍显示偏好控件。
- 更新文件 L3 和 `web/src/components/AGENTS.md`。
- `web/tests/ui/peripheral-visual.spec.ts` 增加邀请页账户必须位于顶栏、只存在一个实例、品牌说明和账户不重叠的断言。
- 更新 `web/tests/ui/AGENTS.md`。
- 修复截图：`1440-light/invitation-long-name.png`，另有 desktop/mobile、zh-CN/en、light/dark 八组合回归。

## 当前浏览器验证

临时矩阵 `review.spec.ts`：1440×1000、375×812，各 light/dark。**4 passed**，记录见 `test-results-final.json`。每组合 30 个页面状态，共 120 张截图。所有捕获均检查整页横向溢出≤1px，结尾无 pageerror。代表页面执行 WCAG 2 A/AA 和 2.1 AA Axe 检查，未发现自动规则违规。截图使用 reducedMotion reduce；不代表动态质感验收。

| 页面族 | 本次直接验证 | 既有回归入口 | 证据文件（各组合目录） |
| --- | --- | --- | --- |
| 登录/注册/邀请 | 两个认证入口、长工作空间邀请、过期邀请、认证顶栏账户 | peripheral-visual、registration、preferences | login、register、invitation-long-name、invitation-expired |
| 工作空间大厅 | 长名称、归档状态、搜索无结果 | tenant-chooser、tenancy | workspaces-long-name、workspaces-empty-search |
| 项目目录 | 普通列表、无项目、长项目与仓库名称 | prism-system、readable-routes、workspace-hierarchy | projects、projects-empty、project-long-name |
| 工作空间概览/图表 | 聚合指标、两张图、空数据、近期活动、两主题窄屏 | dashboard | workspace-overview、overview-empty |
| 节点池 | 长名称和大计数、无池 | node-pools | pools-long-name、pools-empty |
| 设备 | 目录、详情五个标签 | node-details、node-detail-design、node-identification、node-evolution | devices、device-tab-0…4 |
| 终端 | 具备 terminal-v1 的在线设备，正常未连接状态、Shell 选择和工具栏 | terminal | terminal-inactive |
| 通知 | 空收件箱、长标题与长摘要、通知偏好 | notification-center、notification-followup | notifications-empty、notifications-long-title、notification-preferences |
| 账户 | 偏好、个人信息、安全 | navigation-followup、preferences、browser-audit | account-preferences、account-个人信息、account-安全 |
| 验证/取消订阅 | 两种邮箱确认入口、取消订阅确认 | notification-followup | verify-confirm、email-verify-confirm、unsubscribe-confirm |
| 错误 | 独立404、过期邀请 | errors、peripheral-visual | not-found、invitation-expired |

## 本次验证命令结果

- `npx playwright test --config playwright.ui.config.ts peripheral-visual.spec.ts --project desktop --project mobile`：**8 passed**，覆盖 zh-CN/en×light/dark×desktop/mobile。输出 `peripheral-results/`，报告 `peripheral-report/`。
- `playwright test --config /tmp/artifactory-final-polish/page-review/playwright.config.ts`：**4 passed**。输出 `test-results-final/`。
- `npm run typecheck`：**通过**。

## 限制与待复核

- 该矩阵验证前端布局、空态和长文案；未操作真实登录注册、加入工作空间、邮件、设备或终端服务。
- 终端本次只验证正常未连接布局；真实 xterm 字节、重连、粘贴、撤权、320px 焦点仍由既有 terminal.spec.ts 覆盖，本次未重跑。
- 设备运维截图包含夹具默认未知遥测值；不代表真实设备上报验收。
- 120张截图为中文；认证边界另有8个中英文明暗回归。其他页面族英文行为主要引用既有测试，未在本次矩阵再次完整运行。
- 部分跨页矩阵的手机图表截图出现Y轴首字符视觉裁剪。单页复查截图显示完整刻度，DOM测量仅有不足1px的边界误差；尚未稳定复现来源。未扩大修改，保留 `chart-diagnostic.png`、`chart-diagnostic.json` 和原矩阵截图供后续复核。自动溢出及Axe检查无法识别此视觉差异。
- AI审计、设置、交付表和独立原型由其他任务负责，本报告不覆盖其验收。
- 无新增确定的发布阻断。本子任务未提交或部署。
