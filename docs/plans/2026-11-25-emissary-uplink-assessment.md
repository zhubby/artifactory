<!--
[INPUT]: Uplink 固定提交与 Artifactory 0.2.3 源码核查。
[OUTPUT]: 比较协议、Action、上报、探测和 OTA。
[POS]: Emissary 演进计划的调研依据，不代表产品验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Bytebeam Uplink 与 Artifactory Emissary 评估

调研日期：2026-11-25。

本报告保留实施前的比较基线。
后续代码和验证见[本地实施记录](../acceptance/emissary/2026-11-26-evolution.md)。

- Uplink：完整克隆 `https://github.com/bytebeamio/uplink`，固定提交 [`e94a67186d64b3a38233da1afdafc966082f4492`](https://github.com/bytebeamio/uplink/commit/e94a67186d64b3a38233da1afdafc966082f4492)。该提交是本轮 GitHub API 查询的 `main`，对应 v2.18.5；提交及最新正式 Release 日期均为 2025-10-01。
- Emissary：当前工作区，HEAD `5b8d69e02b1db4db858de61eaf6f0c99b8fdbcfc`，产品版本 0.2.3；以本轮工作树为准，保留已有未提交改动。 调研结束时另有外部文档提交将 HEAD 推进至 `16535fdf1f7b2f10baaab36d58fb666d73903118`；已检查本报告涉及的 Emissary、协议和 Telemetry 源码未随该提交变化。
- 方法：对照文档、Rust 实现、配置和测试；分别核查协议/Action、遥测/探测、OTA，并交叉复核关键结论。
- 范围：公开 Uplink 设备端。没有审计 Bytebeam 闭源云平台、运行其远程 Shell/安装器或进行真实 OTA。
- 本轮运行 `cargo test --locked -p artifactory-emissary-protocol`：11 项通过。其余结论为源码及测试代码审阅，不代表本轮端到端或六平台验收。

## 1. 核心判断

**值得借鉴，但应吸收扩展和运维体验，保留 Emissary 的执行与信任边界。**

Uplink 是通用 IoT 通信代理：本地应用提交命名数据流、接收命名 Action；代理承担批量、压缩、缓存、传输和部分内置采集。Emissary 是跨平台构建执行节点：任务授权、租约、进程树、源码与签名材料、制品、终端和自身更新都带业务围栏。

Uplink 的长处是接入简单、数据流策略可配、处理器可组合。Emissary 在构建副作用控制、业务入库确认、设备身份和自身 OTA 恢复方面已经更完整。MQTT 是标准传输协议，Uplink 的 JSON、Action 和 topic 则是 Bytebeam 自定义应用协议；不能把两者混称为标准设备管理协议。

**建议主线：协议演进与关键上报可靠性 → 运维可见性与有限 Action → 按需求扩展采集。** MQTT 替换、万能脚本执行、整机固件 OTA 都没有本轮证据支持其成为当前主线。

| 维度 | Uplink 的参考价值 | Emissary 当前基础 | 建议方向 |
|---|---|---|---|
| 协议规范 | 简单统一的应用信封、接入文档 | v1 Protobuf、共享契约、capabilities、会话围栏 | 补 JSON schema 协商、typed decoder、命令错误语义 |
| Action | 命名处理器、统一回执、进度和阶段组合 | 构建、OTA、终端各有可靠生命周期 | 增加有限运维操作目录，统一查询和审计 |
| 数据上报 | 每流批量/刷新/压缩/优先级/缓存策略 | 独立 mTLS、事务 ACK、幂等、原字节 spool | 先保护清单、补链路指标，再优化流量 |
| 本地探测 | 内置 collector、journald/logcat、本地应用接入 | 主机/工具链/网络/托管进程/GPU/cgroup | 探测结果规范化、受控刷新、可选服务诊断 |
| OTA | 下载进度、空间预检、弱网续传思路 | 强制签名、六平台、排空、激活确认与回滚 | 空间/进度/GC 优先；严格续传随后推进 |

## 2. 规范化协议：保留现有传输，补齐内层演进契约

### Uplink 实现事实

- MQTT 3.1.1 接云端，可选 TLS；事件 topic 含租户、设备、stream，JSON 数组承载批次。
- 数据点采用 `stream + sequence:u32 + timestamp:u64 + flatten(payload)`；序列化时 stream 由 topic 表达。
- Action 是 `action_id/name/payload` 三个字符串，payload 再包含 JSON 文本。
- ActionResponse 统一关联操作 ID、序号、时间、state、progress、errors；state 为自由字符串，终态以 `progress == 100` 判断，失败也填 100。
- 没有在这些应用信封中发现明确 schema_version、能力协商、deadline 或租约。sequence/time 异常只登记，不作为拒收条件。

证据：[Action 类型](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/actions.rs#L10-L83)、[Payload 类型](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/bridge/mod.rs#L23-L33)、[stream 异常记录](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/bridge/stream.rs#L66-L99)。

### Emissary 的真实调整点

Emissary 已有版本化 `artifactory.emissary.v1`、主版本拒绝、mTLS、capabilities、command/request/sequence、attempt/lease。无需重建这些机制。

但 Protobuf 外层里的 `kind/payload_json` 仍需独立约束。尤其 `TelemetryBatch` 只包装 JSON bytes，遥测 Batch/Inventory 使用 `deny_unknown_fields`，没有独立 schema 版本。未来新设备新增字段时，旧服务端会返回 InvalidArgument，而发送端把该批次作为永久拒收删除并计 gap。

这是**新增字段和混合版本部署的演进风险**，不是本轮已复现的线上丢数事件。现有字节与字段号兼容测试保护旧格式，不能代替新设备/旧服务与旧设备/新服务组合验证。

建议：

1. 建立命令目录：名称、输入/结果类型、schema、能力条件、权限、截止时间、幂等与取消政策。
2. 先在共享 Rust crate 内以 typed enum/decoder 收拢字符串边界，保留现有 wire format。后续有跨语言消费方时再交付可生成的 schema/SDK。
3. 遥测服务独立返回 accepted schema；不能只协商 Control，因为 Telemetry 可独立部署。
4. 先部署可读新旧 schema 的接收端，再启用新发送端；新设备遇旧服务降级到双方支持格式，离线旧批次保持原字节重放。
5. 拒收执行请求时返回明确 unsupported/schema_mismatch，区别于可忽略扩展通知。目前 runner 的未知命令默认空响应，不利于诊断。
6. 不直接给旧 JSON 信封塞一个 version 字段：这本身也会被旧端的严格解码拒绝。需要先定义兼容引导和能力门禁。

证据：[遥测模型](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary-protocol/src/telemetry.rs#L16)、[服务端 JSON 解码](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/telemetry-service/src/lib.rs#L89)、[客户端永久拒收分支](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/mod.rs#L291)、[未知命令分支](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/runner.rs#L380)、[兼容测试](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary-protocol/tests/wire_compatibility.rs#L15)。

## 3. Action：借鉴统一操作体验，保留不同执行生命周期

### 值得借鉴的部分

Uplink 的名称→handler 注册、统一状态回执、取消路由和完成后重定向，让新功能容易接入。同一个设备可将 precheck、downloader、installer 组合，外部程序也能实现 handler。

但它的 routing cache 是 ID→handler，容量有限，不保存完整请求摘要、已完成结果或持久去重状态。重复 Action 在完成后可以再次执行；当前活动 ID 的去重不能保证跨重启或终态后幂等。ProcessHandler 还明确忽略 timeout，进程非零退出也不自动生成失败回执。

MQTT QoS 1 只提供 MQTT 投递语义。入站 Action 采用内存 `try_send`，队列满仅日志记录；它不等于操作已持久接受，更不保证副作用只发生一次。

证据：[Action bridge 与缓存](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/bridge/actions_lane.rs#L28-L114)、[路由/取消/重定向](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/bridge/actions_lane.rs#L116-L231)、[进程执行边界](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/process.rs#L52-L124)、[MQTT 入站](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/mqtt/mod.rs#L207-L291)。

### 对 Emissary 的建议

先增加少量内置运维操作，例如 `collect_diagnostics`、`refresh_inventory`、`flush_telemetry`。第一批不开放任意脚本或任意远程可执行文件。

统一操作查询信封可包括：

| 字段 | 作用 |
|---|---|
| operation_id / kind / schema_version | 关联操作与明确解释规则 |
| issued_at / expires_at | 拒绝过期投递 |
| phase / progress / error_code | 状态不依赖百分比推断，错误可分类 |
| request_digest / result_ref | 识别同 ID 异内容，保存有界结果 |
| node/startup/session/generation | 按操作需要绑定实际执行身份 |
| actor / authorization / audit | 谁发起、作用范围、权限复核 |

每类操作明确并发、重放、取消和重启恢复规则。副作用操作先持久接受，再 ACK；终态结果应可重放。不能笼统承诺 exactly-once。

构建继续使用两阶段 start grant 与租约；OTA 继续使用 supervisor/generation/健康确认；终端继续使用独立字节流和短期授权。可以统一展示、查询和审计，执行器无需合并成万能引擎。

证据：[当前协议说明](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/docs/protocol.md#L35)、[设备 runner](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/runner.rs#L290)。

## 4. 数据上报：最值得吸收的是每流策略与链路自观测

### Uplink 的优点和保证边界

每流支持 batch_size、flush_period、Lz4、priority、持久文件大小/数量；serializer 区分 normal、slow、catchup、crash，另可优先发送最新数据。存储、序列化、压缩、MQTT 各有指标。

这能让低频关键状态、普通资源采样和高频应用数据采用不同策略，而不是使用同一个采样频率和缓存预算。值得参考的指标包括队列字节数、最老数据年龄、磁盘占用、丢段、压缩前后大小、连接重试与 ACK。

边界同样明确：默认 max_file_count 为 0，即不启用磁盘持久化；备份文件读入内存时就删除，未等云端业务入库；write 使用 flush 而非 fsync；正常关闭会保存 inflight，但突然断电仍存在内存窗口。不能据此声称零丢失或业务级 exactly-once。latest-first 也不是公平带宽调度保证。

证据：[StreamConfig 与默认值](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/uplink_config.rs#L50-L113)、[最新数据优先](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/serializer/mod.rs#L189-L229)、[持久文件写入与读取删除](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/serializer/storage.rs#L485-L515)、[链路指标](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/base/serializer/metrics.rs#L8-L72)。

### Emissary 已经有的部分

- 控制、日志、Telemetry、Terminal 独立通道。
- 遥测批次 ID 与原字节摘要幂等，数据库事务完成后 ACK，匹配 ACK 才删本地记录。
- 私有原子 spool 默认 32 MiB、8192 条，重启原字节重放，满额淘汰最老遥测并持久计 gap。
- 构建日志独立预算，满额明确失败，不能采用遥测的静默淘汰政策。
- 主机约 30 秒、托管进程约 10 秒采样；上传有超时/退避，失败不传播为构建错误。

证据：[spool](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/spool.rs#L26)、[上传 ACK 与重试](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/mod.rs#L269)、[服务端幂等事务](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/telemetry-service/src/store/ingest.rs#L18)。

### 本轮发现的具体缺口

**A. 清单落盘后被淘汰，当前进程不再重发。**

`pending_inventory` 在成功写入本地 spool 后置空，随后所有记录都遵循 FIFO 淘汰。首批 inventory 未入库就因长时间离线被淘汰时，Telemetry inventory history/latest 会缺失该次启动清单。Control 的启动 metadata 有独立 ACK，不受这条路径直接影响。

建议把清单作为待业务确认的独立耐久记录，并使用受保护配额；收到 ACK 后结束待发。规划阶段进一步确定独立清单 RPC，保留原 startup、摘要和探测时间，不套用采样48小时窗口；旧接收端不支持时明确等待升级。禁止篡改旧批次时间或内容。验收应覆盖离线填满、驱逐、重启、服务恢复与时钟偏差。

证据：[pending 清空](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/mod.rs#L230)、[统一淘汰](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/spool.rs#L79)。这是高置信源码路径，尚未新增运行复现测试。

**B. FIFO 积压会推迟新鲜资源摘要。**

上传一直取最老批次；大量补报时，新 host sample 必须排在历史数据后。现有 latest/fresh 判断不会被旧数据欺骗，但新鲜度可能长时间不可用，影响 AI 选机。建议按清单、最新主机摘要、历史采样分配预算，采用有限比例公平补报；最新状态优先且历史有最小吞吐。保留事务 ACK，不把最新数据直接绕过耐久通道。

**C. 只看 dropped_batches 难以定位原因。**

补 spool_bytes/records/oldest_age、last_successful_ack、upload_latency、retry/backoff、reject_reason、collector_duration/error、quota_eviction/expired/schema_mismatch/disk_error。先收集稳定有界指标，避免 stream/错误文本成为无限 cardinality 标签。

完成 A/C 后再以实际流量决定压缩和批量。保留压缩前大小上限及解压上限；摘要继续绑定规范确定的原始批次字节，不能因重序列化改变离线幂等键。

## 5. 本地探测：从资源清单走向可解释、可授权的诊断

Uplink 的 systemstats 覆盖内存、磁盘、网络、每核 CPU、温度以及配置进程名；journald/logcat 支持日志源过滤；TCP/stdio 让本地业务应用自报数据。这是“采集器 + 接入桥”，不是自动理解所有设备或自动发现任意协议。

Emissary 已有真实工具命令探测、八路并发、单工具三秒超时、输出上限、稳定启动快照、GPU、Linux cgroup、Windows IP Helper、PID+start_time 托管进程归属。不能把基础主机探测列成缺失。

可推进三层：

1. **先规范探测结果。** 统一 available/unsupported/permission_denied/timeout/error、observed_at、duration、probe_version。版本命令失败不再全部折叠成 available=false。保留“存在工具”与“通过原生目标构建验收”的区别。
2. **再做受控刷新和诊断。** 管理员可请求指定工具或白名单 endpoint 的检查；有超时、总预算、脱敏结果和权限。刷新产生 inventory revision，不能偷偷改写 immutable startup 快照；调度使用哪个 revision 必须显式定义，避免设备能力与已生成计划脱节。
3. **有业务需求再加本地扩展。** 首批可选读取 Emissary 服务自身日志与固定健康指标。接入外部应用时优先 Unix socket / Windows named pipe、服务账户 ACL、来源 allowlist、schema、速率与逐记录上限。插件只提交允许的数据，节点身份、attempt 归属、操作回执由 Emissary 控制。

不建议默认扫描全机服务、完整进程命令行、环境变量和无范围日志。Uplink 的 TCP collector 实际绑定 0.0.0.0，未在此边界发现客户端认证，且使用无显式行长上限的 LinesCodec；新连接会替换旧连接。这是接入便捷性与隔离性之间的取舍，不应直接移植。

证据：[Uplink 采集循环](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/systemstats.rs#L444-L627)、[本地 TCP](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/tcpjson.rs#L53-L136)、[Emissary doctor](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/doctor.rs#L11)、[启动快照](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/telemetry/mod.rs#L29)。

Uplink 的 device_shadow 核心目前只周期上报 uplink_version，不能据名称推断它提供通用 desired/reported 配置协调。

## 6. OTA：借鉴下载体验，保持强制信任与激活闭环

### Uplink 有价值的设计

- precondition → download → install 职责分离。
- 下载前检查剩余磁盘；有解压大小时可做安装空间预检。
- 按下载字节计算进度，百分比变化才上报；独立 checksum 验证阶段。
- 网络失败后发 Range 继续下载；优雅关闭时保存任务，启动恢复。

证据：[容量预检](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/preconditions.rs#L62-L94)、[下载与进度](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/downloader.rs#L530-L565)。

### 不能由功能名推断的保证

- checksum 可选，下载器没有 Emissary 这种签名发行清单校验。
- Range 请求后没有验证 206/Content-Range/ETag/If-Range；返回完整 200 时也追加写入。续传思路可借鉴，代码不能直接复制。
- 恢复状态主要在优雅关闭保存，不是持续原子 checkpoint，不能称掉电恢复保证。
- installer 只解包并 spawn 外部 updater，不等待退出、健康确认或自身回滚。
- app/rootfs 回滚位于 Raspberry Pi 示例脚本。rootfs README 明确 kernel 不支持回滚；不等于核心提供跨平台 A/B OTA。
- 下载文件名与权限边界较宽，不能用于 Emissary 可信可执行文件目录。

证据：[Range 重试](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/downloader.rs#L256-L313)、[状态恢复](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/downloader.rs#L479-L519)、[文件路径边界](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/downloader.rs#L414-L459)、[installer](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/uplink/src/collector/installer.rs#L55-L81)、[rootfs 限制](https://github.com/bytebeamio/uplink/blob/e94a67186d64b3a38233da1afdafc966082f4492/docs/examples/rpi/updates/rootfs_update/README.md#L24-L28)。

### Emissary 已经更完整的部分

强制 Ed25519 manifest、六平台/大小/SHA256/协议范围、私有临时文件与 sync_all、固定 supervisor、不覆盖运行服务 EXE、generation、防迟到命令、构建/日志/终端排空、旧运行程序激活确认、候选同 session 三次健康 ACK、120 秒失败回滚、持久恢复。管理员显式发起、统一产品目标、池并发和失败暂停也已经存在。

证据：[下载与原子资产](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/ota.rs#L349)、[持久状态恢复](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/crates/emissary/src/ota.rs#L168)、[当前产品契约](https://github.com/zhubby/artifactory/blob/16535fdf1f7b2f10baaab36d58fb666d73903118/docs/emissary-ota.md#L8)。

建议推进：

1. **空间预检。** 按 OTA 所在文件系统计算新资产、临时文件和安全余量；保留 current/previous/candidate。预检不能保证检查后没有其他写入，写失败仍需安全失败。
2. **进度与阻塞原因。** bytes_done/total、阶段起止、最后进展、等待哪个 worker/log/terminal、错误码；低频有界上报，状态不依赖百分比推断。
3. **版本目录 GC。** 明确 current/previous/candidate/活动下载保护集合，受配额或保留数量约束；禁止先删唯一回滚版本再下载。
4. **严格断点续传。** 部分文件绑定签名 SHA256/大小/platform/generation；验证 206/Content-Range 和对象一致性，200 重下，拒绝错位/越界，最终仍全量验大小和摘要。预签名 URL 过期可重新领取同一冻结资产 URL，不能改目标。
5. 先做运行期网络重试，再决定跨进程 checkpoint。现行“中断失败、管理员显式重试”是产品语义，不能借续传改成自动循环升级。
6. 批次/金丝雀/维护窗口后续建立在管理员明确授权上，继续保留“版本差异不自动升级”。SDK、业务应用、OS 和固定 launcher 自更新另立契约。

## 7. 推进清单与验收出口

| 优先级 | 工作项 | 收益 | 验收出口 |
|---|---|---|---|
| P1 | 清单独立确认与保留 | 修补本轮发现的可靠性缺口 | 离线填满、驱逐、重启后能补交；48h 超窗有明确语义 |
| P1 | JSON schema 协商与兼容矩阵 | 使协议可持续演进 | 新旧 Control/Telemetry/设备组合；旧 spool 原字节重放 |
| P1 | 遥测链路指标与拒收分类 | 可解释断网、背压、失配和磁盘错误 | 断网/限流/磁盘满/时钟错/永久拒收均可辨识 |
| P1 | OTA 空间预检、进度、排空原因 | 提高升级可操作性 | 低空间、无进展、日志积压、终端退出与取消竞态 |
| P2 | 有限运维操作目录 | 远程诊断和刷新有统一契约 | 权限/过期/重复/重启/取消；不削弱既有执行围栏 |
| P2 | 清单/最新状态/历史公平发送 | 积压时维持选机可见性 | 历史有最小吞吐、最新状态有延迟界限、内存与磁盘有上限 |
| P2 | OTA 严格续传与资产 GC | 降低弱网重复流量与长期磁盘占用 | Range 200/206/416、错位、对象变化、URL 过期、回滚资产保护 |
| P3 | 本地应用上报与可选日志源 | 在真实业务需求下扩大观测范围 | IPC 身份、schema、限流、脱敏、来源与租户边界 |
| P3 | 金丝雀活动、SDK/OS/launcher 更新 | 扩大生命周期覆盖 | 独立授权、兼容/恢复方案与各平台真实验收 |

P1 不是“已确认线上紧急事故”的严重性标签，而是建议的实施先后。第一批建议覆盖清单可靠性、协议兼容、链路可见性与 OTA 可见性。后续新增字段必须走该批建立的演进机制。

## 8. 设计自查与复用边界

- 新能力沿用现有共享协议、传输、进程监督和 store 事务；不再添加一套并行的命令执行体系。
- 统一查询和事件信封，保留构建、OTA、终端的授权与生命周期边界。
- 最新状态可优先，审计、终态和构建日志不能套用可淘汰采样的政策。
- 插件和外部 updater 按真实需求引入；没有消费方时不先建设插件市场、通用 DAG 或双协议栈。
- Uplink 是 Apache-2.0。若实际复制代码，按上游 LICENSE 和文件通知保留归属；借鉴模式仍应重新满足 Emissary 的契约与测试。
- Uplink 文档存在漂移：Action ID 文档写 integer 而实现为 String；旧 tunshell 名称和当前 launch_shell 不同；installer 示例与实际 updater 契约不同。评估以固定提交源码为主。
- 本轮没有验证其商业云端策略、性能和大规模设备行为；不根据 README、提交活跃度或测试存在推断生产保证。

## 9. 调研轮验证与复查入口

本节只记录2026-11-25调研轮。
后续实施与验收以[本地实施记录](../acceptance/emissary/2026-11-26-evolution.md)为准。

已执行：固定 clone SHA、GitHub main/latest Release 查询、本地工作树识别、协议/Action/采集/缓存/OTA 源码交叉核查、Emissary protocol 11 项测试。

未执行：Uplink 连接云端、Uplink OTA/installer/rootfs 脚本；Emissary 真实数据库/S3/原生服务/设备升级；清单淘汰场景的新回归测试；混合 schema 版本的端到端验收。

调研保留完整临时克隆。后续规划将本评估归档到仓库；没有修改产品源码、提交、推送或部署。

## 10. 规划阶段的补充结论

实施路线见 [Emissary 演进计划](2026-11-25-001-feat-emissary-evolution-plan.md)。

- 清单采用独立观察回执，保留原探测时间，不靠改时间绕过48小时采样窗。旧接收端不支持时，明确等待兼容端。
- 首批清单刷新只形成观察 revision。调度继续使用不可变启动 metadata；采纳刷新能力需独立设计计划失效。
- OTA增强由固定launcher执行。现有runtime OTA不会替换launcher，因此需要管理员安装迁移与能力读回。
- 有限操作首批为诊断与清单刷新；flush、插件、压缩、升级campaign和OS/SDK更新均设业务准入条件。
