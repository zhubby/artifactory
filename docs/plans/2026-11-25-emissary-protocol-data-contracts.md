<!--
[INPUT]: 当前严格 JSON、mTLS、spool 与调度快照契约。
[OUTPUT]: 固定格式协商、清单确认和有限操作的目标行为。
[POS]: Emissary 演进计划的数据契约与本地实施准则。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary 协议、数据与有限操作契约

状态：本地实施中，完整出口尚未验收。
所属计划见 [Emissary 演进路线](2026-11-25-001-feat-emissary-evolution-plan.md)。
实现与测试证据见[本轮记录](../acceptance/emissary/2026-11-26-evolution.md)。
本文定义目标契约，不能替代平台与部署验收。
2026-11-26用户决定下次部署统一升级所有测试设备。
旧功能回退与新旧混部不再是交付要求。
已实现的旧字节解析与能力降级保留为防御行为。
以下协商仍约束当前独立服务，不授权长期混部。

## 1. 兼容引导

### 1.1 独立 Telemetry 协商

在现有 Telemetry Service 增加 `Negotiate` RPC。
请求列出支持的 batch schema、inventory schema 和编码。
响应返回交集、服务上限、服务器时间和有限 features。
首轮编码只有 `identity`。
请求最多 8 个 schema、8 个 features，总计不超过 4 KiB。
协商使用当前设备 mTLS 身份与相同吊销检查。
服务繁忙、授权库故障和认证失败不能伪装成旧服务。

| 结果 | 新设备行为 |
|---|---|
| RPC `Unimplemented` | 采用精确 legacy batch v1；独立清单等待兼容端 |
| 有交集 | 新观测使用交集内最高且本地已启用的 schema |
| 无交集 | 报 `schema_unavailable`，暂停该类发送，保留有界队列 |
| 超时、Unavailable、限流 | 保持上次安全格式或启动时 v1；重试协商，不启用新格式 |
| 认证/权限失败 | 暂停上传、显示权限原因；保留记录，不降级绕过认证 |

兼容部署的接收端始终保留 batch v1。
新发送格式上限与软件版本分开，初始只启用 v1。
Control 与 Telemetry 分别协商，互不推断能力。
连接重建重新协商；响应不构成永久路由保证。

### 1.2 不改变旧 JSON

`TelemetryBatch.json` 保留 protobuf 字段 1。
新增 protobuf `schema_version`，0 与 1 均表示 legacy v1。
不在 legacy JSON 内加入 schema/version/features 字段。
旧接收端忽略外层新字段，仍能解码 v1 原始字节。
新接收端先分派 schema，再调用该版本的严格解码器。
不支持格式必须在写入前拒绝。

新 ACK 可增加原字节 SHA-256 和已解码 schema。
协商具有 `digest_ack_v1` 时，必须同时匹配 ID、schema、摘要。
旧服务只返回 batch ID 时，沿用既有 ID 匹配语义。
发起请求时固定其所需 ACK 规则；中途换后端不降低规则。
缺失摘要视为兼容性重试，不删除记录。

schema 与原字节一起持久化，不从当前软件版本推断。
旧 `.batch` 无元信息时固定解释为 v1。
新格式记录进入独立版本目录，继续使用同一发送锁。
旧程序可以暂停新目录，不能清理或重写它。
原始 JSON 是摘要输入，HTTP/gRPC 编码不改变它。

负载均衡切到旧副本时，新客户端遇新格式拒收：

1. 保留原记录，标记对应格式等待。
2. 重建连接与协商；新采样降到可用格式。
3. 允许发送其他可用格式，避免队头阻塞。
4. 历史采样超48小时后计 `expired`，按明确策略释放。
5. 独立清单保持待确认，不套采样48小时窗口。

不得将一个 schema 的已持久批次重编码成另一个 schema。
升级后的接收端保留 v1 和已启用格式的解码器。
统一升级保持原持久记录可读，不增加旧功能回退路线。

### 1.3 拒收与重试

新服务在 gRPC status details 中返回有限机器原因。
code 仍保持现有兼容含义；details 不含原始数据与秘密。

| 原因 | 本地处置 |
|---|---|
| `schema_unsupported` / `encoding_unsupported` | 保留、重新协商、该格式独立退避 |
| `overloaded` / `database_unavailable` | 保留、带抖动退避，不记数据缺口 |
| `authentication_failed` / `authorization_denied` | 保留并暂停该身份上传，等待权限恢复或身份删除 |
| `startup_unregistered` | 保留清单，等待Control确认该startup，再补交 |
| `clock_future` | 保留，提示时钟；不得重写时间，历史队列仍受总额限制 |
| `expired_sample` | 历史采样记缺口后删除，不扩散至独立清单 |
| `malformed` / `bounds_exceeded` | 记录 ID、摘要与原因后删除坏记录；不记录正文 |
| `id_content_conflict` | 隔离身份摘要，报警并停止该记录重试，不覆盖服务端内容 |
| `identity_retired` | 按身份清理政策终止重放；节点删除优先于补交 |
| 未识别 details 或无 details 的永久 code | 保留、降速并标记 `unclassified_rejection`；仅在已协商 legacy 的 v1 明确 OutOfRange 且本地年龄也超48小时，才按过期释放 |

隔离记录只保留有界元信息，最多64条/64KiB。
采样正文继续受历史队列额度约束。
受保护清单的格式/权限问题保持可见等待。
没有匹配 ACK 时，不能把拒收显示为“已送达”。

独立清单只对规范`id_content_conflict`持久隔离。
隔离键为`observation_id + delivery_digest`。
源字节、观察时间和启动检查点保持不变。
选择器跳过精确隔离键，继续交付有效刷新。
格式等待计数不包含已隔离记录。
纯隔离队列显示`id_content_conflict`。
有界索引最多64键，统计JSON最多64KiB。
活动源仍存在时，索引不得淘汰它的隔离键。
原清单额度、当前revision 0与最新刷新保护保持有效。
旧历史仍按原额度政策显式替代，不增加永久保留类别。
索引写盘失败显示存储故障，不能合成ACK。
改名成功而目录同步失败时，保留新索引及同步债务。
重开后仍跳过原键，累计冲突不重复增加。
心跳保留活动隔离数、累计冲突数和最近冲突时间。
后续真实ACK不清除这些持久诊断。


### 1.4 命令目录

共享协议 crate 导出静态目录和 typed decoder。
目录记录 kind、输入/结果 schema、能力、权限、截止、去重与取消规则。
首轮把现有字符串边界收拢，不改变构建 wire 编号。
build、OTA、terminal 保留原执行器。
通知与执行请求必须在目录中区分。

设备 hello 声明 `command-catalog-v1`。
Control 仅对该能力发送 `capabilities_ack`，确认可接收的响应种类。
未收到确认时，设备保持旧事件集合。
已确认时，未知执行请求返回 `command_rejected`。
原因限定 `unsupported_kind`、`schema_mismatch`、`invalid_payload`。
capabilities_ack同时携带服务器时间，供有限操作建立保守截止。
未知可忽略通知仅增加计数，不产生执行结果。
日志不得输出完整 payload。
Control 不向缺少目标能力的旧设备发送新操作。

## 2. 清单的持久交付

### 2.1 独立观察与回执

增加 `UploadInventory`，与 batch `Upload` 同属 Telemetry mTLS 服务。
使用独立 inventory schema 1；JSON 内容沿用原 Inventory v1。
第一轮不修改 Inventory 的已有字段。
观察信封固定以下含义：

| 字段 | 含义 |
|---|---|
| `observation_id` | 持久 UUID，重试保持不变 |
| `source_startup_id` | 实际采集进程；历史补交不改为当前进程 |
| `revision` | startup 内从0开始的递增观察号；0为启动清单 |
| `observed_at` / `monotonic_ms` | 实际探测时间；重连与补交不更新 |
| `inventory_schema` / `inventory_json` | 严格版本与不可变内容，单记录最多256KiB |
| `delivery_digest` | 覆盖身份字段、时间、schema及原 JSON 的共享确定性编码摘要 |
| `source_batch_id` | 可选，仅用于旧 spool 提取的来源追踪 |

摘要编码由共享 crate 唯一实现并固定 golden fixture。
摘要使用字段长度前缀，不能用歧义字符串拼接。
内容版本摘要沿用现有 Inventory 规范化序列化规则。
交付摘要与内容摘要不能混称。
同 observation ID 异摘要返回冲突，不覆盖。
ACK 必须回显 observation ID 与 delivery_digest。
数据库事务完成后才返回 ACK。

新表保存交付回执、startup/revision 和真实观测时间。
`inventory_versions` 继续按内容摘要去重。
新观察表不复用旧 `(node_id,sampled_at)` 唯一键。
同毫秒观察仍有独立身份。
新查询按当前 startup 读取最新 revision。
旧 schema 记录仍从旧投影读取，并标记 provenance 为 legacy。
旧端与新端不会争写同一个“当前”投影。

### 2.2 时间、历史与调度

独立清单不受普通采样48小时限制。
`received_at` 是交付时间，不是新的探测时间。
未来超过5分钟的观察拒收并保留等待时钟修正。
历史记录保留180天，按接收时间清扫。
幂等回执至少覆盖同一保留周期。
当前摘要的保留与历史清扫分开。

历史 startup 的清单允许补交。
其身份必须属于当前认证节点。
新RPC通过业务只读连接查询 `node_startups` 的节点归属。
未登记startup返回可重试原因；不得静默接受为当前startup。
只读授权脚本追加该表SELECT，不授予写权限。
退役 startup 不得恢复 Control 所有权或执行权限。
Control 查询带当前 startup，Telemetry 按该键返回观察。
迟到旧 startup 不得覆盖当前视图。
业务库和遥测库不使用跨库写事务。
节点生命周期沿用 tombstone 与遥测清扫锁，并扩展到新增表。
新增RPC共用64路全局接收上限，每节点最多一个清单写入。
清单准入初始为每节点每分钟2条、burst为4，超限保留并退避。

启动 metadata 是调度依据，仍不可变。
刷新清单是观察，不写 `nodes.metadata` 或 capabilities。
首批 UI 明确区分“启动调度快照”和“最近探测”。
AI 工具与已固定计划继续使用启动 metadata_digest。
若以后采纳刷新结果，需独立引入能力 revision 与计划失效契约。

### 2.3 本地额度与原子顺序

32MiB定义为持久记录正文的逻辑额度，不含临时峰值。
从中保留最多4MiB、16条独立清单记录。
当前startup的启动清单revision 0与最新未确认刷新各保护一条。
两者相同时只保存一条，普通采样不得淘汰任一保护记录。
普通历史采样可用剩余额度，不能借用清单保留额。
自定义总额至少8MiB，清单保留额固定4MiB。
不满足的新配置拒绝启动遥测，构建继续并报告配置错误。
实际文件、临时写入和索引均纳入磁盘占用观测。
32MiB之外预留1MiB原子写临时预算及64KiB有界索引预算。
文件系统块对齐开销单独观测，不宣称逻辑额度等于物理占用。
旧队列已满时，先保护含清单记录，再逐次淘汰普通历史。
迁移无法取得保护记录空间时停止采样入队，不能先删源清单。

顺序固定为：持久化清单意图 → 生成交付记录 → 发出 → 匹配 ACK → 持久删除。
崩溃发生在任一步，重开都能定位待确认身份。
采样写入成功不能清除清单意图。
清单 I/O 失败不终止构建，但必须显式报告。
同一 startup 的并发刷新由本地观察锁分配 revision。

超过16条/4MiB时，先移除已确认记录。
仍不足时，从最旧历史 startup 的待确认记录开始释放。
仅剩当前startup时，旧的待确认刷新可被更新revision取代。
revision 0不能由刷新取代；只有startup变化后才进入历史额度政策。
每次都持久记录 `inventory_superseded` 的 ID、数量与原因。
当前启动清单与最新刷新不得因普通采样或历史清单被淘汰。
此处保证当前启动与最新观察交付，不保证无限启动历史零丢失。
写入新记录与移除旧记录必须保持至少一个有效当前副本。

旧 `.batch` 迁移先读原字节，提取其清单到受保护记录。
提取身份由来源 batch ID 固定生成，重跑迁移保持一致。
同startup的多份legacy清单按来源batch确定序号，不能占用新刷新revision。
接收表对legacy来源使用独立命名空间，查询按observed_at展示。
提取成功前，不能因额度清扫删除该含清单批次。
旧批次继续整体重放，不更改原字节与摘要。
旧批次匹配 ACK 可结束其 legacy 交付等待。
独立清单投影可在新接收端就绪后以同来源身份补齐。
该补齐是投影回填，不能生成新的探测时间。
无法解码的旧记录保留并报告，不做猜测提取。

新设备遇旧服务时，48小时内可通过旧 batch 投递清单。
若超窗仍未确认，显示 `waiting_inventory_receiver`。
保留独立清单，等待支持新 RPC 的接收端。
禁止把旧内容包装成当前时间的“新清单”。

## 3. 上报与链路可见性

每类队列独立记录 bytes、records、oldest_age、last_ack。
重试还记录当前 reason、backoff 和最后进展时间。
collector 只使用固定名称与有限错误枚举。
资源缺失显示 unknown，不合成零或假进度。
计数器区分 quota_eviction、expired、coalesced、malformed、schema_wait、disk_error。
每个缺口均有原因，不能全部折叠到 dropped_batches。
兼容 v1 的聚合 dropped_batches只计批次丢失，仍保持单调。
coalesced和清单superseded使用独立计数，不伪造批次丢失。

32MiB正文额度分为4MiB清单、1MiB最新主机、27MiB历史。
8192条上限覆盖全部普通主机/历史记录，不含最多16条清单。
新鲜主机最多一个待发送记录与一个发送中记录。
两者合计不得超过1MiB；编码时每份最多512KiB。
主机字段超过上限按固定字段预算截断并报omitted，不能生成无效JSON。
一旦进入请求，原字节固定至 ACK 或明确缺口。
新主机观测可替换尚未进入请求的槽，增加 coalesced。
托管进程历史不因该替换被丢弃。
满8192条时，等量替换不增加记录名额。
只有创建新待发槽时才释放一个历史名额。
发送中记录固定到请求完成，采样淘汰器不能删除或替换它。
ACK按不可变记录ID和摘要删除，不能依赖“当前队首”位置。
队列调度在成功、永久拒收或一次超时后释放类别机会。
清单失败有独立退避；不能连续占满上传器。
历史至少每两个普通类别机会获得一次发送机会。
不承诺不同设备之间的带宽公平。

新鲜主机数据只更新对应 startup 的资源摘要。
当前查询不得从旧 startup 的较大墙钟时间选择资源。
fresh 继续要求 sampled_at 与 received_at 都满足时效。
后台上报不触发前台加载进度。

## 4. 有限操作

### 4.1 首批目录与权限

| 操作 | 输入 | 结果 | 并发与预算 |
|---|---|---|---|
| `collect_diagnostics` | 固定探测 ID 子集；空集合表示默认安全集合 | 有界状态、版本与耗时 | 每节点一个操作；总30秒，单探测3秒、最多8并发 |
| `refresh_inventory` | 只接受固定 full 模式 | 新 observation ID/revision 与交付状态 | 共用每节点操作锁；总30秒；不能修改调度快照 |

两种操作都不接受 Shell、任意 executable 或任意 URL。
首批不探测外部 endpoint，不收集原始系统日志。
探测目录最多32个固定 ID，输入最大8KiB。
超出预算返回部分结果与原因，不能静默遗漏。

HTTP 位于 `/api/v1`：

- `POST /nodes/{id}/operations`：kind、schema、input、客户端 request_id。
- `GET /nodes/{id}/operations`：游标分页，默认20、最多100。
- `GET /node-operations/{id}`：状态与尚未过期的结果。
- `POST /node-operations/{id}/cancel`：仅请求取消，不伪造已取消终态。

创建/取消要求当前租户 owner/admin 登录 Session、CSRF 和 Origin。
读取也要求当前 owner/admin，不能通过项目设备可见性取得诊断结果。
项目 Token、API Token 和普通成员均无权限。
在线、未吊销、可写租户、有效证书、当前 startup 与能力均需复验。
离线不排队，缺少能力返回明确 unsupported。
分发前复核权限；撤权后不再新执行或读取结果。
已通过准入的只读探测在30秒预算内结束，不承诺瞬时撤销。
OTA 排空期间拒绝新操作，取消在途探测并计入排空条件。

### 4.2 身份、去重与截止

信封包含 operation_id、kind、schema、input_digest、issued_at、expires_at。
还绑定 tenant/node、startup、当前 Control session 和操作者引用。
Control 持久保存 actor；设备不信任 payload 自报的租户或权限。
创建幂等键为 tenant/node/request_id。
同键同摘要返回原操作，同键异摘要返回冲突。
服务端创建幂等窗口与操作元数据一致，为90天。

接收截止为服务器发起后30秒。
设备使用当前认证 Control 的服务器时间建立单调截止。
时间基准由新能力握手提供，扣除网络往返和本地经过时间。
剩余接收窗口最多30秒；无法建立保守剩余窗口时拒绝执行。
时间偏差不可校准时返回 clock_unsynchronized。
接收成功后的执行截止为本地单调30秒。
过期新投递不执行；已接受重复投递重放原状态。
相同 startup 断线后，只有新 Control session 的重新授权通知可续传状态。
重新授权不重启探测，也不延长截止。
startup 变化使旧操作 interrupted，不能换进程重新执行。

先持久 accepted 日志，再 ACK，再启动探测。
本地日志无法持久化时返回 storage_unavailable，不执行。
状态为 requested → accepted → running → terminal。
terminal 为 succeeded/failed/canceled/expired/interrupted/rejected。
结果全成功才 succeeded；部分失败使用 failed 和逐项原因。
progress 带单调 sequence；终态与取消通过同一锁排序。
完成已持久化时，迟到取消返回原终态。
取消先提交时，清理子进程后才报告 canceled。

进程重启读取 accepted/running 项，报告 interrupted。
不得自动重跑，因为结果采样时刻已变化。
同进程网络中断继续到预算截止，持久结果等待重连确认。
Control 崩溃依靠现有持久命令重发与 reconciliation。
Control 不根据传输 ACK 推断业务接受。
断线且无结果时，界面显示 awaiting_result，不推断执行成功或已取消。
执行截止后5分钟仍无结果，Control释放准入锁并标记 outcome_unknown。
这是交付状态，不是设备执行终态；迟到终态仍按ID和摘要入库。
设备本地操作锁和30秒截止仍阻止并发探测。
操作终态和结果提交在一个 Business 事务内。
设备收到匹配 operation_id/result_digest 的 result_ack 后才释放结果正文。

### 4.3 结果、保留与探测含义

单次结果最多64KiB；不包含 bundle、私钥、环境或完整命令行。
输出先按白名单提取，再脱敏，再按预算截断。
截断包含 `truncated` 与 omitted 数量。
本地 journal 最多64项/8MiB，保留摘要最多7天。
ACK 后可清除结果正文，保留去重摘要与终态。
没有可安全释放的空间时拒绝新操作，不能淘汰未确认在途结果。
未确认终态超过7天时释放正文，保留有界过期计数。
下次可连接时上报 result_expired，不重跑原操作。
Control已有终态时保持终态，未见终态时保持outcome_unknown。
服务端结果正文24小时过期，操作/审计元数据90天。
节点/租户删除同时清扫结果并终止待发命令。

探测结果包括 status、observed_at、duration_ms、probe_version。
首批以operations schema 1的ProbeReport携带这些字段。
doctor内部采用同一类型，现有Inventory v1仍只投影available/version。
不得把ProbeReport字段直接加进旧Inventory JSON。
完整探测结果进入操作结果，不改变旧批次解码规则。
status 固定为 available/unsupported/permission_denied/timeout/error。
缺少二进制使用 unsupported，非零版本命令使用 error并保留有限错误码。
未运行项单独列 skipped/budget_exhausted，不冒充 unsupported。
成功表示该探测命令成功，不证明对应原生构建可用。
unsupported属于已完成探测，不是操作失败。
permission_denied、timeout、error或budget_exhausted使操作返回部分失败。
刷新失败不覆盖已有成功清单；部分结果属于操作诊断。
完整刷新成功才生成新的 inventory revision。
观察已落盘、Telemetry 未确认时，操作可成功但 delivery 状态为 pending。
成功含义是采集并耐久保存，不能显示为已入库。

## 5. 界面、风险和验收

节点详情运维区先展示当前状态和最近失败原因。
其次展示上报积压、OTA 进度与有限操作历史。
普通成员保留既有设备视图，不展示诊断操作与结果。
旧设备显示“此版本不支持”，不显示空按钮或假零值。
结果过期显示状态和过期时间，保留再次发起入口。
新操作需明确显示探测范围与30秒预算。
网络提交结果不明时，用 request_id 查询，不再次创建。
刷新结果与启动调度快照同时标注时间和用途。

首读骨架、前台顶部进度和顶部 notify 沿用既有规范。
后台轮询不重置草稿、焦点或展开项。
取消期间禁用重复提交，终态后依据服务端结果更新。
手机采用单列和字段换行；320px宽度无横向溢出。
按钮可键盘访问，状态使用文本和 aria-live，不能仅依靠颜色。
新文案同时提供中英文；弹层使用现有自定义组件。

三个主要滥用路径与约束：

1. 借诊断执行任意命令：固定探测目录、typed input、无 Shell输入。
2. 借补交复活旧节点或污染当前资源：mTLS、tombstone、startup分区与当前查询键。
3. 借结果和指标泄漏秘密或压垮服务：白名单、预算、有限标签与当前权限复核。

验收按主计划 U1/U2/U3/U5/U6 的测试路径执行。
需要真实数据库事务、mTLS、断网、磁盘故障和浏览器证据。
本文件的确定性契约不能代替这些运行证据。
