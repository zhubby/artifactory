# AI-Workspace 双平台真实构建验收

日期：2026-11-16。
当前状态：双平台真实构建、S3 读回和内部发布通过。

后续正式签名见 [2026-11-17 macOS 托管证书验收](ai-workspace-signing-acceptance-2026-11-17.md)。该轮使用用户证书重新编译并签名 Mac 制品，未执行公证。

内部版本为 `1.1.22-artifactory.20261116`。
第 5 次构建生成 Windows EXE、Mac DMG 和 ZIP。
两个目标各附验证报告与 SHA-256 清单，共 7 个对象。
全部对象已从内建 S3 下载并核对。
[内部 Release 页面](http://127.0.0.1:38100/tenants/default/projects/ai-workspace/releases/31f054bc-9bca-4b0a-8f44-b234313d9415)。

## 范围

使用默认租户的 `ai-workspace` 项目。
固定提交为 `8b2800641bf82c69d8a850fbeb15a7f37304d832`。
产品版本为 AI-Workspace 1.1.22。
仅授权 `codehub-windows` 和 `codehub-mac-mini`。
目标为 Windows x64 和 macOS arm64。
最终制品存入内建 S3，再显式创建内部 Release。

产品仓库保持只读，不写入构建配置或源码修改。
构建验收后，用户另行授权提交并推送本轮 Artifactory 修复到 `main`。
Windows 使用未签名包。macOS 使用 ad-hoc 签名。
正式签名、公证和人工安装体验不属于本轮验收。

## 模型与流程

项目使用 DashScope 的 OpenAI Chat Completions 接口。
地址为 `https://dashscope.aliyuncs.com/compatible-mode/v1`。
模型为 `qwen3.8-max`，关闭思考模式。
集群 Worker 内的真实探针返回 HTTP 200。
密钥只保存在私有配置和平台加密凭据中。

网页向导提交固定源码和双平台目标。
确认页保持“同时生成内部 Release”关闭。
先验证所有最终制品，再显式执行内部发布。

## 已确认的问题与修复

| 现象 | 根因与处置 | 当前证据 |
| --- | --- | --- |
| 用户指定的 HTTP Git 入口被拒绝 | 接入用户指定的 HTTP 协议，保留外置凭据与同源继承约束 | source-core 13 项、control-plane 14 项通过 |
| 供应商超时显示为规划总时限 | Worker 将供应商超时与总时限分别分类 | Worker 16 项通过，修复镜像已部署 |
| Mac 心跳正常，源码入口超时 | 节点 port-forward 通道失效；空闲时重建网关并重连 | Mac 本机入口 HTTP 200 |
| Windows 干净源码报 source_modified | NTFS 不表示 Unix 执行位；按宿主选择 Git filemode 策略 | 原生修复前失败，修复后 3 项通过 |
| Mac 无法参与新版选机 | 旧版 Emissary 未发送启动元数据；维护升级运行时 | 已在线并上报 36 项 metadata |
| 第一轮计划使用错误的脚本变量与 ABI | 补充平台侧执行契约，要求使用真实 cwd 与打包 Electron | 第 5 次构建双平台已通过 |

Windows 源码校验仍拒绝已跟踪内容变化。
Unix 源码校验继续检查执行权限位。
Mac 的同组 3 项源码回归也已通过。

## 构建记录

| 构建 | 标识 | 结果 |
| --- | --- | --- |
| #1 | `d11e8f68-34e8-446f-8fba-1ac3bf85f60e` | 原供应商连接超时，无制品 |
| #2 | `8f061fb7-8f96-4fb2-8f25-396d3704c147` | Qwen 完成规划；源码通道、权限及计划问题导致失败，无制品 |
| #3 | `2baffa0d-8dc8-449f-a987-8bb42c25dafd` | Qwen 恢复规划后启动双平台；脚本审查未通过，已取消，无制品 |
| #4 | `41f5d5b6-af8a-487c-bf7c-1859a411b632` | Windows 成功并上传 3 个对象；Mac 被验证入口的大小门槛拦截，整体失败 |
| #5 | `04b99252-6a43-4321-84f2-806fcf36bb54` | Qwen 6 次调用生成 v6 计划；双平台成功，7 个对象的 S3 下载校验通过，内部发布成功 |

Windows 维护镜像为 `emissary-v0.2.0-winmode-20261116`。
镜像 SHA-256 为 `2db38a752330dfc6bafd02b6675523121996c6cf2426fceb4da948d7a5083277`。
原启动器保持 `emissary-v0.2.0`。
原运行时保留在上一版本槽位。
控制平面已读回新版本和在线状态。
本次通过节点本机维护安装，不创建外部 Emissary 标签或 Release。

Mac 维护镜像为 `emissary-v0.2.0-metadata-20261116`。
镜像 SHA-256 为 `789fa87958021289794efa0a195f9be0bdecafa07a0f7124165c790f5a021c12`。
两台节点的身份配置和 OTA 信任配置均保持不变。
Mac 服务重载需等待旧 launchd supervisor 退出。
维护脚本已增加退出等待和有界重载重试。
第 3 次构建通过网页补充升级结果，再继续规划。
随后审查发现 Windows 复制命令排除了 `bun.lock`。
两个平台的原生模块和运行时证据也不完整。
Mac 脚本还忽略了 Homebrew 契约检查失败。
本次主动取消构建，不采用其输出或校验声明。
取消后已读回两个节点的 `active_jobs=0`。

后续构建使用固定摘要的外置构建入口。
Qwen 负责选机、执行计划与制品收集。
外置入口调用固定源码的原生打包脚本。
入口只在隔离副本中写依赖缓存及生成的运行时清单。
原始下载归档必须匹配源码清单的 SHA-256。
Electron 头文件通过官方 SHA-256 清单核验。
每项失败均中断步骤，不写成功报告。
第 4 次构建的外置入口清单 SHA-256 为：
`0c1b170ebc3977fe17bfcee8eefbd4538ca1c1453d9a5a7b4f14ab765c89519d`。
两台节点已读回各文件摘要一致。
Windows 独立 Python 工具的 Authenticode 签名有效。
签名主体为 Python Software Foundation。
Mac 的 Electron 37.3.1 已通过独立 ASAR 读取探针。

第 4 次构建首先拦截了 Office CLI 缓存不一致。
官方 v1.0.160 资产摘要为 `fe2b31c6c27534dfbbf155386b6b9105420f9315bbb606335ec61490d171386d`。
已下载同版本文件，核对官方资产摘要后原子替换缓存。
替换前的文件保留为节点本地备份。

旧运行时缓存是安装目录的重打包归档。
这些文件不能替代源码清单中的原始下载归档。
新版入口改用 Node 和 Python 原始归档及原始 SHA-256。
Git 和 Bash 通过固定源码的运行时组装器重新生成。
组装先执行真实 Homebrew 动态库契约检查。
前缀从已安装的 formula 路径读取，不写 Homebrew 锁目录。
组装器保留动态库重定位、符号链接及 ad-hoc 签名检查。
本轮组装的工具为 Git 2.56.0 和 Bash 5.3.20。
其版本和新归档 SHA-256 写入隔离清单及验收证据。
产品源码和 `bun.lock` 保持固定。

Windows 服务进程没有加载已有的 MSVC 环境。
v5 入口加载 `vcvars64.bat` 后再执行原生命令。
独立目录已用固定源码编译更新程序成功。
生成文件为 502784 字节，不进入正式制品。
Windows 嵌入 Python 默认隔离模式阻断 `PYTHONPATH`。
入口在独立工具目录删除 `._pth` 后再运行 node-gyp。
独立探针已确认隔离模式关闭且自定义模块可导入。

v5 外置入口已部署到两台节点并读回摘要一致。
其清单 SHA-256 为：
`bb5bd9677f840df259edcb489c784f9f67d012d180efce077a2f420ab3b72339`。
旧 v4 入口保持不变。

网页补充说明后，Qwen 用 6 次调用生成 v5 计划。
计划为 `97336538-f8f1-47a1-9c12-a5db80b5e206`。
计划脚本与审查后的 v5 入口逐字一致。
Mac 原生打包命令已成功生成 DMG 和 ZIP。
后续校验错误地要求 Electron 启动程序大于 1 MB。
官方 Electron 37.3.1 arm64 启动程序为 50472 字节。
v6 入口移除该任意大小门槛，保留实际文件检查。
后续架构、严格签名及原生运行校验继续阻断错误。
v6 清单 SHA-256 为：
`af0f713ea829e8cc0f66b9600a01bb342a488d1cca26534a2623941d5fe222fd`。
两台节点已读回 v6 各文件摘要一致。
独立预检包不作为正式发布输入。
v6 独立预检已通过以下真实检查：
arm64 架构、严格 ad-hoc 签名、包内 SQLite 与 sharp。
运行时动态库、符号链接及各工具 smoke 均通过。
DMG 校验和与 ZIP 压缩数据检查均通过。
预检的 `bun.lock` 摘要匹配固定源码。

第 4 次构建终态不支持继续提交澄清。
第 5 次构建重新从固定源码编译两个目标。
其计划脚本与 v6 入口逐字一致。
所有目标和步骤的超时均为 7200 秒。
第 4 次构建的 3 个 Windows 对象已从 S3 下载。
其大小、SHA-256 与验证报告交叉引用全部通过。
第 5 次构建不采用这些旧构建对象发布。
第 5 次构建的 4 个 Mac 对象已全部从 S3 下载。
大小、SHA-256、报告交叉引用均通过。
下载后的 DMG 再次通过 `hdiutil verify`。
下载后的 ZIP 逐文件 CRC 检查通过。

## macOS 已通过的正式制品验证

第 5 次构建的 Mac attempt 为：
`cc5c59d4-470f-4edf-a41d-9924ac3403ec`。
S3 下载复核时间为 2026-11-16 18:29（北京时间）。
包内 Electron 37.3.1 使用 ABI 136。
它从 `app.asar` 加载 better-sqlite3 和 sharp。
SQLite 查询返回 42。sharp 写入并读取 4×4 PNG。
应用通过 arm64 与严格 ad-hoc 签名校验。
运行时动态库、符号链接与工具执行检查通过。

| 对象 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `AI-Workspace-1.1.22-mac-arm64.dmg` | 428284795 | `93621188dbbaf3f07b66f5323f1b747a791541e671caa0a67f973349f13c745e` |
| `AI-Workspace-1.1.22-mac-arm64.zip` | 437103585 | `a6ae7c2d1925d1622b475515327ef1e19ff0920f367d2447df028a215caa756e` |
| `BUILD-VERIFICATION.json` | 6571 | `bddf5afc0cabd1e362d61fc4fd0b865c6b6a41a80922e0439d8fbf9e9fe0ab57` |
| `SHA256SUMS.txt` | 290 | `502aa166ac028b424a79f4021249c221e31e06ea80537bfd57235493026aa381` |

Node 24.11.1 和 Python 3.12.13+20260414 使用原始上游归档。
它们的摘要匹配固定源码的运行时清单。
Git 2.56.0 和 Bash 5.3.20 从节点 Homebrew 重新组装。
这两个版本与原源码运行时清单不同。
只在隔离构建副本替换这两个运行时项。
来源、版本与摘要已写入 `BUILD-VERIFICATION.json`。
不能据此声明所有运行时版本均与原清单一致。

## Windows 已通过的正式制品验证

第 5 次构建的 Windows attempt 为：
`0f264553-fe67-4bad-b1a6-614ed7ec7ac6`。
该目标于北京时间 18:52:27 成功结束。
应用主程序通过 PE x64（`0x8664`）架构检查。
安装器 Authenticode 状态为 `NotSigned`。
NSIS 引导程序本身允许使用 x86。
包内 Electron 37.3.1 使用 ABI 136。
它从 `app.asar` 加载 better-sqlite3 和 sharp。
SQLite 查询返回 42。sharp 写入并读取 4×4 PNG。
固定源码的更新程序通过 MSVC 原生编译。

`runtime.7z` 为 125845134 字节。
真实 SHA-256 为：
`8d9319955d04ec54c519fd1ef94084cc2daa2eec1bffa5ea2f34705ea2cad4b3`。
`a4P6FnZkriMc` 仅为短缓存 ID。
归档通过测试，并实际解出 14782 个文件。
解包后的 Node 24.11.1、npm/npx 11.6.2 检查通过。
Git 2.51.2.windows.1、Python 3.12.13 检查通过。
Bash 5.2.37 执行检查通过。
安装包的 `7za t` 返回 0 并输出 `Everything is Ok`。
日志同时保留安装器尾部数据警告。

| 对象 | 字节数 | SHA-256 |
| --- | ---: | --- |
| `AI-Workspace-1.1.22-win-x64.exe` | 359221914 | `4a22430184c88d46d0a16b171b0dae1145f45e5099325435c5de5d4f2c27a2fc` |
| `BUILD-VERIFICATION.json` | 2415 | `ea4e6f6c2254e11924c290fcf04708c2b975016d58c1b2921afb93bea508b17a` |
| `SHA256SUMS.txt` | 188 | `5648292064be5eb451ee1178b1dd2a1ba56febd9fd93191796a0644d793a10f5` |

## S3 读回与内部发布

北京时间 18:53:17，全部 7 个对象的下载复核通过。
每个对象的字节数与 SHA-256 匹配平台记录。
两个 `SHA256SUMS.txt` 的文件引用完整。
两个验证报告均匹配固定提交、锁文件和 v6 入口。
报告中的包大小、摘要与实际下载文件一致。
Mac 对象复用本轮已下载且重新计算摘要的读回记录。
Windows 对象来自本轮新上传的对象 ID。

随后在网页点击“发布制品”。
填写版本后点击“生成内部 Release”。
北京时间 18:53:50，发布状态变为 `succeeded`。
Release ID 为 `31f054bc-9bca-4b0a-8f44-b234313d9415`。
读回确认版本为 `1.1.22-artifactory.20261116`。
来源为第 5 次构建及本报告的固定提交。
发布目录绑定上述 7 个原始 S3 对象。
对象 ID、attempt、目标、路径、大小和摘要全部一致。
发布过程没有重新编译或复制制品。

## 网页与证据

真实浏览器已核对以下页面状态。
构建详情显示两个目标成功，发布状态成功。
内部发布列表显示 1 个版本与 7 个制品。
Release 详情显示完整提交、7 行制品及受控下载入口。
1440×1000 截图中，表格与操作入口均可见。

安全证据保存在本机私有验收目录。
`build-5-s3-verification.json` 保存 7 个对象的读回结果。
`build-5-release-readback.json` 保存 Release 读回结果。
`build-5-release-verification.json` 保存对象绑定核对结果。
`build-5-succeeded-before-publish.png` 记录构建终态。
`build-5-publish-confirmation.png` 记录显式发布表单。
`build-5-published.png` 与 `build-5-release.png` 记录发布结果。
构建终态和 Release 详情截图已实际打开核对。
凭据和认证文件不属于公开验收证据。

## 验收边界与后续建议

本轮完成真实原生构建、自动化包验证和内部发布。
Windows 安装器未签名。
Mac 使用 ad-hoc 签名，未执行公证。
未执行人工安装和图形界面操作验收。
Mac 的 Git/Bash 版本变化已在本报告和随附报告披露。
Artifactory 源码修复与验收文档按追加授权交付到 `main`。

正式分发前，补齐平台正式签名、公证与人工安装验收。
后续将 Mac Git/Bash 的归档来源和版本纳入统一清单。

[PROTOCOL]: Update this header on change, then check AGENTS.md
