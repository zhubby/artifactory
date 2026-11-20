<!--
[INPUT]: Emissary 公共协议、设备执行与规划协调器身份。
[OUTPUT]: 定义设备会话、心跳预算、执行租约与临时源码边界。
[POS]: 设备通信入口；规划 Pod 的恢复细节由规划协调器协议定义。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary 协议与状态机

公共契约位于 `proto/artifactory/emissary/v1/emissary.proto`，包名为 `artifactory.emissary.v1`。协议主版本为 1；不兼容的主版本在注册和 hello 时拒绝。控制平面和设备共用 Rust `emissary-protocol` crate，构建时由 Tonic/Prost 生成代码。设备不再提供旧 Go 协议或名称别名。

同一 crate 的 `signing`、`telemetry` 和 `nodes` 模块分别定义签名材料、遥测记录与节点资源契约。遥测模型合并只调整 Rust 导入路径，保留设备 RPC、JSON 字段和元数据摘要。

## 连接与身份

1. 管理员为指定节点池创建 15 分钟、一次性注册令牌。
2. Emissary 本地产生 ECDSA P-256 私钥与 CSR，通过钉住 CA 的 TLS 调用 `Enrollment.Enroll`。
3. 控制平面验证 CSR，签发 30 天节点证书；仅保存证书摘要与到期时间。节点私钥不离开本机。
4. 所有 `Control` RPC 校验客户端证书链和数据库登记的证书摘要，并检查吊销 / 到期状态。TLS 层允许无证书连接只为注册；这不意味着 Control 可匿名调用。
5. `Connect` 为双向流。hello 绑定节点 ID、协议版本和运行诊断；新版节点另提交进程启动时采集的 `startup` 快照，内含 `startup_id`、metadata 和 capabilities。新连接会替代旧 session；旧 session 不能继续改写任务或 metadata。
6. 每 10 秒心跳。35 秒未收到消息关闭连接；是否重派由 90 秒持久化任务租约决定，而不是简单的 online 标志。

节点证书到期前需要排空、吊销并以新配置重新注册；本版没有自动续证；Emissary 自动升级见 [OTA 协议与运维](emissary-ota.md)。吊销不可反向恢复，防止旧私钥重新有效。

启动快照编码上限 64 KiB，控制平面提交后返回 `metadata_ack`，以 `request_id` 回显启动 ID。ACK 丢失时 `metadata` 事件重发同一快照；普通重连不重新探测，已退役 startup 和旧 session 的提交被拒绝。新进程快照完整替换旧值；管理员 labels、签名权限和 slots 独立维护。普通心跳只保留运行信息，详见 [节点元数据](node-placement.md)。

完整 hello 和 heartbeat 的 `payload_json` 上限为 96 KiB。协议共享此预算：启动快照 64 KiB、网络诊断 8 KiB、运行信封 24 KiB。运行信封包含最多 32 项活动任务、OTA、指标和 JSON 字段开销。旧 72 KiB 事件加完整网络诊断和字段开销仍在新上限内；超过完整预算的事件在 JSON 解码前拒绝。独立启动快照上限不变。

## 两阶段开始与租约

```text
job: queued → leased → running → succeeded / failed / canceled / lost
                │          │
                └─租约过期─┘
           未开始: queued   已开始: lost

center: persist leased → assign
emissary: assign 去重 → accept
center: persist running → start grant
worker: 收到 start grant 后才能执行
```

任务身份为 `attempt_id + lease_token`。命令有独立 `command_id`，续租 / start grant 的 `request_id` 回显 Emissary 请求编号。Emissary 记录请求发出时的单调时间，以该锚点计算本地截止期限，网络延迟不能延长服务端租约。

执行开始前过期，旧 attempt 记为 lost、job 可重新排队；执行开始后过期，job 为 lost，必须显式重试生成新 attempt。控制平面重启保留 PostgreSQL 内的在途租约，不立即重派。心跳只续有效且未取消任务的租约，不能复活旧 attempt。

任务事件等待 attempt / 节点行锁后，在同一事务连接上重新检查当前租约与构建取消标记。等待期间已提交的取消阻止新的 start 授权；finished 也按当前取消状态登记为 canceled。

Emissary 在租约内断线保持 Worker，重连后补报；无续租则本地终止进程树。父 Emissary 退出 / 崩溃关闭 Worker 控制管道，Worker 终止正在运行的命令。Unix 使用进程组，Windows 在恢复挂起的初始进程前分配 Job Object，关闭 Job 时杀死所有后代。

`started` 的保守定义是控制平面已提交 running 并准备发送 start；即使随后响应丢失，也不假定副作用未发生，因此不自动重跑。

## 事件与日志

- `accept` 可重放；同 attempt 不重复创建 Worker。运行事件从 1 单调递增，重复事件只返回已提交游标，不重复写状态；跳号、终态后的新事件和过期租约均拒绝。
- 日志使用独立 gRPC 连接及 `UploadLogs` 流，不与心跳串行传输。每批最多 64 块，每块不超过 64 KiB；按 attempt 维护连续序号。相同序号不同摘要拒绝。
- S3 日志写入发生在租约行锁之外，对象 key 绑定内容摘要；短事务在网络完成后重新检查有效租约、序号与摘要，再登记日志索引。存储等待不会阻塞同一 attempt 的心跳续租，冲突上传不会覆盖已接受内容；过期或冲突对象不进入有效日志索引，可能留下私有孤立对象，由人工保留策略处理。
- Emissary 默认磁盘日志预算 256 MiB；满额或写失败会中止并明确报告失败，不静默丢弃。旧日志留存在本机，计入下次启动的总预算。
- finished 通常等到日志得到确认才上报。最终日志阻塞 120 秒会改为显式失败，未确认日志保留本机。取消后的剩余日志在原有效租约内仍可上传，制品则不再接受。
- 浏览器 SSE 以数据库日志 ID 作为恢复游标；55 秒轮换连接并重新鉴权，`Last-Event-ID` 支持续传。返回 `no-transform`，Next 关闭内部压缩，反向代理须禁用 SSE 缓冲。

## 制品传输

`PrepareArtifact` 声明相对路径、大小和 SHA-256，获得 15 分钟 HTTPS staging PUT URL。`CompleteArtifact` 获取对象，验证完整内容，再写入从不签发 PUT URL 的 final key；成功后登记制品。上传声明和制品登记各自在事务提交前获得 attempt / 节点共享锁，并重新检查当前租约、节点证书期限、吊销和构建取消状态；锁等待期间失效则回滚本次记录。完成接口在 S3 校验结束后才取得这些锁，保留独立心跳续租。重复完成返回相同制品，仍需当前授权有效。下载经 HTTP 项目权限校验，签发 60 秒 GET URL，不能匿名访问桶。

对象传输不使用双向控制流。未来容器执行器可以复用 attempt / lease / log / artifact 契约，但不改变制品为文件还是 OCI 镜像的独立建模。

## Emissary 级 OTA 扩展

协议主版本仍为 1。启动快照的 capabilities（旧节点为 hello 的 capabilities）包含 `emissary-ota-v1` 才调度升级。心跳的独立 `ota` 对象报告持久化阶段；`emissary_update` 命令使用 Command.payload_json（新增字段 10），没有 attempt_id / lease_token；`emissary_health` 引用当前心跳 command_id。所有状态及确认继续验证节点 mTLS 身份、当前 session、吊销和证书期限。升级确认没有收到时不提交候选版本。

## AI 与临时源码

AI 规划由 Operator 直接创建 Kubernetes Pi Pod。规划协调器提供模型代理，保存累计调用预算、用量、截止时间、proposal 和分代会话。Worker 丢失后撤销旧授权；旧执行完成停机清理后，Operator 才创建新 generation 的 Pod。恢复不回放旧 checkpoint，不重置预算。协议细节见 [运行协议](planning-protocol.md)。

Assignment 使用 `source_manifest_json` 传递固定元数据，包含仓库路径、上游 URL、commit、标签与 LFS OID。凭据不进入 Assignment。原生 Emissary 经 mTLS 调用 `Control.AcquireSourceMaterial`，请求包含 attempt、租约令牌和 Manifest digest；服务端验证节点身份、任务当前授权、租约、计划版本和源码清单。

每个 attempt 直接从上游拉取到独立临时目录。Kubernetes 任务使用 init container、投影令牌和 `emptyDir`。控制平面不代理 Git/LFS 流量，不保存源码包或缓存。只有准备进程领取源码凭据，任务退出后清理源码和临时认证材料。

计划提交、澄清、取消和过期回收通过事务排序。重发相同结果返回原回执；过期任务不能生成新的规划或执行。每个 attempt 固定 `source_manifest_id` 与 `plan_revision_id`，修订只替换失败目标。
