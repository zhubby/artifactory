<!--
[INPUT]: 固定源码、镜像摘要、Helm 回执与真实浏览器结果。
[OUTPUT]: 记录骨架加载、顶部进度条和租户大厅上线验收。
[POS]: 本轮 Web 部署记录，区分镜像身份与后续文档提交。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 加载反馈与租户大厅 Kubernetes 上线记录

日期：2026-11-20，Asia/Shanghai。
当前实例为 `artifactory`。
公开入口为 `http://10.169.1.239:30820`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
本轮只更新 Web 镜像。

## 源码与镜像

界面实现已提交并推送到 `main`：

```text
31ec79c6a6a3e0762c2c90868f04f83abbe8166a
```

GitHub Actions [Web 构建](https://github.com/zhubby/artifactory/actions/runs/44671169372)成功。
构建使用该提交的纯净检出。
OCI revision、version、source 标签均已核对。
GHCR 索引中的 Linux amd64 清单按原摘要复制到 Harbor。
Harbor 匿名读回的清单与配置核验通过。

| 身份 | 固定值 |
| --- | --- |
| GHCR 标签 | `ghcr.io/zhubby/artifactory-web:sha-31ec79c6a6a3e0762c2c90868f04f83abbe8166a` |
| GHCR 索引摘要 | `sha256:0d3a72f5c0b568916cd88bf267f3be12ca545d160c0955dfcb379208dbe267f3` |
| Harbor 标签 | `registry.kubeease.cn/pre-release/artifactory-web:git-31ec79c6a6a3e076` |
| 运行清单摘要 | `sha256:7a7c2930c1862b4c56bc9029aeb312c51753b0b85756dc13c868a003f169f25d` |
| Helm Release | `artifactory`，revision 7，deployed |

## 部署核验

升级前的 Helm revision 为 6。
当前 Chart 渲染与安装清单一致。
完整资源比较只允许 Web 容器镜像字段变化。
升级后运行清单与预览一致。
Web Pod 的 `imageID` 与运行清单摘要一致。
Web Pod 重启次数为零，九项 Deployment 全部就绪。
其他八项 Deployment 的 spec 与 UID 保留。
两个 PVC 与已有十七个 Secret 的 UID 保留。
本轮未初始化数据库、替换凭据或清理业务数据。
`/healthz`、`/login` 和 `/docs/` 均返回 HTTP 200。

## 页面验收

本机 Chrome `147.0.7727.15` 通过真实入口登录。
桌面 1440px 与手机宽度 390px 各测试深浅主题。
四组组合全部通过，浏览器和 HTTP 异常数为零。
手机结果为浏览器视口验证，不代表实体手机验收。

- 首次读取租户时显示卡片骨架和顶部 2px 进度条。
- 延迟真实 API 响应后验证加载状态，不替换业务数据。
- 响应完成后显示租户卡片并收起进度条。
- 后台轮询保持卡片，不显示骨架或进度条。
- 租户大厅桌面双列，手机单列，页面没有横向溢出。
- 搜索忽略大小写和首尾空格，无结果状态正确。
- 当前所有者账号显示管理入口并能进入设置。
- 卡片能进入租户工作台，归档与删除页没有操作进度面板。
- 四个独立验收会话全部退出，后续认证查询返回 401。

线上账号只有一个所有者租户。
其他角色的入口隔离由本地 UI 用例覆盖。
验收只发送业务读取请求，不创建或删除业务对象。
本轮未执行归档、删除、撤销或重试操作。

上线前的生产模式 UI 相关用例 66 项通过。
TypeScript 检查和 Next.js 生产构建通过。
推送后的 CI 还发现上一 Helm 重构留下的测试断言错误。
YAML 文档切分消耗末行换行，旧断言因此误报。
断言改为检查完整 selector 行后，脚本单测 58 项全部通过。
该修正不改变 Chart 或运行服务。

私有证据位于 `.deploy/loading-tenant-deploy-20261120/`。
其中包含部署回执、脱敏浏览器报告和十六张截图。
凭据、认证材料和原始配置不进入本记录。
本记录及测试断言修正晚于镜像构建。
它们不包含在上述固定镜像中。

[PROTOCOL]: Update this header on change, then check AGENTS.md
