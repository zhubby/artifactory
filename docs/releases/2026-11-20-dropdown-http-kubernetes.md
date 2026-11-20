<!--
[INPUT]: 固定源码、镜像摘要、Helm 回执与 Chrome 结果。
[OUTPUT]: 记录自定义下拉、等宽对齐与 HTTP 操作修复的上线验收。
[POS]: Web 界面交付记录；保留生产数据与浏览器验证边界。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 自定义下拉与 HTTP 操作 Kubernetes 上线记录

日期：2026-11-20，Asia/Shanghai。
实例与命名空间均为 `artifactory`。
公开入口为 `http://10.169.1.239:30820`。
命名空间 UID 为 `5397584f-9b1b-4320-8d0a-7904f9687547`。
本轮两次升级均只更新 Web 镜像。

## 修复范围

- 全站表单选择与筛选复用自定义 Select。
- 可编辑源码建议使用自定义 Combobox。
- 偏好字段由完整按钮承担边框与点击区域。
- 弹层与完整输入框等宽，左右边缘对齐。
- 长选项换行，空间不足时弹层收缩。
- 普通 HTTP 使用 `getRandomValues` 生成 UUID v4。
- 创建租户与生命周期弹窗不再依赖 `randomUUID`。
- 失败重试复用幂等键，新操作生成新键。

隐藏原生字段只承担表单提交与校验。
界面不显示原生选择菜单或 `datalist`。
[设计规范](../design-system.md)已记录统一要求。

## 源码与镜像

| 升级 | 固定源码 | 结果 |
| --- | --- | --- |
| revision 7 → 8 | `2fa1029a9978f6b86507fe22f1e7f038ba9384c6` | 自定义下拉与 HTTP 操作修复 |
| revision 8 → 9 | `0e4017ce0fe224b4e941a59ed38b90d2f5876034` | 完整输入框等宽对齐 |

两个提交均已推送至 `main`。
最终镜像来自该提交的纯净检出。
GitHub Actions [Web 构建](https://github.com/zhubby/artifactory/actions/runs/44693246595)成功。
OCI revision、version、source 标签核验通过。
Linux amd64 清单保摘要复制至 Harbor。
Harbor 匿名读回摘要与配置核验通过。

| 身份 | 最终固定值 |
| --- | --- |
| GHCR 标签 | `ghcr.io/zhubby/artifactory-web:sha-0e4017ce0fe224b4e941a59ed38b90d2f5876034` |
| GHCR 索引摘要 | `sha256:837f1bb094dbacda9dc96b391bead40ee30cade663f59790f6518535f6ecdef3` |
| Harbor 标签 | `registry.kubeease.cn/pre-release/artifactory-web:git-0e4017ce0fe224b4` |
| 运行清单摘要 | `sha256:192ef430b5482d41e79f6ab153032212bb6e3bdebe8400df770a7cd02104cfa8` |
| Helm Release | `artifactory`，revision 9，deployed |

## 部署核验

Chart 与升级前安装清单一致。
完整资源预览只允许 Web 镜像字段变化。
最终运行清单与预览一致。
Web Pod 的 `imageID` 与固定摘要一致。
Web Pod 重启次数为零。
九项 Deployment 全部就绪。
其余 Deployment 的 spec 与 UID 保留。
现有 Secret 与 PVC 的 UID 保留。
本轮未初始化数据库或修改业务对象。
`/healthz`、`/login`、`/docs/` 均返回 HTTP 200。

## 页面与回归验证

本机 Chrome `158.0.8127.36` 通过真实入口登录。
桌面 1440px 与手机宽度 390px 各验证深浅主题。
四组组合全部通过，浏览器与 HTTP 异常数为零。
手机结果为浏览器视口验证。

- 真实 HTTP 环境确认 `randomUUID` 不可用。
- 创建租户按钮能打开表单并编辑草稿。
- 关闭时确认放弃草稿，租户数量保持不变。
- 归档与删除弹窗能打开和关闭。
- 主题与语言支持键盘选择、组合输入和分层 Escape。
- 弹层与完整输入框的宽度及左右边缘误差均不超过 1px。
- 页面和弹层均无横向溢出。
- 首次读取保留骨架与顶部进度条。
- 验收会话逐一退出，认证查询返回 401。

真实页面验收只读取业务对象。
验收不提交创建、归档或删除请求。
提交、失败重试与幂等行为由浏览器夹具回归验证。

下拉迁移与 HTTP 修复的生产模式全站回归 488 项通过。
另有 1 项按桌面视口条件跳过。
等宽修正后的生产专项回归 54 项通过。
TypeScript、生产构建与文档链接检查通过。

最终私有证据位于 `.deploy/dropdown-width-deploy-20261120/`。
前一次升级证据位于 `.deploy/dropdown-http-deploy-20261120/`。
目录包含固定身份、镜像回执、资源快照和脱敏截图。
凭据与原始配置不进入本记录。
本记录晚于镜像生成，不包含在上述固定镜像内。

[PROTOCOL]: Update this header on change, then check AGENTS.md
