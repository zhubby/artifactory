<!--
[INPUT]: 现有签名发行、固定 supervisor 和管理员 OTA 契约。
[OUTPUT]: 定义空间、进度、GC、续传和 launcher 迁移规则。
[POS]: Emissary 演进计划的 OTA 契约；尚未实现。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary OTA 与固定启动器演进契约

状态：设计目标，尚未实现。
所属计划见 [Emissary 演进路线](2026-11-25-001-feat-emissary-evolution-plan.md)。
当前已实现行为见 [OTA 手册](../emissary-ota.md)。

## 1. 保留的授权与恢复规则

管理员明确发起一次更新，目标固定为当前统一产品版本。
版本差异、重连、发行导入和池策略不创建更新。
同一设备最多一个未完成更新。
继续使用签名 manifest、平台/大小/SHA-256 校验与 generation。
继续等待构建、日志、终端与新增有限探测完全排空。
继续要求旧运行程序激活确认。
候选必须以原证书、目标版本和同 session 连续三次心跳确认。
120秒健康窗口失败仍回滚旧运行版本。
新下载体验不能削弱任一条件。

下载或排空期间进程重启，仍终态 failed。
激活期间重启，仍恢复旧运行版本。
管理员显式重试产生新 update ID 与 generation。
本阶段不增加跨进程自动续传或自动循环升级。
操作进度不是执行授权，也不能替代持久状态机。

## 2. 固定 launcher 是独立交付前提

`crates/emissary/src/ota.rs` 的 supervisor 执行下载和切换。
服务安装路径的固定 EXE 不随 runtime OTA 替换。
因此仅发布新 runtime，不能让旧 launcher 获得新下载器。

新增本地能力握手，明确提供以下 capability：

- `ota-progress-v1`：真实字节和阶段进展。
- `ota-space-v1`：目标文件系统预检。
- `ota-storage-gc-v1`：受保护集合清扫。
- `ota-range-v1`：单次执行内严格续传。

runtime 报告自己的版本、launcher 版本及握手能力。
不能仅根据 runtime 版本或版本字符串推断能力。
未支持握手的 launcher 按 legacy 处理。
旧 launcher 继续既有签名、排空与回滚语义。
UI 显示哪些增强能力需要 launcher 维护升级。
缺少新能力时，不能发送只有新 launcher 能理解的参数。

### 2.1 管理员安装迁移

迁移使用平台安装器，不复用 runtime OTA 请求。
管理员取得固定签名发行中的同平台资产并验签、验摘要。
安装器先读取服务配置、账户、启动模式和当前状态。
必须没有未完成 OTA、构建、终端或有限操作。
未确认日志或受保护清单必须保留，不能作为清理对象。

维护步骤的事务边界：

1. 保存固定 EXE、服务定义、ACL 和状态备份。
2. 正常停止服务，等待 runtime 和子进程树退出。
3. 确认 Windows EXE 句柄释放后，原子替换固定 EXE。
4. 保持配置路径、账户、UUID、证书、版本目录和 spool。
5. 启动服务，读回 launcher 能力与原身份。
6. 验证心跳、遥测、构建和一次显式 OTA 后再清理备份。

步骤2无法干净停止时，退出并保留备份，不强行覆盖。
替换前中断保持旧 EXE；替换后启动失败恢复原 EXE和服务定义。
恢复不得改写 current/previous 指向或更换注册身份。
旧服务原来由专用普通账户运行时，迁移后保持该账户。
安装器提权仅用于原生服务和权限配置，不让 runtime 提权。
Windows x64、macOS arm64、Linux arm64 分别有真实验收。
其余平台单列证据，不以 ABI 或交叉编译推断通过。

### 2.2 状态兼容

保留既有基础 phase 集合与 Image 字段。
扩展状态放入有版本的可选结构，不改变基础恢复判断。
新 launcher 必须读取0.2.3已有 `state.json` 和邮箱。
新 runtime 必须接受旧 launcher 缺少扩展字段。
新状态写出前验证旧 launcher 的反序列化和恢复测试。
旧 launcher 无法安全读取时，维护回退必须先导出兼容状态。
只允许空闲终态执行该导出；激活中不能转换状态。
新文件保留私有权限，使用 fsync 与原子替换。

## 3. 下载空间与资产重用

先验签并冻结目标资产，随后检查实际 `ota/versions` 所在文件系统。
不能使用构建目录或系统根盘余量代替。
当前资产与回滚资产已占空间，不从 free space 中再次扣除。
优先校验同摘要完整目标；可用时直接重用，不重复下载。

目标文件与下载临时文件位于同一文件系统。
完成后原子 rename，不创建第二份完整二进制。
资产上限继续为128MiB。
预检所需新增可用空间为：

`remaining_bytes + max(64MiB, ceil(asset_size × 10%))`

首次下载 remaining_bytes 等于签名大小。
续传时等于签名大小减已保留字节数。
64MiB是初始安全余量，覆盖状态与文件系统开销。
这不是整个主机磁盘空间的独占预留。
检查后其他进程仍可耗尽空间；每次写失败必须安全失败。
实现使用有检查的整数运算，拒绝溢出和负余量。

U4没有GC能力时，空间不足直接安全失败。
U7具备GC能力后，空间不足先执行受保护GC，再重新读取余量。
仍不足则返回 `insufficient_space` 与 needed/free 值。
GC失败或配额不足均不触发删除 current/previous。
未知文件系统余量时返回 `space_check_unavailable`。
首版不提供跳过校验的远程参数。

## 4. 真实进度与排空原因

进度绑定 update ID、generation、asset digest 和单调 sequence。
基础 phase 继续由现有持久状态机决定。
可选 subphase 包括 space_check、download、verify、drain、health。
bytes_done 只能表示已写入的资产字节，total来自签名大小。
传输完成不等于验签完成，验签完成不等于健康成功。
Range 从头重下时增加 download_attempt，允许 bytes_done 重置。
UI 不跨 attempt 平滑伪造递增百分比。

变化进度最多每2秒上报一次。
阶段、错误与终态立即上报；无变化每30秒保活一次。
每个进度信封最多4KiB；等待对象最多8个，余下只报数量。
进度事件可合并，终态与现有 OTA 状态不得合并丢弃。
持久状态最多每5秒或每4MiB进展保存一次，阶段与终态立即保存。
该落盘只恢复可见状态，不授权跨进程续传。

排空分别报告 active_workers、pending_log_records/bytes、terminals、operations。
可见 ID 仅限当前租户管理员；不包含命令、日志正文或下载 URL。
终态未确认与日志未确认使用不同 reason。
等待90秒无进展显示 stalled，保留最后进展时间。
stalled 是诊断状态，不能自动失败构建、删除日志或强制激活。
下载仍有原有总超时；排空不因 UI 阈值新增强制超时。

Control 支持 `emissary-ota-observability-v2` 后才接收扩展事件。
旧 Control 保持原事件集合；runtime仍可本地诊断。
Control 对 update/generation/sequence 校验并拒绝迟到进度。
API和浏览器不把未支持/未知进度解释为0%。

## 5. 版本目录 GC

GC仅遍历配置 OTA 根目录下受管资产目录。
保护集合包含：current、previous、candidate、活动下载和未完成迁移备份。
只要任一持久状态或活动句柄引用，资产即受保护。
进入激活前重新读取保护集合；与激活共用 launcher 存储锁。
运行程序和本地插件不能要求删除任意路径。

初始清扫规则：

- 保留全部受保护资产。
- 额外保留最近2个已验证完整资产。
- 其余未引用完整资产按最旧使用时间清扫。
- 非活动临时文件超过24小时后清扫。
- 预检空间不足时允许提前清扫未保护临时文件及额外缓存。
- 保护集合之外的缓存总额初始为512MiB。

保护集合超过512MiB时保留并报告，不破坏回滚。
必须遍历真实文件，拒绝 symlink、Windows reparse point 和根外路径。
摘要目录名不是安全证明，仍检查文件类型与目录归属。
GC采用重命名到受限 trash 后删除，目录更新同步落盘。
重启只继续删除已入trash且不再受保护的条目。
移入trash前再次在同一锁内复核保护状态。
Windows持有句柄导致删除失败时记录原因，下一次重试。
未知文件不执行递归删除，报告人工检查。

## 6. 严格运行期续传

续传只延续同一次管理员授权下载。
绝对下载期限从首次请求开始为300秒。
最多3次网络重试、一次从头重下、一次 URL刷新。
所有等待与刷新均计入期限；取消立即停止后续请求。
部分文件绑定 node/update/generation/platform/manifest digest/asset digest/size。
路径由本地摘要生成，不从 URL 文件名生成。
新 generation 不继承旧任务的部分文件。
进程重启后的部分文件只供 GC清扫，不自动执行。

请求显式使用 `Accept-Encoding: identity`。
有 N 字节时发送 `Range: bytes=N-`。
若已有 strong ETag，附带 If-Range，弱 ETag不用于对象保证。
缺少 ETag不放松签名大小和最终摘要校验。

| HTTP结果 | 处理 |
|---|---|
| `206` | 必须是单段 `bytes N-(S-1)/S`；起点、终点、总长均匹配签名；实际body长度也必须匹配 |
| `200` | 不追加；清空临时内容，使用这次完整响应从0校验写入，计入一次重下预算 |
| `416` 且 N等于S | 重新读取全文件校验大小/摘要；匹配才完成 |
| `416` 且 N不等于S | 释放当前部分内容，预算内从0重下；不能当成功 |
| 非identity Content-Encoding、multipart、错位或越界 | 协议错误，停止；不把错误内容写入可激活资产 |
| 403/明确票据过期 | 仅通过认证控制通道刷新同一冻结资产URL；一次失败即结束 |
| 其他4xx | 终态下载失败；不无限重试 |
| 断流/超时/5xx | 预算内退避重试，保留当前部分字节 |

S为签名资产大小，不信任响应单独声明的大小。
字节N只证明本地长度，最终仍从头计算全文件SHA-256。
完整大小与摘要都匹配后 fsync，再原子安装到摘要路径。
Content-Length存在时必须与预期body一致。
chunked响应按实际总字节限制，不允许越界写入。
不接受服务器重定向，沿用当前受限URL规则。

URL刷新通过 runtime转发给Control，launcher不持有平台登录凭据。
新RPC绑定节点证书、update ID、generation和冻结发行/资产摘要。
RPC位于现有Control mTLS服务，不暴露浏览器任意URL签发接口。
Control复验当前执行未取消、未终态、平台一致且未吊销。
返回的URL可变，签名manifest、目标版本、资产摘要与大小不能变化。
请求和回执写入私有有界邮箱，迟到/错代响应丢弃。
运行程序或Control不可用时，下载预算到期失败，保留旧版本。
票据、URL查询参数与认证材料不进入日志、进度或审计正文。

## 7. 验收与回退出口

U4先交付可见性、空间预检与launcher维护迁移。
U7随后交付GC与运行期续传。
旧launcher与新runtime组合必须仍可完成既有OTA。
新launcher与旧runtime组合只启用双方支持能力。
混合Control时扩展进度缺失不能破坏更新状态机。

实机验证包括低空间、只读目录、断网、服务重启、候选失败与取消竞态。
每次保留UUID/证书、current/previous、日志与清单队列读回。
不同挂载点、Windows句柄和目录权限分别验证。
新launcher回退前先停止新能力准入，再按状态兼容规则恢复。
没有实际launcher迁移证据时，只能报告runtime已更新。
不能报告空间、GC或续传已覆盖旧安装设备。
