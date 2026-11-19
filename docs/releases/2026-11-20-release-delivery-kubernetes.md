<!--
[INPUT]: 冻结源码、镜像回执、集群 fixture 与真实交付结果。
[OUTPUT]: 记录版本交付实现、部署身份和验收边界。
[POS]: Release 中心交付记录；部署与真实验收分别更新。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Release 中心版本交付 Kubernetes 记录

日期：2026-11-20，Asia/Shanghai。
目标实例为 `artifactory-20261119`。
旧 `artifactory` 实例保持保留范围。
Release 实现与发布门禁修复已完成。
最终源码的十项镜像已发布并完成本机验证。
限定重置只执行一次，现有业务数据继续保留。
最终镜像滚动部署、镜像发布和原生交付验收已通过。
临时夹具清理后的最终严格结构核验已通过。
Git 与规划模型均使用集群内 fixture。
原生交付仅验证 Kubernetes 中的 Linux amd64 容器。
本轮未验证公网 GitHub、外部模型或全部原生平台。

## 实现范围

项目以 Release 组织每次构建。
版本严格采用 SemVer，同项目唯一且创建后不可修改。
拒绝 `v` 前缀、构建元数据和数字前导零。
稳定版与预发布版使用独立 Release。

Release 保存目标合同和 `spec_revision`。
每次构建复制当前修订，执行全部目标。
首次构建冻结仓库绑定和完整 SourceManifest。
并发构建共用解析任务，重试保留已固定的源码身份。
AI 不能缩减目标或指定正式镜像版本。

正式发布选择当前修订的一次完整成功构建。
发布前验证源码、规划、原生和镜像执行均已停止。
构建创建、重试和发布共用 Release 锁。
发布冻结 Build、源码、文件摘要和完整 OCI 图。
失败只允许重试相同交付，不恢复为草稿。

镜像执行只写 execution 隔离的候选仓。
正式标签固定为 Release 版本，不提供 `latest` 或别名。
Gateway 按发布租约和冻结摘要白名单准入写入。
版本与根摘要永久绑定，租约过期不释放绑定。
Manifest 完成有界读取和校验后才转发。
Registry 网络策略只允许 Gateway 入口。

原生交付名包含项目、目标、版本、平台和架构。
下载票据固定交付名，GET、HEAD、Range 使用相同名称。
Release 同时提供 delivery manifest 和 SHA256SUMS。
文件字节、签名证据和对象身份保持原值。
本期不保证所有包格式的内部版本均已改写。

控制台提供版本列表、版本详情和版本内构建入口。
目标修订、发布状态和交付文件使用同一 Release 身份。
完整产品合同见[项目版本交付](../release-delivery.md)。

## 最终源码与镜像身份

本轮包含尚未提交的工作区改动。
基础提交为 `595f3166d4f72b3117b9570331fb5a7cd5832019`。
冻结快照包含 1059 个文件，共 8142945 字节。
工作区摘要为：

```text
170929781a08c04b16b14d0ca795be66a2aafd45285f2185e91703b372bc0f9a
```

十项镜像统一使用标签 `workspace-170929781a08c04b`。
目标平台为 `linux/amd64`。
该标签对应冻结快照，不是纯净基础提交的产物。
镜像由 arm64 宿主通过 amd64 仿真构建。
十一项 Rust 可执行文件完成 x86_64 ELF 身份检查。

交付仓库前缀为 `registry.kubeease.cn/pre-release/artifactory-`。
下表组件名直接追加到该前缀。
十项镜像均已发布到 Harbor，并匿名读回摘要。
远端摘要与本地 OCI 归档一致。
本轮没有发布到 GHCR。

| 组件 | 固定摘要 |
| --- | --- |
| control-plane | `sha256:ef2de2adb4e0ee5dc50a81dbba87c25abd81bbce2f78b58a54aa5797f9e4c88c` |
| gateway | `sha256:185679c1b2a068a46f8f9e9d6379ff4012950ef52c718929d495cf9a8c202476` |
| web | `sha256:2470805a83ba8e761e8df441126df1325666ca98875cdb47fa96ae053e236a85` |
| planning-operator | `sha256:8974b5a4cd5ea25361046998ded43adb3a238c8fbde9e1fa35887887ba61694b` |
| planning-worker | `sha256:c00f65c5926674dd9882ea2140309352702702e320bf2dde50fd6b1518aaf0d9` |
| image-operator | `sha256:fcff6a2d8dcd18c9876e73ad334e9bad94b42fe3540be7d41231ddd27c2f6b5b` |
| source-preparer | `sha256:7a6ddb7a77d531c0428e3c15535b38c3af13c4aabc47fa1d6990d3bf0aee5728` |
| image-worker | `sha256:08a8d3146f23aaac60077a480546cdc12837fae9b2b3ceb2c799b73913fabda2` |
| buildkit | `sha256:e0b65fc5c66ed959ec6ce47e119daa33fa48c79743817f7c307fae0e29559c55` |
| emissary | `sha256:c980388268d457748427bb3b81ab417b96896f34a509b3328942879944cfb4e7` |

Control 镜像同时提供遥测二进制。
Web 运行层为 Node `22.23.5`，UID 为 `1000`。
Planning Worker 运行层为 Node `24.22.3`，UID 为 `10001`。
Source Preparer 和 Emissary 的容器身份探测通过。
Image Worker 的 Git、Git LFS 和 BuildKit CLI 探测通过。
Emissary 镜像交付不等于设备升级完成。

Worker 的原 Node 24 镜像源授权超时。
本轮使用 `docker.m.daocloud.io/library/node`。
基础内容仍固定为原索引摘要：

```text
sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54
```

备用源匿名 raw 索引及 amd64 子清单均按摘要读回核验。
Node 版本和基础内容没有改变。
原始 1059 个冻结文件在构建前后重算一致。

## 已完成的本地验证

下表的数据库测试使用隔离夹具。
浏览器测试使用本机页面和接口 fixture。
这些结果不代表集群真实交付已完成。

| 验证范围 | 已完成结果 |
| --- | --- |
| Rust 静态检查 | workspace all-targets Clippy、格式检查和 diff 检查通过 |
| 修复后 Control 静态检查 | all-targets、all-features Clippy 通过 |
| 修复后 Rust 默认测试集 | `cargo test --locked --workspace` 完成，退出码为零 |
| Control 库与隔离数据库 | 60 项通过 |
| 业务数据库集成 | 109 项通过 |
| 重复版本与并发创建回归 | 3 项通过，重复版本返回冲突且保留原版本 |
| 发布门禁与物理停机回归 | 4 项通过，逐类验证 Operator 终态与 Pod 停机证明 |
| 规划协调与遥测数据库 | 38 项通过 |
| Helm 初始化 | 19 项通过 |
| Gateway 真实 TCP/TLS | 下载名、Range、摘要与版本篡改拒绝、权限、正文上限和认证挑战通过 |
| Web 桌面核心 | 25 项通过 |
| Web 平板与移动端核心 | 30 项通过 |
| Web 移动导航 | 32 项通过 |
| Web 桌面视觉与规划 | 24 项通过 |
| Web 并发编辑回归 | 通过 |
| Python 平台验收测试 | 38 项通过 |
| Python 镜像测试 | 23 项通过 |
| Planning Worker 镜像 | JavaScript 语法检查与 19 项容器内测试通过 |
| Web 镜像 | 登录页、文档首页与版本交付文档均返回 HTTP 200 |
| 镜像内静态文档 | 86 章、88 个 HTML 页面、11507 项本地链接和资源检查通过 |
| 限定重置脚本 | 17 项离线故障注入测试通过，独立只读复核通过 |

重置脚本等待 Deployment 和 ReplicaSet 观测零副本。
脚本保留业务 Pod 的节点集合并要求节点 Ready。
未知创建台账或未停止的执行阻止清空数据。
部分数据库重建中断后，恢复使用原 guard 元数据。
完成回执阻止重复清空已交付实例。

完整 `--include-ignored` 测试未全部通过。
Android 原生签名夹具未配置。
独立 Emissary 协议测试缺少运行中的本机服务。
专用原生服务夹具与源码 SSH 夹具未执行。
上述边界保留，不能计入已完成验证。

## 部署与真实验收状态

本轮仅重置并部署 `artifactory-20261119`。
获准清空范围为三个逻辑数据库和两个 Bucket 内容。
保留命名空间、PVC、Secret UID、CA、角色密码和基础凭据。
旧 `artifactory` 命名空间不在清空范围内。

| 验收项 | 状态 |
| --- | --- |
| 停止业务写入并保存停机证明 | 通过，七个业务 Deployment 及其 ReplicaSet 均确认零副本 |
| 清空三库两桶并保留基础身份 | 通过，三库重建且两桶保留 |
| 首次四个 Helm Release 部署 | 通过，九个 Deployment 达到预期副本数 |
| 最终源码镜像滚动部署 | 通过，四个 Helm Release 与九个 Deployment 达到预期副本数 |
| 新业务基线 `2026112001` 与遥测只读授权 | 通过，基线核验与授权恢复已完成 |
| 真实源码解析与镜像构建 | 通过，同 Release 两次构建成功并复用固定源码 |
| Release 正式发布 | 通过，镜像与原生版本均选择 Build #2 发布 |
| 正式文件下载、HEAD、Range 和校验清单 | 通过，GET 200、HEAD 200、Range 206 与摘要一致 |
| 正式 OCI 镜像 pull 与版本标签防覆盖 | 通过，完整 OCI 图、版本标签、写入拒绝与匿名鉴权均验证 |
| 真实浏览器桌面与移动端 | 六项通过，包含版本工作台、对话框导航和正式文件下载 |
| 集群 CNI 限制 Registry 仅接受 Gateway | 通过，Web 直连 Service IP 与两个 Pod IP 均超时拒绝 |
| 首次部署后持久身份比对 | 通过，旧 49 项资源与新实例持久身份保持原值 |
| 最终滚动部署后持久身份比对 | 通过，旧 Build、三个 CR、八个 Secret、两个 PVC 及旧实例均保留 |
| 临时夹具清理后严格结构核验 | 通过，严格模式检查九个 Deployment，旧 49 项资源无差异 |

限定重置于 `2026-11-20 01:19:46 +08:00` 完成。
业务 Bucket 原有零个对象，Registry Bucket 删除六个对象。
两个 Bucket 均为空，未完成的 multipart 上传数为零。
三库 owner、ACL、配置和扩展版本保持原值。
重置后的 Secret、PVC 和角色凭据哈希核验通过。
旧实例 49 项资源的 UID 和内容指纹保持原值。
Control 已启动新业务基线，遥测只读授权已恢复。
部署后再次核验旧实例资源数量、UID 和内容指纹。
最终四个 Helm Release 部署完成，九个 Deployment 全部就绪。
十三个新业务容器均 Ready，重启次数为零。
容器 `imageID` 与最终快照的固定摘要完全一致。
两个 Pool 的 Worker 镜像使用最终冻结源码身份。
滚动升级保留旧成功 Build、三个执行 CR 和持久数据。
八个 Secret、两个 PVC、角色哈希和数据库 owner、扩展均通过比对。
旧实例 49 项资源的数量、UID 和内容指纹保持原值。
Registry 沿用 Distribution `3.0.0` 的固定摘要镜像。

Web Pod 到 Registry Service IP 和两个 Pod IP 的 TCP 5000 均超时。
公开 Gateway 的 `/v2/` 返回 HTTP 401。
响应保留 Distribution API 版本与 Bearer 认证挑战。
该结果验证实际 CNI 拒绝直连且网关路径可达。

## 真实交付验收

两条链路均通过公开网关执行登录、构建和发布。
Git 仓库与规划模型使用集群内专用 fixture。
生产 Control、Operator、Worker、Gateway 与 Registry 执行真实协议。
这些结果不代表公网 GitHub 或外部模型已连通。

| 项目夹具 | 版本 | 构建 | 正式交付 |
| --- | --- | --- | --- |
| 镜像 | `1.0.0-rc.1` | Build #1、#2 成功，固定源码清单和提交相同 | 选择 Build #2 发布 |
| 原生 | `1.0.0-rc.1` | Build #1、#2 成功，固定源码清单和提交相同 | 选择 Build #2 发布 |

镜像源码提交固定为 `6edc5a1d811904693fece6b1a3caf90ec65d7d6d`。
原生源码提交固定为 `e74b7a5b6c956a3d366e53d7e8241fa47bb7f3cc`。
镜像第二次构建的源码复用通过持久化身份比对确认。
该构建的连续 watch 命令失败，不记为连续观察通过。
原生第二次构建使用 Kubernetes raw watch 连续观察。
该观察没有新增 SourceTask，原任务仍为 attempt 1。
两次构建复用同一 SourceManifest 与提交。

两个已发布 Release 均验证发布重试幂等。
换用另一 Build、修改已发布目标和再创建构建均返回 HTTP 409。
执行 Pod 已停止后才完成正式发布。

### 镜像交付

正式镜像仓库为：

```text
00000000-0000-0000-0000-000000000001/40095f7a-7a0e-4846-9844-892472041e2a/images/image:1.0.0-rc.1
```

根 OCI Index 摘要为：

```text
sha256:3aa5dc974d7ca8bb52b026d4fdab4bcab981e59a0297cb4411274f27fa083d27
```

完整 Index、Manifest、Config 和全部层摘要读回通过。
平台为 `linux/amd64`，镜像内 ELF 为真实 amd64 可执行文件。
Dockerfile RUN 生成的执行标记读回通过。
OCI 版本标签与源码 revision 标签均与 Release 身份一致。
Blob 的 HEAD 和 Range 读取通过。

普通 API Token 申请 push scope 返回 HTTP 403。
只读 Registry JWT 的四类正式写请求均返回 HTTP 403。
四类请求为相同内容、不同内容、任意标签和摘要写入。
匿名 pull 返回 HTTP 401。
Manifest 与 SHA256SUMS 下载通过，临时 Token 已撤销。

### 原生文件交付

执行环境为 Kubernetes 的 Linux amd64 Emissary 容器。
本轮只使用未签名文本夹具，不代表物理设备或其他平台验收。
Release ID 和版本环境变量进入构建后的 86 字节文件。
正式文件名为：

```text
release-acceptance-native-app-1.0.0-rc.1-linux-amd64.txt
```

文件 sha256 为：

```text
8cbbebef376ffc9461b093fbebfab536e97811cc68a1783398dd1e75d6ff39fc
```

GET 返回 200，HEAD 返回 200，Range 返回 206。
文件字节和冻结摘要一致，Range 字节与完整文件相符。
三种响应的原始 Content-Disposition 均与冻结名称一致。
`filename` 和 `filename*` 同时包含版本化正式文件名。
Release manifest 与 SHA256SUMS 下载并核验通过。

### 真实浏览器

镜像项目桌面与移动端共四项测试通过。
覆盖对话框进入版本工作台、发布状态与 manifest 下载。
原生项目桌面与移动端共两项测试通过。
浏览器实际下载的 suggestedFilename 等于冻结交付名。
原生 manifest 与 SHA256SUMS 下载通过。
桌面与手机截图已保存并人工复核。
共六项真实浏览器测试通过。

临时 Git、模型和 Emissary fixture 已按精确标签清理。
对应 Deployment、Service、Secret、NetworkPolicy 和 Pod 剩余数均为零。
两个项目、两个 published Release、四次成功构建与正式制品继续保留。
专用 Emissary 节点历史记录保留，停止心跳后自然离线。
清理后使用默认严格模式复核结构，不使用额外资源例外参数。
九个产品 Deployment 均符合预期，没有额外 Deployment。
旧实例 49 项资源无缺失、无新增、UID 与内容指纹均无变化。
滚动升级保留检查再次通过，八个 Secret 与两个 PVC 身份不变。
角色凭据哈希、数据库 owner 和扩展版本再次核验一致。
最终格式检查与 diff 检查通过。

## 首次验收发现与修正

首次验收使用集群内专用 Git 与规划 fixture。
该结果不代表集群公网 GitHub 出口已恢复。
首次镜像构建与执行均为 `succeeded`。
Source Preparer、BuildKit 与 Worker 均以零退出码完成。
执行 Pod 已停止，Worker 源码只读与凭据隔离检查通过。
严格 SemVer 非法输入返回 HTTP 400。
重复版本返回 HTTP 409。
版本和源码不可修改的接口检查通过。

首次正式发布被规划任务终态门禁阻止。
PlanningRun 使用小写 `succeeded`，原门禁只接受大写值。
修复后按 CR 种类检查各自序列化协议。
PlanningRun 接受 `succeeded`、`failed`、`canceled`、`needs_input` 和 `Superseded`。
SourceTask、ImageBuild 接受 `Succeeded`、`Failed` 和 `Canceled`。
未知状态与跨协议大小写值继续拒绝。
控制器终态不能替代 Pod 物理停机证明。
完整成功构建和全部目标合同仍由独立门禁约束。
四项回归覆盖协议差异、未知状态、普通容器与 init 容器停机。

修复后重新冻结源码并重建十项统一身份镜像。
现有数据库、Bucket 内容和验收对象没有再次清空。
首次快照 `workspace-866e59d5426c20cf` 的源码、OCI、回执和部署证据均已归档。
归档路径为 `attempt-workspace-866e59d5426c20cf/`。

公网 GitHub 出口曾超时，本轮未将 fixture 结果视为公网恢复。
最终镜像内文档来自冻结时的进度记录。
冻结后追加的摘要与最终验收结果不在镜像内文档中。
私有部署证据位于 `.deploy/release-deploy-20261120/`。
凭据、私钥、会话和原始配置不写入本记录。

本轮 Git 与规划模型均为集群内 fixture。
原生执行只覆盖 Kubernetes 的 Linux amd64 未签名容器夹具。
物理设备、应用签名、其他原生平台、公网 GitHub 和外部模型未验收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
