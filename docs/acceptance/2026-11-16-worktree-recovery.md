# 工作树备份比对与源码恢复 · 2026-11-16

本次从迁移前的工作树备份恢复遗漏源码。
恢复基线为 `34be99b58ee73b4f149a6cc9f823d2d94d8f2b9f`。
备份标识为 `codex-worktree-recovery-20261116-110403`。

以下记录描述 11 月 16 日的隔离恢复准备。
该恢复当时未写回 Artifactory 主仓库。
11 月 17 日的恢复仅包含 Artifactory。
当前交付见[新版 UI 恢复与旧代码清理](ui/2026-11-17-restore-and-remove-legacy.md)。
本次保留原始备份，不执行跨项目清理脚本。

## 比对范围

| 仓库 | 原工作树 | 结论 |
| --- | --- | --- |
| Artifactory | control-ha、monochrome-console | 两个 HEAD 已在主分支；单色控制台有未提交内容 |
| uDeck | emulator-libretro-library、udeck-emulator-platforms | 两个 HEAD 已在主分支；没有未提交源码 |
| CyberOS | mcp-system-coverage、nvr-card-dialog、workspace-panel-latency | 三个 HEAD 已在当前历史；没有未提交源码 |

Artifactory 原有 53 项已跟踪变更及 24 个未跟踪文件。
恢复内容覆盖单色控制台、快捷导航和三步构建引导。
同时恢复设备侧面板、弹窗动效、公开文档主题及回归测试。
备份补丁与恢复后暂存补丁逐字节一致。
24 个新增文件核对原始字节后，补充文件契约。
本次另补充模块地图及本记录。

## uDeck 历史比对

旧 stash 的 24 个文件已有对应功能。
其中 15 个文件三方合并后与当前源码一致。
另外 9 个文件已推进翻译键、错误结构或 Qt 导入。
逐块保留现有实现后，合并结果与当前源码完全一致。
旧 stash 的新增中英文文案全部存在于当前词条值中。

以下三条引用由应用脚本写入仓库的归档命名空间。
原始对象和父提交保持不变。

| 原对象 | 内容 | 归档引用 |
| --- | --- | --- |
| `835d2d447277d5ea159ffc1d24ef75cefca8507d` | 内部任务记录 | `refs/archive/worktree-recovery-20261116/snapshot-4b9229` |
| `6c5811aa9b21d65127226fe1aea6a5e9fbc51dd8` | Debian 构建输出快照 | `refs/archive/worktree-recovery-20261116/snapshot-af14ff` |
| `b697a3934f8aaeacf2279d3c252566e70551647a` | 旧主分支 stash | `refs/archive/worktree-recovery-20261116/stash` |

构建输出和内部任务记录保留在历史对象中。
它们不作为新业务文件加入当前源码树。

## 本次验证

- TypeScript 检查通过。
- Web 与 mdBook 完整生产构建通过。
- 原始补丁及新增文件字节核对通过。
- 22 个模块地图的成员、父链接及文件契约核对通过。
- 写回脚本的 11 项隔离 Git 回归通过。
- 文档回归有 2 项通过，浏览器用例未完成。
- Chromium 启动被 macOS 沙箱拒绝 Mach 端口注册。
- UI 回归在重复出现同一启动错误后中止。

历史验收文件保存的是原执行日期的结果。
本次没有重新执行 Kubernetes 或实体设备验收。
应用脚本在本机补跑浏览器回归后，再写回恢复提交。

## 写回与清理

应用脚本先校验固定基线、提交包和备份摘要。
它保留 uDeck 和 CyberOS 的现有改动与暂存状态。
全部备份历史可从目标仓库引用到达后，才删除指定备份。
本地凭据、缓存和诊断日志不加入源码提交。
这些私有备份随本次授权的整目录清理删除。

外置盘写入受本次会话的只读权限限制。
恢复提交先在隔离副本生成。
实际写回、回归和清理由本机应用脚本记录回执。
