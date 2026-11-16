# AI-Workspace macOS 托管证书签名验收

日期：2026-11-17，Asia/Shanghai。
第 8 次构建已成功。正式签名、S3 下载复核和材料清理均通过。
应用尚未公证。Gatekeeper 返回 `Unnotarized Developer ID`。

[构建 #8](http://127.0.0.1:38100/tenants/default/projects/ai-workspace/builds/8)
提供两项受控下载。
[此前双平台验收](ai-workspace-build-acceptance-2026-11-16.md)
记录第 5 次构建的未签名 Windows 包和 ad-hoc Mac 包。
两轮构建和制品相互独立。

## 范围与固定输入

本轮验证 AI-Workspace 1.1.22 的 macOS arm64 客户端。
源码固定为 `8b2800641bf82c69d8a850fbeb15a7f37304d832`。
项目为默认租户下的 `ai-workspace`。
产品仓库的已跟踪文件保持干净。
原始源码和 `bun.lock` 不作修改。
隔离构建副本承载依赖、运行时组装和生成文件。

指定节点为 `codehub-mac-mini`，服务账号为 `artifactbuild`。
节点 ID 为 `24c78c09-9718-438c-ab4c-7cbdca0c9f7f`。
节点运行 macOS 27.2 arm64。
构建入口使用 Node 22.23.4 和 Bun 1.3.14。
本轮只运行 Mac 目标，不升级其他节点。
构建发布状态为 `not_requested`。

外置签名入口清单 SHA-256 为：
`0b00977eea0f19536fbc244111d26431c6b26e713f604e37e9ad5ddf661d2202`。
它调用此前通过验证的 v6 原生构建入口。
v6 清单 SHA-256 为：
`af0f713ea829e8cc0f66b9600a01bb342a488d1cca26534a2623941d5fe222fd`。
计划中的三段脚本与审查后的入口逐字一致。

## 证书与授权

导入材料为用户提供的 `cert.p12`。
已验证其私钥与内嵌证书匹配。
单独提供的 `.cer` 与该身份不同，本轮采用 PKCS12 内嵌身份。
导出密码只在进程内读取，不写入计划、命令文本或 Git。

| 项目 | 固定值 |
| --- | --- |
| 身份 | `Developer ID Application: Bangdao Technology Co., Ltd (QDZPPB35A9)` |
| Team ID | `QDZPPB35A9` |
| SHA-1 | `81752A480D118345F3EC56AA920F3EDF71B81FC5` |
| SHA-256 | `ad7970e11d2b6d645f534288796eb4e95f95983227774fc910d9486e072d7a1f` |
| 有效期（UTC） | 2026-06-01 09:24:37 至 2027-02-01 22:12:15 |
| 凭据 | `aee14d83-be6b-4dc4-aa58-760fd93a1221` |
| 不可变版本 | `f590ad69-095b-4608-b9e2-f5741574b7d0` |
| 方案 | `managed:ai-workspace-macos-developer-id` |
| 公证要求 | `notarize: false` |

平台加密托管材料，项目将该方案设为 macOS 默认。
指定节点显式获得方案授权。
构建创建时固定上述版本。
签名步骤通过真实节点 mTLS、会话、任务及租约领取材料。

## 构建与签名结果

| 标识 | 值 |
| --- | --- |
| Build | `5b62b587-2a44-4746-961a-3684f9e8e73d` |
| Plan | `37687412-a4a8-4216-9742-ebeda4d109e8` |
| Attempt | `80acbd5f-64e4-4b04-90e8-f0a7781d409f` |
| 源码快照 | `e6149aa1-b3fb-495f-b982-b9300eacd950` |

Qwen 用 6 次调用完成规划。
原生构建从 11 月 16 日 23:56:03 开始。
build、sign、verify 三步均返回 0。
11 月 17 日 00:09:23，平台确认构建成功。

签名使用任务临时 Keychain。
`@electron/osx-sign` 显式接收 Keychain 和身份。
全部 Mach-O 逐项验证严格签名与 arm64 架构。
共 129 项通过，Team ID 均匹配。
外层应用具有 hardened runtime 和可信时间戳。
证书链和指纹匹配上述固定版本。

签名后调用产品 hook 的 Claude CLI 元数据刷新函数。
更新 `version.json` 后重新签署外层应用。
最终 Claude CLI 的大小和 SHA-256 与元数据一致。
DMG 镜像自身也已签名。
ZIP 解包应用和 DMG 挂载应用均通过严格验签。

Electron 37.3.1 使用内嵌 Node 22.18.0、ABI 136。
它从 `app.asar` 加载正式包内的原生模块。
better-sqlite3 查询返回 42。
sharp 成功写入并读取 4×4 PNG。
签名后再次运行内置运行时 smoke，结果通过。

独立内置 Node 为 24.11.1。
Python 为 `3.12.13+20260414`。
两者的原始归档摘要匹配固定源码清单。
原始归档和运行时完整性检查在正式签名前执行。
签名后的二进制摘要单独记录在 `nested_macho` 中。
Git 2.56.0 和 Bash 5.3.20 从节点 Homebrew 重新组装。
它们与原源码运行时清单版本不同。
替换仅发生在隔离副本，来源和摘要写入验证报告。
本轮不声明所有运行时版本与原清单一致。

## S3 下载复核

平台复制最终制品后，对同一副本验签、计算摘要并上传。
两项证据均固定方案、版本、路径和证书指纹。
本机通过受限下载接口重新取得两项对象。
00:12:33 完成独立复核。

| 制品 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `AI-Workspace-1.1.22-mac-arm64.dmg` | 588257544 | `7ab5eaa8e929a4efbd59474ddf8bd15c01d419a9feec7197b635f508040dd8a6` |
| `AI-Workspace-1.1.22-mac-arm64.zip` | 438121381 | `e0eecf4117c4ad72817ae3f1ef89d8a7ca45bf2a8d25e24b581e45ec29c00635` |

大小、摘要、节点报告及平台证据相互一致。
下载的 DMG 通过 `hdiutil verify` 和镜像签名检查。
本机从挂载 DMG 和解压 ZIP 分别验证应用。
严格签名、证书 SHA-256、Team ID、时间戳和 arm64 均通过。
两个副本的 Claude CLI 元数据也均匹配。
ZIP 全部压缩数据通过 CRC 检查。

初次本机 DMG 挂载点位于外置卷工作区，挂载被拒。
改用系统临时目录后，挂载和验签成功。
两个下载包的最终复核均已完成。

## 清理与审计

00:12:39 完成最终只读检查：

- 节点在线，`active_jobs=0`，未排空、未撤销。
- 临时材料目录为空，清理闭锁标记不存在。
- 本轮工作目录已由节点回收。
- verify 阶段不存在签名材料环境变量。
- 用户 Keychain 搜索列表和默认配置前后一致。
- 服务账号初始没有默认 Keychain，完成后仍保持该状态。
- 方案、节点身份、池、标签、槽位和证书到期时间保持一致。

审计中有 1 条 `signing_material_acquired`。
其 actor 为指定节点，attempt 为本轮，step 为 sign。
另有 2 条 `signing_output_verified`。
两条路径、摘要和版本分别匹配最终 DMG、ZIP。
日志、计划和报告扫描未发现原 PKCS12 Base64 或私钥 PEM 标记。

私有证据位于 `.local/ai-workspace-signing-20261116/`。
目录权限为 `0700`，文件权限为 `0600`。
关键记录包括 `mac-final-report.json`、`s3-verification.json`、
`audit-final.json`、`mac-final-cleanup.json` 和 `final-verification.json`。
两项下载包保存在该目录的 `downloads/` 下。

## 真实验证发现与源码交付

第 6 次构建被默认 Keychain 预检拦截。
后台服务账号没有默认 Keychain 是有效状态。
外置入口已接受并比较该状态，构建取消且无制品。

第 7 次构建完成编译和正式签名。
后续证书提取使用了错误的命令格式。
本轮同时修复入口和节点原生验签代码：

- `codesign --extract-certificates=前缀` 用等号传递前缀。
- `security verify-cert -p codeSign` 使用有效的原生 policy。

旧 `codesigning` policy 会输出创建失败却返回 0。
修复后真实证书链校验参与最终成功条件。
第 7 次构建已取消，无制品，失败后材料清理通过。
第 8 次构建从固定源码重新编译并完成本报告全部验证。

修复提交为 `cf14d165d1d9e64aa35283a7ade81287e8e1ce41`。
本机 Emissary 测试为 45 项通过、4 项显式忽略。
忽略项要求 Android 工具、独立实时环境或系统服务授权。
格式检查和全部目标、全部特性的 Clippy 通过。
原生回归使用真实 codesign、security 和系统应用证书。
它同时检查证书链提取、信任与错误指纹拒绝。
测试提交 `7c53a3caf1645eb08d4f04610df448b079a55223`
将夹具验证日期固定到证书链有效期交集。
生产验签继续使用当前时间和系统信任规则。
该回归及 Clippy 已在本机再次通过。

[源码修复的 CI](https://github.com/zhubby/artifactory/actions/runs/43901324483)
中，Mac ARM 原生 job 和 Compose TLS 通过。
完整 CI 尚未全绿。
kind 初始化因 kubeadm `v1beta3` 配置被拒而失败。
Linux 数据库轮次为 95 项通过、2 项失败、1 项过滤。
失败涉及 SSH fixture 和 placement 重选用例。
Windows OTA fixture 在建立连接前超时。
旧版 Mac Intel 的系统应用信任回归也失败。
[固定夹具日期后的 CI](https://github.com/zhubby/artifactory/actions/runs/43904698883)
截至 00:21，Linux 和 Windows 的四个原生 job 均通过。
两个 macOS job 仍在等待 runner。
Mac Intel 的夹具修正尚未获得该 runner 的复核结果。

## 已部署范围与验收边界

[Kubernetes 交付记录](releases/2026-11-16-managed-signing-kubernetes.md)
记录 Helm revision 11 的托管签名服务。
服务端镜像源码保持 `6ce851d2c94451286cd3be4b1a99b5df75c1e7bf`。
本轮新生产行为仅位于 Mac 节点原生验签。
指定节点已安装 `emissary-v0.2.0-signing-verify-20261116`。
该二进制来自 `cf14d16`，SHA-256 为：
`990a83df1089e35d3a4f8bc4256f829a42ffe6ba73ed79df065d54fee6839794`。
节点身份和 OTA 信任配置保持一致。
最终 3 个 Control 副本全部就绪。
命名空间内 10 个 Pod 全部 Ready，容器零重启。

本轮证明真实证书托管、领取、签名、原生验签及 S3 读回。
没有 Apple 公证凭据，未提交公证或装订票据。
`spctl` 返回 3，结果为 `rejected`，来源为 `Unnotarized Developer ID`。
`stapler validate` 返回 65，未找到票据。
正式签名通过不代表 Gatekeeper 已允许分发。
人工安装、首次启动和图形界面交互尚未验收。
Android 真机覆盖升级不属于本轮 Mac 验收。

[PROTOCOL]: Update this header on change, then check AGENTS.md
