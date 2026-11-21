<!--
[INPUT]: 固定 main、Web 镜像、Helm 与真实设备页面回执。
[OUTPUT]: 记录设备详情上线、资源身份与页面验证结果。
[POS]: 设备详情视觉改版的正式交付记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备详情 Kubernetes 上线记录

日期：2026-11-21，Asia/Shanghai。
设备详情改版已合并 main、推送并部署。

## 实现范围

保留右侧面板与五个标签。
标题和标签固定，正文独立滚动。
概览先展示型号、CPU、物理内存和任务槽位。
完整硬件、连接身份与原始数据默认折叠。
工具路径、复杂元数据和运行诊断按需展开。
标题状态紧随名称，重命名使用具名图标入口。
手机按钮保留至少 44px 触控区。
宽屏内存负载成对显示，手机逐行显示。
升级版本复用对齐字段，移除重复分隔线。
吊销成功后，焦点恢复到运维标签。
请求生命周期、权限和业务接口保持原有合同。
视觉规则见[设计规范](../design-system.md)。

## 固定源码与镜像

| 身份 | 固定值 |
| --- | --- |
| 实现提交 | `84e3c4f71c4e11f86446e6520632bfeca7466d5c` |
| 版本 | `0.2.0` |
| 源码归档 SHA-256 | `aafceed60a95283a61b01e2b681b78ed17d250d20758ee2636a7296c865608ff` |
| Web 镜像标签 | `git-84e3c4f71c4e11f8` |
| Web 运行摘要 | `sha256:a4b98f7b3788542f5d7e2daa1846077ad8505cb0a9cb484b640875db86c8c8c8` |

镜像仓库为 `registry.kubeease.cn/pre-release/artifactory-web`。
Web 来自 [GitHub Actions 44842467894](https://github.com/zhubby/artifactory/actions/runs/44842467894)。
构建使用固定 main 的纯净源码。
镜像标签中的源码、版本与平台均已核对。
linux/amd64 镜像保摘要复制至 Harbor。
Harbor 匿名读回的镜像摘要与源镜像一致。

## Kubernetes 结果

命名空间与 Helm Release 均为 `artifactory`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
沿用[设备入口](http://10.169.1.239:30820/tenants/ai-workspace/nodes)。
Helm revision 从 10 升至 11，状态为 deployed。

模板预览只改变 `Deployment/artifactory-web` 的主容器镜像。
Helm 只更新 `images.web.tag`，没有使用 post-renderer。
运行中的 Web 摘要与固定镜像一致，重启次数为零。
九项 Deployment 全部就绪。
其他 Deployment 的 spec 与 UID 保持一致。
应用 Secret 与 PVC 的 UID 保持一致。
最终 Helm values 和 manifest 与预览一致。
Helm 按默认十条历史上限淘汰 revision 1，保留 2 至 11。
第 1 版发布记录的清理不涉及应用 Secret。
`/healthz`、`/login`、`/docs/` 均返回 HTTP 200。

## 真实页面与回归

使用当前实例的 Mac 与 Windows 设备数据验收。
1440px 桌面、820px 平板和 390px 手机均覆盖明暗主题。
六组组合中的两台设备均通过五标签检查。
共保存六十张设备详情截图。

页面和抽屉没有横向溢出。
标签导航保持可见，Home 键恢复概览标签焦点。
重命名弹窗的输入焦点与 Escape 返回焦点正常。
详情关闭后，焦点恢复到原设备入口。
浏览器没有 console error 或 pageerror。
验收只发送业务读取，没有提交业务修改。
六个验收会话均已精确退出。
手机结果属于浏览器视口验证。
现场使用管理员账号；其他角色由夹具回归覆盖。

| 必要验证 | 结果 |
| --- | --- |
| 类型检查与生产构建 | 通过 |
| 设备详情、身份与 OTA 回归 | 99 项通过 |
| 三视口生产页面与吊销焦点 | 9 项通过 |
| 明暗主题 WCAG 自动检查 | 2 项通过 |
| 真实设备页面 | 六组组合通过 |
| 镜像与集群 | 摘要、预览、就绪状态和持久身份通过 |

私有回执与截图位于 `.deploy/device-detail-20261121/`。
本轮没有升级原生设备程序。
本记录在镜像生成后编写，不包含在上述镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
