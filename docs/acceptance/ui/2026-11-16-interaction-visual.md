# 交互与视觉复查 · 2026-11-16

本轮延续全站重构后的浏览器巡检，重点检查键盘操作、手机触控区域、短视口和长名称。先在既有生产预览中复现，再在独立输出目录的生产构建中复查；没有更改 API、权限、数据库或实体设备配置。

## 已复现并修复

| 问题 | 修复与验证 |
| --- | --- |
| 空间切换器从搜索框按向上键进入倒数第二项 | 正确进入末项；覆盖租户、项目、无匹配项、首尾循环和 Escape 焦点恢复。搜索框保留 Home / End 文本光标操作 |
| 手机端返回入口、短标签和账号入口过小 | 返回链接由 16px 高增加至 44px，三个详情标签由 24px 宽增加至 44px，账号按钮由 36px 宽增加至 44px；实际测量至少 44×44px |
| 横屏和短窗口的导航底部不可达 | 顶部空间切换保持可见，主导航和底部入口在独立区域滚动；验证滚动到文档、键盘操作和关闭抽屉后恢复焦点 |
| 项目较多时切换菜单超出短视口 | 限制菜单高度，只滚动结果列表；包含 13 个项目，末项聚焦后完整显示 |
| 个人设置面板的最小高度高于短视口 | 按视口约束面板高度，手机使用明确的三行布局，表单内部滚动；在 844×390 和 390×320 验证关闭与安全表单提交入口完整可达 |
| 快捷面板中的长项目名挤压操作文字 | 限制次要项目名宽度并省略溢出，主要操作名称保留空间；验证中英混合长名称、点击跳转和没有额外写请求 |
| 中文项目名超出后端长度上限时只有通用错误 | 按现有 API 的 UTF-8 字节上限在提交前校验，显示中英文具体提示、聚焦名称字段并保留草稿；验证 102 字节被本地拒绝，100 字节仍可提交 |

新增 `web/tests/ui/interaction-visual.spec.ts` 五项行为回归，在三个视口运行共 15 项。其中 12 项先在既有生产预览失败，多项目菜单的 3 项在键盘修复后单独复现失败；修复后全部通过。触控区域调整采用页面实际测量和截图复查。

线上继续检查时补充发现了名称长度反馈问题，增加中英文共 6 项回归，先在旧生产预览失败，修复后通过。该补充沿用后端现有的 100 字节上限，没有改变 API 或数据库约束。

## 本地验证结果

| 检查 | 结果 |
| --- | --- |
| TypeScript 与独立生产构建 | 通过，独立输出目录 `.next-interaction-qa` |
| 最终完整 UI 回归 | 305 通过、1 按条件跳过，无失败或不稳定用例；包含本轮 21 项、axe WCAG AA、减少动态效果、草稿与 SSE 回归 |
| 名称反馈补充回归 | 6 通过，覆盖中英文及三个视口；补充修复后的 TypeScript 与独立生产构建通过 |
| 文档浏览器 | 14 通过；目录、搜索、主题、Mermaid、源码链接及匿名访问 |
| 隔离真实后端浏览器 | 14 通过；注册与授权、原生日志、下载、发布、取消和构建引导 |
| 真实后端视觉与交互组合 | 12 组通过：桌面 / 平板 / 手机 × 中 / 英 × 浅 / 深；页面异常为 0，无整页横向溢出 |
| 源码差异检查 | `git diff --check` 通过 |

## 截图及证据

截图与回归输出保存在工作区的忽略目录 `.local/interaction-visual-20261116/`。修复前后截图聚焦受影响区域，保持原有 Inter 字体、黑白灰配色及无装饰背景。

| 内容 | 文件 |
| --- | --- |
| 手机构建详情 | `before-artifacts-mobile.png`、`after-build-controls-mobile.png` |
| 横屏导航滚动 | `before-sidebar-landscape.png`、`after-sidebar-landscape.png` |
| 横屏个人设置 | `before-account-landscape.png`、`after-account-landscape.png` |
| 长项目名快捷面板 | `before-command-long-mobile.png`、`after-command-long-mobile.png` |
| 修复前失败证据 | `red-results/`、`menu-red-results/` |
| 修复后行为证据 | `green-results/` |

隔离真实后端的既有构建仍能回放 SSE 日志，并能切换执行、计划、制品标签。账号菜单 Tab 离开后正确恢复到菜单触发器，无需改动。表格自己的横向滚动属于预期行为，未作为整页溢出处理。

本轮使用 Chromium；手机和平板为视口模拟。实体触摸、Safari / Firefox 和屏幕阅读器人工验收不计入自动测试结果。

## Kubernetes 发布与复查

08:03:32–08:03:39 CST，将 `artifactory` release 从 revision 8 更新至 **revision 9**，状态 **deployed**。保留入口 `http://127.0.0.1:38100`，只替换 Web 镜像：

```text
ghcr.nju.edu.cn/zhubby/artifactory-web:uxiv-20261116-30c3627bd9c4@sha256:21f1d609d7c6c368153e549aa3464794ee132f59d2d91007023f72f9caa5b8a6
```

镜像来自 194 个文件的独立快照，基准 HEAD 为 `34be99b58ee73b4f149a6cc9f823d2d94d8f2b9f`，包含本地未提交的重构与修复。源码 manifest SHA256 为 `30c3627bd9c4cb14217fdc8e0da50dca3b0b62d5d75c0bbe86725032ec91e016`；发布前核对全部文件摘要。未执行源码提交、推送或合并。

实际 linux/amd64 standalone 镜像通过 21 项交互和 14 项文档检查，五个真实节点预拉取并校验摘要、文档字体和部署 API 转发。Helm lint 与初始化脚本 7 项测试通过；22 个渲染资源只有 Web 镜像变化。为适配既有单 Web 配额，替换期间临时调整滚动策略，完成后恢复原 `25% / 25%`。

原入口上的 21 项前端回归全部通过，这一组明确使用模拟 API。另通过真实网页登录，创建独立临时租户和长名称项目，实际检查快捷面板、44px 账号入口、横屏导航滚动、项目切换键盘操作、两个短视口中的个人设置与焦点恢复。中文名称超限时没有发出创建请求；改成恰好 100 字节后真实后端接受创建。页面异常为 **0**。

10 个业务 Pod 全部就绪、容器零重启；Web Pod 为 `artifactory-web-6c686fb7c5-7bcss`，实际 image ID 与固定摘要一致。三个 Control 副本及全部后端 Pod UID 保持不变，既有数据库、PVC、凭据、Gateway 和配额不变；158 项选定非目标资源的身份和规格未变。

持续约 156 秒完成 6 次稳定性采样，Pod 和接口持续可用。两台原有在线设备及一个 Worker 的心跳持续推进；一台 Windows 设备保持原有离线状态，身份、证书和吊销状态不变。临时租户及两个项目经网页生命周期操作删除，操作状态 `succeeded`；原有两个租户及项目列表保持基线。未向生产设备提交构建或 OTA。

临时容器和预拉取 Pod 已清理；两个专用线上验证会话均已退出，旧 Cookie 返回 401，私有会话文件和临时仓库授权目录已删除。保留原端口转发；隔离后端预览 `http://127.0.0.1:32161` 已切换至最终生产构建。

| 交付证据 | 工作区位置 |
| --- | --- |
| 源码、镜像、渲染与发布 | `.local/k8s-interaction-visual-20261116/` 下的 `source-identity.json`、`published-image.json`、`render-verification.json`、`upgrade-result.json` |
| 最终全站回归 | `.local/interaction-visual-20261116/final-ui.json`、`final-ui.log` |
| 镜像及线上前端回归 | 交付目录 `ui-image.json`、`docs-image.log`、`ui-live.json` |
| 真实线上页面与清理 | 交付目录 `browser-verification.json`、`browser-cleanup-verification.json`、`browser-session-cleanup.json` |
| 运行与稳定性 | 交付目录 `runtime-verification.json`、`stability-verification.json`、`final-verification.json` |
| 线上截图 | 交付目录 `live-command-long-mobile.png`、`live-sidebar-landscape.png`、`live-account-844x390.png`、`live-account-390x320.png`、`live-name-validation-mobile.png` |

回滚基线为 revision 8，完整 values 和原 Web 固定摘要保存在交付目录；本次未执行回滚，没有数据库变更。最终回归数字及本节在构建后补录，镜像内本页保留构建时的本地验证记录。
