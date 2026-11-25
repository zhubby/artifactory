<!--
[INPUT]: Artifactory 统一版本、节点 mTLS、签名清单和固定启动器。
[OUTPUT]: 定义管理员发起的同版本升级、信任、排空和失败恢复。
[POS]: 原生节点升级的发布与运维契约。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary OTA 升级

全部一方组件共用根 `VERSION` 定义的 Artifactory 统一版本。
Emissary 的升级目标固定为当前部署的产品统一版本。
节点通过已有 mTLS 出站连接接收升级命令，使用私有 S3 的短期签名地址下载。
普通升级不需要 SSH 或管理员密码。
升级由租户管理员在控制台发起。
版本不一致只显示可用升级，不自动创建更新或排空节点。
发行导入、节点上线和池恢复均不代表升级授权。

节点当前版本与产品统一版本相同时，不重复升级。
历史 `v`、`emissary-v` 前缀和合法构建元数据不改变产品版本。
例如 `0.2.0+terminal.abc` 与 `0.2.0` 属于同一产品版本。
版本不一致时，只选择当前租户中对应版本的签名发行。
缺少该发行或出现多个标签歧义时，不创建更新、不排空节点。
版本目录的导入时间不参与目标选择。

## 从控制台发起升级

在节点详情的运维分区查看节点当前版本和 Artifactory 统一版本。
确认对应签名发行已就绪、节点在线且 OTA 启动器已启用。
管理员点击「升级至目标版本」，确认后创建一次持久化更新。
控制平面冻结目标发行，并通过现有 mTLS 连接下发命令。
节点完成验签、等待在途构建结束、激活和健康确认。
候选版本失败时，启动器恢复原版本并上报结果。

同一节点重复提交当前目标时，返回原未完成更新。
目标固定为产品统一版本，浏览器不能指定其他版本。
确认请求携带当时的产品版本和发行 ID，作为目标前置条件。
服务端目标已变化时拒绝请求，要求重新确认。
节点离线、已吊销、证书过期或缺少 OTA 支持时拒绝发起。
节点池暂停或并发额度已满时，先恢复策略或等待其他更新完成。
新安装命令自动初始化启动器，但不授权后续升级。
已有旧节点缺少远程初始化协议，须先完成一次启动器准备。

## 发布与信任配置

`.github/workflows/emissary-release.yml` 在六种原生 runner 上测试、构建 Linux、macOS、Windows 各自的 amd64/arm64，然后生成 `emissary-manifest.json` 和 `emissary-manifest.sig`。每个平台都必须通过普通测试和原生系统服务验收，发布 job 才能运行。控制平面导入必须包含全部六个资产。manifest 包含版本、Git commit、协议兼容范围和每个二进制的大小及 SHA-256。签名覆盖 manifest 的原始字节。协议主版本仍为 1。

在受保护的 GitHub `emissary-release` Environment 中设置 `EMISSARY_RELEASE_SIGNING_SEED`，值为独立 Ed25519 私钥的 32 字节 seed 的 base64。只允许发布 job 读取该 Secret。推送 `emissary-v*` Git 标签，或手动执行工作流并选择一个已存在的 Git 标签。工作流不覆盖已有 Release。

部署管理员通过 `ARTIFACTORY_EMISSARY_RELEASE_SOURCE` 指定发行配置文件。
未设置时读取 `${ARTIFACTORY_DATA_DIR}/emissary-releases.json`。
Helm 的 `controlPlane.releaseSourceConfigMap` 将同一公开信任配置挂载到全部副本。
文件示例：

```json
{
  "repository": "owner/artifactory",
  "api_base": "https://api.github.com",
  "public_keys": ["BASE64_ED25519_PUBLIC_KEY"]
}
```

私有仓库可为控制平面提供 `ARTIFACTORY_EMISSARY_GITHUB_TOKEN`。`api_base` 也可指向预先配置的 GitHub Enterprise API；生产环境要求 HTTPS。浏览器提交的字段只有版本标签，不能指定任意下载 URL。下载地址由固定仓库和 GitHub asset ID 构造，重定向限制为配置的 API 主机和 GitHub HTTPS 下载主机。

版本导入必须通过签名、六个平台、协议、大小、完整内容的 SHA-256 校验。全部校验成功后才建立版本目录记录。对象按 manifest 摘要存储；版本号内容不可覆盖，同内容重复导入返回原 ID。未配置验签公钥时拒绝导入。测试密钥必须独立于正式发布密钥。

## 一次性安装固定启动器

新安装使用 Rust Emissary、全新的 `emissary.json` 和注册身份。管理员为专用普通服务账户准备 SDK、PATH、签名环境和二进制后，以该账户初始化 OTA，再安装和启动原生服务：

Windows 的 MSVC 构建还需要与 Emissary 架构匹配的 Microsoft Visual C++ v14 运行库。
从 Microsoft 获取 [amd64](https://aka.ms/vs/17/release/vc_redist.x64.exe) 或
[arm64](https://aka.ms/vs/17/release/vc_redist.arm64.exe) 安装包，校验 Microsoft
Authenticode 签名后由管理员安装。缺少 `VCRUNTIME140.dll` 时，程序可能以
`0xc0000135` 退出且没有命令行输出。运行库属于一次性主机准备，未包含在 OTA 资产中。

准备完成后执行：

```sh
artifactory-emissary ota-init --config /path/to/emissary.json --public-keys BASE64_ED25519_PUBLIC_KEY
artifactory-emissary service --action start --config /path/to/emissary.json
```

`ota-init` 拒绝覆盖已有 OTA 配置。它在配置旁创建独立的 `ota.json`，将初始运行程序复制到服务账户管理的版本目录。设备配置严格解析 `emissary.json`，拒绝未知字段和重复字段。

```json
{
  "directory": "/path/to/managed-root/ota",
  "public_keys": ["BASE64_ED25519_PUBLIC_KEY"]
}
```

`run` 和 `service-run` 检测 OTA 配置后启动固定 supervisor；没有 OTA 配置时直接运行 Emissary。版本目录、升级记录和邮箱仅供服务账户访问。Windows 需给该账户授予目录的继承修改权限，保留证书和服务配置原有私有 DACL。固定服务 EXE 不参与 OTA，避免覆盖运行中的 Windows EXE。不要以 root / LocalSystem 运行构建。

## 运行与恢复

控制平面使用节点事务锁协调升级与任务分配，`ota_draining` 独立于管理员 `draining`。
每个节点最多一个未完成升级。
节点与池的历史版本覆盖不再改变目标。
每池默认同时一个升级，失败或回滚会暂停后续升级。
节点离线时拒绝创建更新；重连后仍需管理员发起。
已接受的更新断线后，按原执行记录恢复命令发送。
控制平面滚动升级期间，已开始的更新继续使用原冻结清单。
新更新使用接收请求的服务实例所运行的产品统一版本。

阶段为：等待接收 → 下载校验 → 排空 → 启动确认 → 成功。下载在启动器后台进行，不阻塞 Emissary 心跳。排空要求任务终态得到控制平面确认、Worker 退出、所有日志得到确认；Worker 的产物登记先于任务终态。旧运行程序先通过已认证心跳报告即将激活，取得控制平面确认后才允许启动器切换，因此候选程序无法连接时也保留可见的激活阶段。不会为了升级取消构建。

候选运行程序必须在 120 秒内以原证书重连，报告目标版本，并取得同一连接连续三次心跳确认，才能原子提交当前版本。重连会重置确认计数。启动失败、确认前退出或超时会恢复原版本；节点成功报告完成前，控制平面持续阻止新任务。升级命令带持久化单调序号，重复和迟到命令不重启已完成操作。

`state.json` 原子持久化当前、候选、上一版本及升级结果。启动器重启发现未完成激活会恢复原版本；下载或排空期间被中断会报告失败并保留原版本，不自动循环重试。Windows 使用 Job Object 清理候选进程树；原生服务仍负责自动启动及启动器故障重启。

原生服务停止或重启时，启动器关闭私有控制管道通知运行程序退出，运行程序等待 Worker 清理构建进程树和工作目录，并保留当前 mTLS 连接最多 5 秒等待任务终态和日志确认；启动器等待 8 秒后仍未退出才强制结束。Linux systemd 使用 `KillMode=mixed`，先通知主服务，超时再清理整个服务控制组。macOS 安装器设置 launchd `ExitTimeOut=15`，避免系统默认的较短期限提前结束启动器、留下不响应退出的候选。运行程序仅响应进程停止信号，网络连接断开产生的 `SIGPIPE` 不会触发退出。此退出路径独立于升级排空流程。

取消排队升级立即恢复调度；已送达节点的取消要等待启动器确认。
已经进入激活的操作继续完成或回滚，界面显示最终结果。
取消不会修改原来的管理员排空状态。
失败、回滚和取消的同一目标不会自动循环重试。
管理员只能重试仍匹配当前产品统一版本的失败更新。
节点已是当前版本时，拒绝重复重试。
候选失败时，启动器仍自动恢复上一运行版本。

`spool/` 中存在未确认日志时保持排空，不能通过删除日志绕过。可检查节点磁盘、证书期限、连接状态与产物存储后恢复传输。固定启动器自身、SDK 和系统工具链不属于第一版 OTA；启动器更换需要管理员安装。证书也不会自动续签。

## 管理接口

均位于 `/api/v1`。查询要求会话 viewer，写入要求管理员会话、`x-artifactory-csrf: 1` 和与 `ARTIFACTORY_PUBLIC_URL` 一致的 Origin。项目 Token 无权访问管理接口。目标、策略、导入、重试、取消和终态均有审计。

| 方法与路径 | 请求 / 行为 |
| --- | --- |
| `GET /emissary-releases` | 已导入版本目录 |
| `GET /emissary-version` | `product_version`、唯一同版本发行及 ready / missing / ambiguous 状态 |
| `POST /emissary-releases` | `{"tag":"emissary-v0.2.0"}` |
| `POST /nodes/{id}/emissary-update` | `{"product_version":"0.2.0","release_id":"当前发行 UUID"}` 校验确认目标后发起升级；返回 `{"id":"更新 UUID"}`，重复请求返回原执行 ID |
| `PATCH /nodes/{id}/emissary-update` | 兼容历史目标配置，不创建更新；UUID 只接受唯一同版本发行 |
| `PATCH /pools/{id}/emissary-update` | `{"release_id":null,"concurrency":1,"paused":false}`；修改并发与暂停，不发起升级 |
| `GET /emissary-updates` | 最近 500 条执行记录 |
| `POST /emissary-updates/{id}/retry` | 管理员重试终态失败 / 回滚 / 取消记录，复验节点与池准入，生成新执行 ID |
| `POST /emissary-updates/{id}/cancel` | 取消尚未进入激活的执行 |

节点页显示节点当前版本、Artifactory 统一版本、启动器和升级状态。
设置页提供签名发行导入、池并发 / 暂停策略与历史操作。
界面区分同版本、等待发行、版本歧义和缺少 OTA 支持。
有可用升级时等待管理员操作，不执行自动升级。
目标不再提供任意历史版本下拉。
未初始化 OTA 的 Emissary 显示“需要安装 OTA 支持”。

## 验证边界

设备终端 Worker 计入 OTA 排空条件。实际 downloading、draining、activating 阶段拒绝新终端并取消已有终端；取消、下载失败与回退后允许显式创建新终端。终端关闭与真实进程回收完成后才计为零；已关闭会话不恢复。见[设备终端](terminals.md)。

`cargo test -p artifactory-emissary --all-targets` 覆盖签名、下载中断、generation、排空和真实子进程经 mTLS 确认的升级 / 回滚。PostgreSQL 测试覆盖节点锁竞争、池限流 / 暂停、权限、过期会话、吊销和证书期限。六平台 CI 覆盖 Darwin、Windows、Linux 各自的 arm64 / amd64，使用 Rust 原生构建；具体通过结果以 CI run 为准。

测试 loopback peer 不替代真实控制平面、对象存储或原生服务安装验收；历史 Go Mac mini 记录不作为 Rust Emissary 的运行证据。各 OS 服务、Xcode / SDK 和正式发行签名需分别验收。
