<!--
[INPUT]: 固定 main、CI Web 镜像和三视口线上验收回执。
[OUTPUT]: 记录四步构建引导的 Web 部署、身份保留与验证边界。
[POS]: Release 内构建引导的正式上线记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 四步构建引导 Kubernetes 上线记录

日期：2026-11-21，Asia/Shanghai。
实现已合并 main、推送并部署。

## 交互范围

Release 内的“发起构建”使用四步弹窗：

1. 选择源码：搜索分支、Tag 或手动输入引用。
2. 说明构建要求：选择常用模板或填写自定义要求。
3. 选择交付目标：使用类型与平台卡片，按需展开高级设置。
4. 确认发起：核对摘要，再提交一次构建创建请求。

构建支持多个原生目标、镜像目标及两者混合。
精细配置保留宿主、架构、应用平台和交付命名字段。
确认步提供可折叠的构建参数。
从摘要修改后返回确认，并保留其他回答。
步骤导航和底部操作固定，正文独立滚动。

最终确认前不创建构建。
提交期间禁止重复提交、编辑和关闭。
提交失败保留草稿供重试。
Release 变为不可构建时保留回答并禁用最终提交。
关闭已修改的弹窗须确认放弃草稿。

Release 仍只接收 SemVer 与版本说明。
构建输入和现有后端接口保持原合同。

## 固定源码与镜像

| 身份 | 固定值 |
| --- | --- |
| 实现提交 | `cd5193eb4c71a133b123feb19d9eaa7608b88290` |
| 版本 | `0.2.0` |
| 源码归档 SHA-256 | `380fca4e62f5e05a300110b6795928ee6c1d9c42815951172390dedb07cb02ff` |
| Web 标签 | `git-cd5193eb4c71a133` |
| Web 摘要 | `sha256:c6969daa643245ea87ab625b7bbb2c0c26b6000e77fa331680975e6fd1a53a65` |

镜像来自 [GitHub Actions 44868958352](https://github.com/zhubby/artifactory/actions/runs/44868958352)。
Web 任务成功，使用纯净 main 的 linux/amd64 镜像。
源仓库为 `ghcr.io/zhubby/artifactory-web`。
部署仓库为 `registry.kubeease.cn/pre-release/artifactory-web`。
复制保留摘要，匿名读回与源镜像一致。
源码、版本和平台标签均已核对。

同次工作流有九项镜像任务成功。
Planning Worker 在既有镜像探测步骤失败。
整次工作流状态为 failure，Web 镜像任务为 success。
本轮只部署 Web，其他组件保留原镜像。

## Kubernetes 结果

命名空间与 Helm Release 均为 `artifactory`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
入口为 [Artifactory 控制台](http://10.169.1.239:30820)。
Helm revision 从 12 升至 13，状态为 deployed。

升级前的 Chart 渲染与现场 manifest 一致。
预览与最终 manifest 只改变 Web 的容器镜像字段。
九项 Deployment 全部就绪。
运行中的 Web 摘要与镜像回执一致，重启次数为零。
全部 Deployment UID 保留。
其他八项 Deployment 的 spec 保持一致。
应用 Secret 的 UID 与内容摘要、PVC UID 均保留。
Helm 历史只增加 revision 13，没有淘汰既有记录。
业务数据库仍使用上轮部署的 `artifactory_release_20261121`。

## 验证结果与边界

| 验证 | 结果 |
| --- | --- |
| TypeScript 类型检查 | 通过 |
| Web webpack 生产编译 | 通过 |
| 生产模式浏览器回归 | 195 项通过，桌面、平板和手机各 65 项 |
| 四步引导与边界回归 | 包含确认前无写入、返回编辑、混合目标、失败重试、草稿与轮询状态 |
| 中英文与明暗主题 | 通过；英文两种主题的四步通过自动 WCAG AA 检查 |
| 共享弹窗与下拉回归 | 通知、焦点、嵌套 Escape、参数与平台约束通过 |
| 文档构建与链接检查 | 通过 |
| 真实健康、登录与文档 | HTTP 200 |
| 真实页面 1440、820、390px | 四步、混合目标、摘要修改与焦点恢复通过 |
| 真实页面横向溢出与浏览器错误 | 均为零 |
| 真实构建创建请求 | 零次；意外 POST 设有拦截围栏 |
| 临时项目与验收会话 | 项目精确删除，原项目列表恢复，会话已撤销 |

本地生产编译使用共享依赖软链。
standalone 跟踪出现软链路径警告。
该本地产物不用于部署，部署镜像由 CI 的纯净检出生成。

线上验收只为源码选项提供固定夹具。
版本创建、说明修改、业务读取和项目清理使用真实接口。
最终确认前没有创建 Build，也没有执行源码准备或正式交付。
确认后的请求与失败重试由隔离 UI 夹具回归验证。

私有回执和截图位于 `.deploy/guided-build-20261121/`。
本记录在镜像生成后编写，不包含在上述镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
