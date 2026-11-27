<!--
[INPUT]: 最终production构建、1242项夹具回归和必要复验。
[OUTPUT]: 记录失败原因、修正、复验结果与环境边界。
[POS]: 支撑FINAL-VALIDATION，不替代真实服务验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Production UI 最终回归

状态：完成。完整套件1242项与必要复验已结束，所有发现均已解决。停止增加检查范围。

## 完整套件结果

同一production服务 `http://127.0.0.1:3100`，确定性HTTP API夹具。

| 项目 | 通过 | 跳过 | 失败 | 总项 |
| --- | ---: | ---: | ---: | ---: |
| desktop | 411 | 2 | 1 | 414 |
| tablet | 409 | 4 | 1 | 414 |
| mobile | 408 | 2 | 4 | 414 |
| 合计 | 1228 | 8 | 6 | 1242 |

耗时10.5分钟。完整命令在web目录执行：

```sh
rtk proxy sh -c 'ARTIFACTORY_UI_PRODUCTION=1 ARTIFACTORY_UI_OUTPUT=/tmp/prism-sync/production-final-parallel-results ARTIFACTORY_UI_REPORT=/tmp/prism-sync/production-final-parallel-html npx playwright test --config playwright.ui.config.ts --project desktop --project mobile --project tablet --workers=3 > /tmp/prism-sync/production-final-parallel.log 2>&1'
```

日志：`/tmp/prism-sync/production-final-parallel.log`
附件：`/tmp/prism-sync/production-final-parallel-results`
HTML：`/tmp/prism-sync/production-final-parallel-html`

单worker首轮至309项后按root指示中止，随后仅重启这一次3worker完整运行；中止轮不计最终统计。完整运行期间未重启服务。

## 失败诊断及必要复验

1. 设备长硬件用例，三个视口失败。原严格scrollWidth<=clientWidth未考虑玻璃边框的1px亚像素取整。面板clientWidth759/scrollWidth760/offsetWidth760，所有子元素无>1px溢出。仅test允许1px取整并补齐自定义设备telemetry空夹具。必要复验desktop/tablet/mobile：**3/3通过**，2.9秒。
2. 手机登录通知用例失败。表单视口y从438.3125变411.3125，scrollY从0变27；宽高与document坐标不变。浏览器点击时滚入按钮造成视口坐标变化。仅test改用document坐标严格比较。必要复验desktop/tablet/mobile：**3/3通过**，2.3秒。
3. 手机顶栏文档按钮明暗2项失败。项目路由`.header-docs`为44×36px；大厅与通知页均44×44px。`prism-workspace.css:27`更强选择器将手机min-height覆盖为36px。按root最终授权，在<=767px断点将文档与快捷指令工具恢复min-height:44px。production重新构建并重启3100后，mobile明暗2项复验通过；每项覆盖390/320px×大厅/项目/通知页。

复验没有重跑完整套件。原6个失败全部解决。按完整套件与最终复验合并计算：1234通过、8条件跳过、0未解决；没有宣称重新全量跑出该结果。

设备复验：`/tmp/prism-sync/production-node-final-results`、`/tmp/prism-sync/production-node-final-html`。
登录复验：`/tmp/prism-sync/production-notification-final-results`、`/tmp/prism-sync/production-notification-final-html`。
诊断证据：`/tmp/prism-sync/production-final-failures.md`、`/tmp/prism-sync/production-failure-diagnostic.jsonl`。

## 跳过说明

- PowerShell bootstrap：3项，本机无PowerShell。
- desktop导航抽屉：1项，仅移动布局适用。
- tablet导航最终截图：明暗2项，测试显式限桌面和320px。
- tablet/mobile玻璃导航反光：2项，测试显式限桌面视口。

## 最终构建与导航复验

- `npm run typecheck`通过。日志：`/tmp/prism-sync/production-final-typecheck.log`。
- `npm run build`通过，包含root当前en.json工作空间术语修正。日志：`/tmp/prism-sync/production-final-build.log`。
- 原3100服务停止后，使用最终production构建重启。当前start session：41509。
- 导航初次必要复验：light通过；dark在大厅因账户首读填入前读取了bell坐标、填入后读取account坐标而失败。保留产品行为，仅将account可见等待移到全部几何测量前。随后dark/light **2/2通过**，4.9秒。
- 最终导航输出：`/tmp/prism-sync/production-navigation-final-settled-results`，HTML：`/tmp/prism-sync/production-navigation-final-settled-html`。

## 改动与边界

修改`web/src/styles/prism-workspace.css`的手机44px工具高度；同步styles L3/L2。修改`node-identification.spec.ts`、`notifications.spec.ts`、`navigation-actions.spec.ts`的测试假设、夹具和tests L3/L2。`git diff --check`通过。未修改其他production源码。

未提交、推送或部署。测试使用fixture HTTP API，不能证明真实后端、硬件或线上权限。完整套件及必要复验完成后停止增加检查范围。
