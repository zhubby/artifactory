<!--
[INPUT]: 最终原型矩阵、生产回归、构建日志与范围复核结果。
[OUTPUT]: 记录本轮双向同步的实际验收证据与连接边界。
[POS]: 同步矩阵的验收入口；不替代真实服务与硬件测试。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Prism 双向同步验收

验收日期：2026-11-27。
界面统一使用「工作空间」。
层级为账户 → 工作空间 → 项目 → 版本 → 构建。
工作空间选择、创建和切换均进入项目目录。

## 实现与原型

- 两侧采用 Prism 明暗语义色、字体和控件基线。
- 桌面侧栏宽236px，外边距16px，顶栏高64px。
- Standard 承载内容。Regular 承载导航与工具。
- 玻璃保留10px模糊、210%饱和、30px圆角与色散亮边。
- 动效响应操作并可中断，空闲时无装饰循环。
- 登录、注册、工作空间和项目遵循现有字段合同。
- 项目构建、制品和版本返回链均保留项目范围。
- 通知使用账户范围。设置使用唯一分组侧栏。
- 发布保留单次完整构建、冻结候选和同一配置重试。
- 图表保留等圆角柱、渐变、保形曲线与缺失断点。
- 120项组件展厅独立保留，本轮只校正空间术语。

## 浏览器结果

原型使用本地静态服务。检查浏览器为Chromium。

| 检查 | 最终结果 | 证据 |
| --- | --- | --- |
| 24条页面路径 × 4种视口 × 明暗 | 192/192 | [页面矩阵](prototype-matrix.json) |
| 所选页面的WCAG A/AA扫描 | 40组，0项违规 | [页面矩阵](prototype-matrix.json) |
| 玻璃、辅助偏好、图表和720px重排 | 48/48 | [偏好检查](prototype-preferences.json) |
| 空空间与授权项目的数据边界 | 3/3 | [范围检查](prototype-data-boundaries.json) |
| 概览筛选与数据披露样式隔离 | 12/12 | [筛选检查](dashboard-filter-results.json) |
| 命令、设置、账户外壳与大厅 | 174/174 | [框架检查](shell-results.json) |
| 框架几何与节点池深链 | 116/116 | [几何检查](shell-geometry-results.json) |
| 项目构建、制品与返回链 | 137/137 | [交付范围检查](scoped-list-results.json) |
| 账户与管理 | 104/104 | [账户检查](account-results.json) |
| 交付主流程 | 72/72 | [流程检查](flow-results.json) |
| 独立目标、日志、审计与分发边界 | 32/32 | [流程边界](flow-completion-results.json) |
| 生命周期影响列表 | 8/8 | [布局检查](lifecycle-layout-results.json) |
| 设备侧面板 | 4组通过 | [设备检查](device-results.json) |
| 独立版本创建与草稿保护 | 4组通过 | [版本检查](release-results.json) |
| 认证主题与密码草稿 | 4组通过 | [认证检查](auth-theme-results.json) |
| 通知工作空间与失权清理 | 4组通过 | [通知检查](notification-scope-results.json) |

页面矩阵的视口为1440×1000、375×812、768×1024和812×375。
192组均无文档横溢、脚本错误、空标题或旧空间术语。
辅助偏好分别检查减少动态、降低透明、增强对比和强制颜色。

## 实际控制台

使用production构建和确定性HTTP API夹具。
桌面、平板、手机完整套件共1242项。
首轮1228通过、8条件跳过、6失败。
六项发现修正后，对应用例复验全部通过。
没有把合并结果写成再次完整运行的结果。

- 三项设备检查修正1px边框取整假设，保留子元素溢出检查。
- 一项登录检查改为文档坐标，避免点击自动滚动干扰测量。
- 两项手机工具检查修复真实36px高度，恢复44px触控目标。
- 顶栏几何读取等待账户首读完成，避免混用两次布局坐标。
- 八项跳过来自PowerShell缺失或限定布局的测试条件。

最终类型检查和生产构建通过。
详见[生产验收报告](PRODUCTION-VALIDATION.md)、
[类型日志](production-typecheck.log)和[构建日志](production-build.log)。

原型全部JS通过ES Module语法检查。
设计变量JSON、文件契约、模块地图、本地链接与差异空白检查通过。
本轮源码文件均不超过800行。

## 视觉复核

| 原型 | 实际控制台 |
| --- | --- |
| [浅色项目目录](prototype-desktop-light-projects.png) | [浅色项目目录](production-desktop-light-projects.png) |
| [深色概览](prototype-desktop-dark-overview.png) | [深色设备目录](production-desktop-dark-devices.png) |
| [手机工作空间](prototype-mobile-light-workspaces.png) | [手机登录](production-mobile-light-login.png) |
| [项目构建](prototype-desktop-light-builds.png) | [平板工作空间](production-tablet-light-hall.png) |
| [手机项目制品](prototype-mobile-dark-artifacts.png) | [手机设备面板](production-mobile-dark-device-drawer.png) |

## 验收边界

原型全部数据、材料和操作均为本地虚构样例。
控制台测试使用夹具，不访问真实生产账户或设备。
API、SSE、模型、外仓、邮件、发布租约与硬件仍由真实服务执行。
原型用显式本地状态呈现这些合同，不宣称链路执行成功。
Safari、Firefox、实体触屏与屏幕阅读器尚未验收。
原型正文以中文为主，帮助链接指向设计交付文档。
这些连接和内容边界在[同步矩阵](../../SYNCHRONIZATION.md)单列。

本轮没有提交、推送或部署。
