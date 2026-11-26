<!--
[INPUT]: Uplink固定源码、演进契约、独立工作树和分层测试。
[OUTPUT]: 记录本地实施、真实协议与数据库证据及平台边界。
[POS]: U1至U8的实施检查点，不替代部署或设备验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary演进本地实施记录

日期：2026-11-26。
状态：实施中。没有提交、推送、发布或部署。
工作树：`codex/emissary-evolution`。
基线：`16535fdf1f7b2f10baaab36d58fb666d73903118`。
Uplink参考：`e94a67186d64b3a38233da1afdafc966082f4492`。

[评估](../../plans/2026-11-25-emissary-uplink-assessment.md)描述变更前的源码。
[计划](../../plans/2026-11-25-001-feat-emissary-evolution-plan.md)定义完整出口。
本记录只证明下列场景，不把所有计划项标为完成。
需求、快照与剩余出口汇总见[收口核对](2026-11-26-readiness.md)。
后续Linux原生服务已通过，见[虚拟机验收](2026-11-26-linux-native.md)。
下文未执行原生服务的陈述保留各次历史检查点范围。

2026-11-26用户确定下次部署统一升级全部测试设备。
旧功能回退与新旧混部不再是本轮交付出口。
下文旧矩阵和同源码恢复保留为已经完成的证据。
它们不再引出旧功能回退候选或兼容桥接待办。
同版本升级验收、身份保留与失败保护继续要求。

## 实施范围

| 单元 | 当前实现 | 尚未取得的完整出口 |
|---|---|---|
| U1 | typed命令目录、独立Telemetry协商、原字节摘要ACK；普通进程停旧切换 | 部署环境安全统一切换 |
| U2 | 独立清单RPC、耐久回执、4MiB保护额、revision与当前startup查询 | 真实设备长期离线和升级数据库读回 |
| U3 | 有界心跳、三类队列进展、耐久拒收、磁盘缓存、控制台与真实RPC指标测试 | 真设备磁盘满、断电和持续负载验证 |
| U4 | 空间预检、真实下载进度、排空计数、启动器维护；Linux原生已通过 | macOS、Windows实际服务、账户与文件权限验收 |
| U5 | 两个有限只读操作、持久接受与终态、当前Session授权、取消与重启重放 | 真设备完整操作串联与持久盘故障矩阵 |
| U6 | 1MiB最新主机槽、27MiB历史、公平机会、独立清单退避 | 生产接收端持续新鲜度与容量测量 |
| U7 | 受保护资产GC、严格Range/ETag/Content-Range、冻结资产URL续签 | 真Windows持有句柄与断电恢复矩阵 |
| U8 | 本机mTLS、真实双库、双副本切换及Linux arm64原生补报 | 全测试设备统一升级；macOS arm64、Windows x64原生验收 |

构建授权、attempt/lease、日志闭锁和mTLS继续生效。
清单刷新只追加观察，不改变调度metadata或已有计划。
旧.batch保持ID、时间和原字节。
旧服务batch ACK不删除待独立补交的清单。
运行程序OTA不替换固定启动器。
跨进程下载中断仍需管理员显式重试。

## 关键修正与回归

目录rename或unlink成功后，内存索引立即前进。
目录fsync失败保留同步债务，后续读写先补同步。
回归覆盖新鲜槽重命名、确认删除和历史确认。

维护围栏使用唯一nonce。Runner先关闭准入。
操作任务返回真实活动状态后，才确认nonce。
普通活动记录和旧nonce不能确认新的维护事务。
维护只关闭准入，不取消已接受操作。
OTA排空继续执行自己的取消政策。

维护事务独占锁覆盖维护和回退。
替换后目录同步失败仍恢复原启动器。
无法确认恢复时保留备份、围栏和恢复日志。
包装脚本先复制到私有目录，再校验实际执行的副本。
Windows服务路径按解析后的完整参数精确比对。

操作状态授权和执行授权分别保存。
resume只重放状态，不启动Accepted探测。
取消在执行信封之前投递；设备耐久接受后直接取消。
结果事务先锁节点，再锁操作，然后检查当前设备会话。
创建和取消在行锁等待后复验登录Session期限。

## 实际验证

| 层次 | 本轮证据 | 局限 |
|---|---|---|
| Rust协议与设备 | 命令截止helper前的lib/tests整轮250通过、6忽略，25个测试套件，193.54秒 | 包含真实mTLS有限操作、升级回滚、损坏清单和100秒遥测积压；新增命令helper与真实数据库组合另列 |
| 维护与OTA | 设备lib、下载、存储和契约122通过、2忽略；维护故障与nonce9项 | FakeHost不证明真实服务账户、启动模式或ACL |
| Runner操作 | 最新5项通过，含先取消不执行、维护不中断和真实子进程关闭 | 仍需目标系统故障矩阵 |
| Control操作 | 真实PostgreSQL8项通过，最终13.67秒；拆分后的租户锁与隔离各2项通过 | 隔离schema，无生产写入 |
| Control OTA | 真实PostgreSQL23项通过，29.54秒，含冻结续签与取消 | 隔离schema，无真实对象存储票据过期 |
| Telemetry数据库 | 真实TimescaleDB4项通过，含幂等、事务故障、180天保护和purge围栏 | 独立数据库，无生产迁移 |
| 公平发送 | 100秒真实mTLS：30秒采样、8秒RPC延迟、1000条历史积压、清单持续失败；至少3个新鲜主机ACK与3个历史ACK，观察年龄<90秒，主机ACK延迟≤60秒 | 有界本机测试接收器，无生产吞吐结论 |
| 协商大小 | 新接收端限制2048字节批次和128字节清单；大记录原字节保留，小记录继续发送 | 不自动改写或拆分旧批次 |
| 协议进程 | 真实mTLS升级和回滚通过；有限操作重复与重连通过 | 本机进程，不是已安装服务 |
| 安装脚本 | Python17项通过，含候选替换、私有路径与配置链接回归 | 命令桩，不执行提权或服务替换 |
| Web | 最终typecheck通过；desktop/tablet/mobile完整回归33/33通过，1.8分钟 | 确定性API夹具，不是真设备 |
| 静态检查 | 新增真实接收组合后四个Rust包all-targets Clippy零警告；访问边界、模块边界和统一版本通过 | 不替代其他OS构建与运行 |
| Control与Telemetry单元 | lib/tests共103通过、259忽略，18个套件；追加迁移身份回归通过 | 忽略的数据库用例单列，不并入通过数 |
| 基线数据库 | 隔离PostgreSQL迁移与未知历史拒绝2项通过，1.53秒 | 不代表生产迁移或旧服务重启兼容 |
| 脚本契约 | 版本、访问边界和部署25项通过；工作流actionlint通过 | unittest需要PYTHONPATH=scripts，不执行实际部署 |

Web已查看中英文320px截图，检查换行和横向溢出。
临时预览与依赖链接已清理。
mdBook构建通过。全站121章、123个HTML、本地链接与资源检查通过。
预处理器报告mdBook版本差异警告，未影响本轮生成与链接检查。

新增大小协商用例首轮使用超长os测试字段，触发清单校验失败。
改为符合契约的有界工具版本清单后，定向与整轮测试通过。

Windows整包交叉编译在ring的C构建阶段受阻。
环境缺少Windows SDK的assert.h。
单独Windows API类型检查通过，不能替代整包构建。
没有执行原生Windows维护。

## 残余边界与交付顺序

损坏旧批次和新鲜槽保留原字节，不自动删除。
保留的损坏文件不提供严格物理磁盘总量上限。
损坏独立清单或启动检查点只关闭清单通道。
普通资源采样与执行租约继续运行。
未提取清单的旧批次保留原位，禁止确认或淘汰。
清单条数与字节为未知，不伪装为空队列。
管理员修复存储并重启后，按原身份与时间补交。
损坏刷新revision检查点只使刷新失败，不阻止已有观察交付。
程序不覆盖损坏字节，不猜测新身份或revision。
损坏总spool索引、文件系统和未知错误仍不保证自动恢复。

先排空旧Telemetry，再发布当前接收端、迁移和只读表授权。
写入、查询和删除必须全部切换到当前版本。
原版旧副本不能与当前副本随机混部。
再发布发送端，保留旧格式回放与新记录接收。
启动器维护需独立管理员操作和签名来源校验。
备份保留到遥测、构建和一次显式OTA读回完成。
当前版本故障修复保留新表、清单和未确认结果。

本地复核命令：

```sh
rtk cargo test --locked -p artifactory-emissary-protocol -p artifactory-emissary --lib --tests
rtk cargo clippy --locked -p artifactory-emissary-protocol -p artifactory-emissary -p artifactory-control-plane -p artifactory-telemetry-service --all-targets -- -D warnings
rtk cargo test --locked -p artifactory-control-plane --test database node_operations -- --ignored --test-threads=1
rtk cargo test --locked -p artifactory-control-plane --test database ota_ -- --ignored --test-threads=1
rtk cargo test --locked -p artifactory-telemetry-service --test database --test inventory_delivery --test workspace_lifecycle -- --ignored --test-threads=1
rtk proxy python3 -m unittest crates/control-plane/src/installation/tests.py
rtk proxy cargo fmt --all -- --check
rtk proxy mdbook build
rtk proxy python3 scripts/check_docs.py
rtk proxy git diff --check
```

数据库命令要求`ARTIFACTORY_TEST_DATABASE_URL`指向隔离测试库。
生成文档前先按仓库流程准备Mermaid与本地字体资源。

完整U8仍需实际目标二进制、服务定义和身份读回。
分别记录心跳、构建、终端、遥测与一次OTA。
六平台CI、交叉编译和物理验收分别记载。
本地通过不构成发布或生产验收。

## 本轮补充复核

审计发现源码与本地覆盖缺口后，补齐以下边界。

| 项目 | 实际结果 | 证据边界 |
|---|---|---|
| 清单损坏 | 三类损坏文件真实mTLS普通上传通过；源批次与损坏字节不变；修复重启按原身份补交 | 不自动修复磁盘或检查点 |
| 临时协商 | Unavailable、限流与20秒超时继续发送安全v1并重试协商 | 权限失败不降级，独立清单不猜能力 |
| 后端变化 | 先建立摘要规则再断线；旧ID回执不能删除原字节 | 本机mTLS服务切换，无多副本部署 |
| 永久拒收 | 成功ACK后发生bounds拒收，空队列仍保留原因、时间和分类计数；重启保留解释 | 丢弃不更新成功ACK |
| 队列进展 | Fresh、历史和清单分别记录ACK、进展、退避和延迟；ACK后更新缓存队列数 | 进程重启后的ACK时刻保持未知 |
| 磁盘观测 | 9项真实文件测试；分类正文、临时、索引与损坏；扫描失败和链接返回未知 | Unix块数不等于可回收空间，Windows分配未知 |
| 主机预算 | 新采样按实际JSON与UTF8字节裁剪，保留身份、时间和资源标量；预留缺口计数字宽 | 旧磁盘原字节不裁剪 |
| 接收端指标 | 真实mTLS与隔离TimescaleDB1项通过，1.03秒；验证接收/重放、11类拒收、64共享许可与释放 | 固定14个reason槽；非生产持续负载 |
| 命令边界 | terminal结构invalid_payload、operation schema_mismatch、目录策略与字段12通知边界 | 已知命令不受通知标记屏蔽 |
| 终端真实流 | 真实Runner接收columns=0，返回invalid_payload后有限操作仍成功 | 本机进程，无原生服务替换 |
| 心跳组合 | 精确64KiB启动、8KiB网络、4KiB交付诊断、32租约与OTA观察通过真实编码/解码 | 完整96KiB信封超限拒绝，诊断超限单独降级 |
| Web补充 | 类型检查通过；最终完整33/33，含中英文320px、截断5与0、7类null | 此前选择器失败已修正；HTTP夹具不替代生产串联 |
| 清单状态 | 本地故障优先；不可读等待数为未知；目录同步恢复后空队列恢复ready | 4项真实mTLS回归通过；本地恢复不制造ACK与进展 |
| 缺口删除事务 | Spool22项通过；计数与待删除原身份原子提交，重启和unlink重试不重复累计 | 最近拒收身份只保留64条；不能校正旧版历史计数 |
| Fresh未知值 | 7项真实文件回归通过，非缺失I/O与链接错误保持未知 | 不宣称对抗高权限并发替换的完整竞态 |
| 采集裁剪计数 | 私有每轮计数写入耐久累计后移除；重启保留累计值 | 1项集成回归通过；私有采集字段不进入心跳 |
| 文档契约 | 111个业务源码L3、29个L2地图与800行限制通过；修正父地图迁移描述 | 只证明结构与职责一致，不替代运行验收 |

独立通知字段12默认false，不改变旧字段1至11。
未知通知须显式标记且不携带执行授权，才只计数。
该计数在同进程重连保留，重启清零。
目前没有生产通知发送方。

磁盘观测只在后台执行，最多每10秒扫描一次。
限制16384项和8层，失败时全部总量未知。
队列逻辑额度不包含临时、索引和异常来源开销。
重复损坏pending使用独立保留名，不覆盖先前来源。

补充验证命令：

```sh
rtk cargo test --locked -p artifactory-emissary --test telemetry_protocol
rtk cargo test --locked -p artifactory-emissary --test node_operations
rtk cargo test --locked -p artifactory-telemetry-service --test ingest_metrics -- --ignored --test-threads=1
rtk cargo test --locked -p artifactory-telemetry-service --lib metrics_tests
```

指标数据库命令同样要求隔离测试库环境变量。
所有代码仍位于独立工作树，没有提交、推送或部署。

## 冻结旧接收端与原生门禁

便携入口`check_emissary_legacy_receiver.py`已在本机执行通过。
入口导出冻结基线，编译该版本生成的路由、Service和存储实现。
当前真实发送器通过mTLS连接该旧接收器。
随机Timescale数据库与私有日志由该入口独立创建。
测试结束已确认删除随机数据库，保留共享测试容器。

旧批次按原ID、时间和SHA-256入库，收到ACK后才删除源文件。
重复投递只保留一条批次和一条主机记录，新主机采样也成功入库。
三条独立清单保留原始观察，状态为`waiting_inventory_receiver`。
没有把旧接收端不支持清单误计为数据丢失。
安全回归4项通过，验证回环准入与自有进程回收。

默认模式使用真实旧路由、Service与存储，绕过旧main业务迁移握手。
新增`--receiver-main`已实际执行通过，不再绕过旧main。
该模式编译冻结旧main，应用真实旧Control迁移，并启动独立旧进程。
受限角色只有业务节点、attempt和迁移表SELECT权限。
普通读写设置下的UPDATE也被ACL拒绝。
旧main经过真实迁移校验，受保护HTTPS返回200后才发布就绪。
就绪探测验证测试CA与回环IP，不跳过证书校验。
当前发送器完成同一组原字节、重放、新采样与清单保留断言。
分别应用真实新增业务和遥测迁移后，旧main均拒绝启动。
拒绝前后批次账本与节点数据完全一致。
结束确认旧进程正常退出、随机数据库和授权角色均已删除。
业务与遥测使用一个隔离数据库内的不同schema。
它不替代生产双库拓扑、多副本滚动部署和升级数据库读回。
CI独立任务已改用完整旧main模式，尚未触发远程工作流。

原生服务辅助入口已改为调用真实平台安装器执行维护。
显式回退和坏候选自动回退继续使用独立良好CLI。
用例比较真实mTLS身份、队列字节、OTA指向、服务定义与权限。
最终发行工作流把实际release资产传入门禁，通过后才上传发行资产。
脱敏阶段JSON单独归档，不混入发行资产。
新增队列、runtime OTA及就绪屏障后，无服务测试7项通过、1项忽略。
定向Clippy、全工作区格式与工作流actionlint通过。
没有运行ignored原生服务用例，没有创建账户或修改本机服务。
Linux arm64、macOS arm64和Windows x64仍需专用宿主验收。

固定启动器维护允许保留未确认日志与受保护清单。
它要求构建、终端、未完成操作和OTA退出。
runtime OTA继续等待业务排空，不能借维护规则跳过。

队列准备使用公开Handle与Runtime，不复制持久化格式。
真实采集生成新鲜主机批次、历史批次和独立启动清单。
清单sink追加真实刷新，操作日志保存接受后取消的终态。
重新打开后原字节保持，遥测与操作写锁均已释放。
维护、runtime OTA及两种回退分别比较各类别原字节。
阶段证据只输出类别、数量、长度和SHA-256，不含正文或路径。
原`native_service_lifecycle`不启用Telemetry接收端。
新增独立补报场景的实施与运行边界见本文末。
初始权限基线先等待操作日志重开后的真实过期拒收。
真实Runner与mTLS回归确认该请求不探测、不新增日志条目。
它消除首次日志替换与权限采样的先后竞态，不使用固定等待。

成功维护后新增经真实mTLS发送的签名runtime OTA阶段。
用例等待实际generation、镜像摘要和同Session三次健康确认。
它要求runtime启动身份变化，固定启动器PID与实例保持。
后续回退使用OTA成功后的合法镜像指针基线。
不同摘要资产要求命中测试HTTP下载端。
相同摘要单列为缓存资产重新激活，不声称跨版本或实际下载。
上述原生阶段仅完成编译与无服务helper验证，尚未在服务中运行。
普通原生CI已为所有构建步骤注入实际checkout的Git commit。
未放宽签名候选对完整commit的要求。

静态复核发现Unix管理员原先发布root:0600围栏。
普通服务账户读不到nonce，运行态维护会超时。
现先在私有临时文件继承OTA目录UID/GID并设置0600。
正文和文件同步完成后，才原子发布并同步目录。
权限失败不覆盖旧围栏，也不停止或替换服务。
维护12项通过，含权限失败保留与真实文件身份检查。
非root宿主明确跳过跨账户降权读取，不能据此声称已实测。
管理员锁和备份仍保持私有，runtime不读取这些路径。
没有放宽服务账户或跳过nonce确认。

安装器原先用普通操作员权限检查服务私有配置路径。
新回归在目录不可穿越时先失败，修复后通过。
现用管理员检查普通文件和非链接，不改变目录权限。
安装脚本17项通过，含私有路径与配置链接拒绝。

CI旧接收端任务预算已增至60分钟。
三次构建各有15分钟上限，剩余时间用于准备、清理与证据上传。
工作流actionlint通过，尚无远程冷构建时间结论。

完整旧main测试也在冻结包内通过Clippy。
原样历史二进制不认识新增迁移，不能直接回退重启。
停旧后先Control迁移，再Telemetry，最后统一升级设备。
用户已取消旧功能回退，不建设兼容回退构建。
不能删除迁移记录或ACK来伪装旧数据库。

复核命令：

```sh
rtk proxy python3 scripts/check_emissary_legacy_receiver.py --database-container artifactory-timescaledb-test --receiver-main --evidence /tmp/emissary-real-legacy-main-evidence.json
rtk proxy python3 crates/control-plane/src/installation/tests.py -v
rtk cargo test --locked -p artifactory-emissary --test native_service maintenance::
```

完整旧main模式需要openssl、私有隔离管理员连接和冻结Git基线。
参数中的容器只用于取得本机测试连接，不停止或删除该容器。

## 发送器重开与真实事务确认

新增`telemetry_durable_receiver`组合回归。
实施与独立复跑均为2项通过，复跑耗时约10秒。
其中1项为共享队列准备回归，1项连接真实接收端。
当前Service与TelemetryStore使用随机回环mTLS。
隔离Timescale数据库应用真实Control与Telemetry迁移。

源队列含2条批次、3条独立清单和1份操作终态日志。
源数据由公开Handle、清单sink与操作Runtime生成。
不同startup的发送器从同一私有目录重开。
数据库独占围栏阻塞真实批次与清单写入事务。
测试确认两个接收事务等待共享锁，回执仍为零。
此时全部源正文保持原字节。
新鲜槽合法改名后，测试按原批次ID追踪正文。

释放围栏后，真实事务ACK才触发源记录删除。
数据库读回核对原ID、摘要、时间、startup与revision。
历史批次按原字节和原ID重投，记录仍唯一。
两轮遥测确认均不改写操作终态日志。
成功、错误、超时和panic均清理自有随机数据库。
最终脱敏证据在读回数据库已删除后写出。

本项证明普通进程重开与当前真实接收存储的组合。
它不证明原生服务维护后的重传或完整Control注册。
业务连接设置只读，但未验证角色ACL最小权限。
共享测试容器未停止或删除。
新增测试的Clippy零警告，模块边界检查通过。
本轮125个变更业务源文件均有文件契约和模块条目。
没有文件超过800行；项目地图已检查。

复核命令使用私有本机数据库包装器：

```sh
rtk proxy python3 /tmp/emissary-db-run.py env ARTIFACTORY_DURABLE_RECEIVER_EVIDENCE=/tmp/emissary-durable-receiver-root-evidence.json cargo test --locked -p artifactory-emissary --test telemetry_durable_receiver -- --include-ignored --nocapture
```

包装器只读取测试容器连接，不输出连接凭据。
脱敏证据不包含源正文、路径或证书。

## 原生命令截止与专用宿主手册

新增测试专用`native_service/command.rs`。
同步服务命令使用独立线程和Tokio运行时。
120秒命令预算之外，调用者最多再等待约10.1秒。
外层async超时不能替代这项同步进程截止。
复用共享进程组或Job回收，不增加产品执行接口。
stdin关闭，stdout与stderr并行持续排水。
默认每流保留1MiB，超额明确失败，不返回截断成功。
私有文件读取显式允许129MiB，保持二进制正文完整。
候选128MiB产品上限保持。

回归覆盖挂起命令和父退出后持管道的后代。
本机普通进程在截止后已退出或仅剩僵尸。
大文件两次只改变1MiB后的尾字节，读回均完全一致。
同时验证双流超额、非零退出、stdin EOF及嵌套调用。
最终快照独立复跑15项通过、1项忽略，耗时10.50秒。
该目标Clippy零警告；没有运行忽略的系统服务用例。

提权后代无法确认回收时，错误保留`cleanup unconfirmed`。
普通进程回收不代表已验证sudo或Windows Job行为。
Drop不因命令错误再次panic。
停止或卸载失败时保留账户与目录。
账户删除失败时保留目录及SID归属标记。
这些恢复路径尚需真实宿主验收。

两份原生工作流与专用宿主命令禁用测试调试段。
该设置避免Linux调试二进制超出完整读取额度。
actionlint通过，没有触发远程工作流。

新增[原生服务演进验收手册](../../emissary-native-acceptance.md)。
手册定义宿主条件、三平台命令和逐阶段读回。
它还定义真实维护补交的接线步骤与数据库条件。
该接线现已实现并完成本机编译，尚未执行原生服务。
不能计为原生业务ACK通过。
本轮126个变更业务源文件的契约、模块条目及行数检查通过。


## Runner恢复与原生持久补报接线

提取共享真实接收、mTLS材料、传输门和源队列夹具。
原发送器重开组合保留全部原断言，最终2项通过，11.85秒。
新增真实Runner恢复组合最终4项通过，11.73秒。
其中2项验证Hello回调发布顺序与失败拒绝。
其余为源队列准备与真实Runner两次启动组合。

两个Runner使用同配置、同目录和同证书。
每次真实Hello先在业务事务中登记不可变启动摘要。
提交成功后，测试控制对端才发布连接。
测试不替代真实Control注册事务。
源Fresh由真实发送器选择并改名为固定ID。
夹具只更新追踪位置，不改名或重新编码正文。

传输门透传真实协商，关闭上传时只返回Unavailable。
放行后保留原Request与TLS身份，调用真实Service/store。
测试确认批次与清单事务阻塞、零回执和源字节保留。
释放围栏后，仍运行的Runner收到真实ACK并删除原记录。
读回核对2批次、3独立清单的原身份、摘要、时间和正文。
操作日志保持原字节，不把遥测确认视为操作确认。
成功证据包含`database_removed=true`。
两个证据均包含`native_service_executed=false`。

新增独立`native_service_durable_recovery`场景。
它复用原维护、runtime OTA和两种回退流程。
固定Telemetry端点在配置保存和OTA初始化前建立。
原生服务仍运行时放行真实接收，不另启发送器代替服务。
ACK后核对原ID入库、源删除和操作日志不变。
成功路径停止并卸载服务，读回账户与目录已删除。
隔离库删除读回后，才写`durable_recovery.json`成功证据。
失败先写入不完整证据，拒绝复用既有同名证据。

围栏等待45秒，覆盖合法30秒发送退避。
原生清单最多16条，按真实限流推导放行预算。
整体原生组合截止15分钟。
两个原生用例在同进程内独占宿主服务与账户。
命令移除数据库连接变量，父进程保留数据库凭据。
候选version、strip和Git准备同样使用有界命令。
新增凭据隔离子进程回归通过。
测试子进程不继承父进程的五类数据库连接变量。

最终原生测试目标18通过、2忽略，8.14秒。
忽略项为原生命周期和新原生持久补报。
相关控制操作3项、runtime8项和终端11项通过。
它们验证共享TLS提取后的现有行为。
没有运行系统服务，没有创建账户或执行sudo。
三平台维护与实际补报仍需专用宿主授权和运行。
最终设备包all-targets Clippy零警告。
格式、差异、actionlint和文档检查通过。
文档校验121章、123个HTML与20633个本地链接及资源。
132个变更业务源码的文件契约、模块条目与800行限制通过。

普通六平台CI和发行任务显式筛选原生命周期。
没有数据库的任务不运行新补报组合，不计完整业务ACK。
Linux工作区数据库CI执行普通Runner恢复组合。
macOS和Windows完整门禁仍需私有回环数据库输入。
未触发远程CI、发布或部署。

共享授权连接仍为fixture owner并设置只读选项。
此项不证明最小权限角色ACL，证据明确标为false。
旧main受限角色ACL证据保持独立。

本机复核命令：

```sh
rtk proxy python3 /tmp/emissary-db-run.py env ARTIFACTORY_RUNNER_RECOVERY_EVIDENCE=/tmp/emissary-runner-recovery-root-evidence.json ARTIFACTORY_DURABLE_RECEIVER_EVIDENCE=/tmp/emissary-durable-extraction-root-evidence.json cargo test --locked -p artifactory-emissary --test telemetry_durable_receiver --test telemetry_runner_recovery -- --include-ignored --nocapture
rtk cargo test --locked -p artifactory-emissary --test native_service --test node_operations --test runtime --test terminal_protocol
rtk cargo clippy --locked -p artifactory-emissary --all-targets -- -D warnings
```

## 冻结旧发送器到当前接收端

新增`check_emissary_legacy_sender.py`反向门禁。
它从固定基线导出旧树，只加入测试入口。
旧Handle、Collector、Spool和生成协议保持原样。
源正文由旧类型与旧JSON夹具生成一次。
后续重试不改ID、时间、startup和原字节。
该源不等于旧Collector离线生成的记录。

当前真实Service/store通过回环mTLS接收旧端。
真实Control和Telemetry迁移在随机隔离库应用。
冻结旧端没有调用协商，所有上传使用schema 0。
业务事务围栏期间，回执为零且源字节保留。
释放围栏后，旧Handle处理真实ACK并删除源文件。
旧Collector生成的另一份主机采样也已入库。
第一旧进程退出后，第二旧进程重投原源正文。
必须观察新增duplicate ACK和再次删除源文件。
两次读回原ID、摘要、微秒时间、startup和清单。
数据库只保存一份原记录。
旧清单投影保持`legacy`来源，startup为null。
没有伪造独立`inventory_deliveries`记录。

最终真实用例1项通过，18.70秒。
父驱动确认全部自有进程退出、随机库不存在。
独立清理入口再次删除并读回，0.07秒通过。
脱敏证据为`/tmp/emissary-real-legacy-sender-evidence.json`。
该文件绑定基线、源码清单、测试和二进制摘要。

同伴复审发现外层超时的清理缺口，已修复。
Python直接监督编译器、接收器与三次旧端调用。
Rust只发布请求，等待父驱动的退出确认。
进程组先检查活成员，再reap固定身份的leader。
创建到登记期间延后父进程中断处理。
子进程不继承被屏蔽的SIGINT或SIGTERM。
Darwin只剩僵尸时的EPERM以活成员读回判断。
历史清理失败不能被后次成功覆盖。
数据库名先只读检查不存在，再记录精确预留。
外层失败后独立入口核对该预留并定点清库。
进程清理和数据库清理分别记录。
任一不能确认时，保留诊断目录并明确失败。

无数据库故障回归12项通过，4.205秒。
覆盖超时、中断、编译后代、spawn中断和清库回执。
实际接收器在CREATE后触发外层超时也已通过。
该场景确认进程退出与精确随机库删除。
证据为`/tmp/emissary-real-legacy-timeout-evidence.json`。
它不覆盖父驱动自身SIGKILL或恶意进程组逃逸。
驱动限定POSIX，不声称Windows进程树验收。

共享库生命周期调整后，恢复组合再次通过。
普通发送器2项通过，12.96秒。
真实Runner恢复4项通过，15.22秒。
Emissary全目标Clippy零警告。
格式、工作流语法和文件契约检查通过。

CI独立兼容任务已接入双向门禁和证据归档。
工作区批量测试跳过三个依赖驱动的私有入口。
尚未触发远程CI。
反向证据不包括完整当前main、Control注册和角色ACL。
原生服务与实际部署双库切换仍待验收。
U1至U8的完整出口未据此勾选。

复核命令：

```sh
rtk proxy python3 scripts/check_emissary_legacy_sender.py --database-container artifactory-timescaledb-test --evidence /tmp/emissary-real-legacy-sender-evidence.json --keep-temporary
rtk proxy python3 -m unittest discover -s scripts -p test_emissary_legacy_sender.py -v
```

## 当前完整Telemetry进程与独立双库

新增`check_emissary_current_main.py`普通进程门禁。
父驱动直接监督编译、验收和真实服务进程。
业务库与遥测库为两个独立逻辑数据库。
两库位于同一本机TimescaleDB容器。
此拓扑不证明两个数据库服务器的故障隔离。

业务库应用完整当前Control迁移和节点种子。
没有手工写入或伪造迁移历史。
真实main在遥测库应用当前Telemetry迁移。
业务授权角色使用实际部署SQL的四表读取授权。
角色不拥有表，也没有超级用户或建库权限。
事务只读设置为off时，仍拒绝业务写入。
用户表读取和建表也由ACL拒绝。

先撤销`node_startups`读取权限。
完整main必须在启动阶段按指定原因失败。
恢复授权后，重新启动完整main。
HTTP使用严格CA和IP证书校验。
缺失或错误服务token均返回401。
真实查询与mTLS上传使用该进程的独立入口。

真实Handle重开已有队列，保留原ID和字节。
真实数据库围栏阻塞批次和清单事务。
围栏期间回执为零，源文件保留。
释放后，真实ACK删除2批次和3独立清单。
原时间、startup、摘要和正文逐项读回。
操作终态日志保持原字节。
业务库没有遥测表，遥测库没有业务节点表。
SIGINT正常退出main后，再次核对持久数据。

成功用例首次通过15.32秒。
脱敏证据为`/tmp/emissary-current-main-evidence.json`。
该证据绑定源码清单和两个二进制摘要。
父驱动独立删除精确双库和角色，并读回不存在。
进程清理、库清理和角色清理分别记录。
所有清理确认后才标记passed。
无数据库驱动回归3项通过。
共享进程监督器回归扩展为13项通过。
新增回归覆盖reap后中断，禁止再次查询旧PGID。
实际双库与角色创建后触发外层超时也已通过。
该场景确认进程退出、双库与角色删除读回。
证据为`/tmp/emissary-current-main-timeout-evidence.json`。
它不证明父驱动SIGKILL后的自动恢复。

共享业务种子提取后的恢复回归再次通过。
普通发送器2项通过，9.54秒。
真实Runner恢复4项通过，13.23秒。
Emissary全目标Clippy零警告。
格式、工作流语法与文件契约检查通过。
变更业务源文件均未超过800行。
文档完整构建及20639个本地链接和资源检查通过。

资源先只读预检，再保存私有精确清单。
CREATE后附加nonce所有权注释。
CREATE与注释之间中断会留下无标记资源。
清理拒绝删除该资源，并记录cleanup_unconfirmed。
不扩大为按前缀扫描或盲删。
父驱动SIGKILL和恶意进程组逃逸不在保证内。

CI独立兼容任务增加本门禁和证据归档。
批量include-ignored跳过三个私有驱动入口。
没有触发远程CI。
本门禁先使用真实Handle，再执行两次真实Runner。
两次Runner保持配置、节点身份、证书和main端点。
测试Control在Hello发布前提交真实启动摘要。
每次连接按startup读回摘要，重启ID必须不同。
第一Runner的两类事务到达真实main围栏后停止。
队列锁释放后核对Fresh身份及全部原字节。
第二Runner重开同队列，释放围栏后收到真实ACK。
它也补交2批次、3独立清单，日志保持原字节。
全部组合首次通过138.42秒，包含真实清单限流。
不注入token，不修改生产退避或RPC超时。
共享Runner寿命夹具统一取消、等待与超时回收。
真实Control main仍待组合验收。
冻结旧Handle接入此main的后续证据见下节。
原生服务、统一升级部署和物理故障仍独立待验。
本轮没有提交、推送、发布或部署。

复核命令：

```sh
rtk proxy python3 scripts/check_emissary_current_main.py --database-container artifactory-timescaledb-test --evidence /tmp/emissary-current-main-evidence.json --keep-temporary
rtk proxy python3 -m unittest discover -s scripts -p test_emissary_current_main.py -v
rtk proxy python3 -m unittest discover -s scripts -p test_emissary_legacy_sender.py -v
```

## 冻结旧Handle直连完整当前main

完整main驱动增加`--with-legacy-sender`。
同一服务、双库和受限角色保持运行。
当前Handle和两个真实Runner先完成退出。
随后依次运行三个受监督的旧测试进程。
它们分别准备源、首次发送和独立重放。
旧产品文件由固定Git提交导出。
只注入一个测试入口，不修改旧产品实现。
父驱动记录并复查导出文件的完整摘要清单。

第一旧Handle直连真实main的mTLS入口。
真实事务围栏期间，原文件保留且回执为零。
释放围栏后，服务事务提交并返回业务ACK。
第一旧进程收到ACK后删除源，父驱动确认退出。
另有真实旧Collector的新主机采样入库。

第二旧进程使用全新私有队列。
它重新投递同ID和原字节的预置批次。
它不表示同一旧持久队列的重启恢复。
首队列的其他未确认采样保持原样。
当前发送器已退出，main入站请求归零后取计数基线。
第二进程退出后，replayed严格增加，rejected不变。
第二源文件删除，两个旧队列的dropped均为零。
原node/batch仍只有一条，摘要与原字节一致。
这组证据共同证明重复确认，不能只看服务端计数。

旧内联清单仍显示legacy来源，startup为null。
没有增加独立清单RPC回执。
真实HTTP查询和退出后数据库读回均核对原记录。
全部组合首次通过167.14秒。
证据为`/tmp/emissary-current-main-legacy-evidence.json`。
它包含旧二进制、测试入口、当前main和源码清单摘要。
双库、受限角色和全部自有进程均确认清理。

schema 0与无协商来自冻结协议源码和实际二进制。
该旧proto没有schema字段或协商RPC。
本门禁没有现场记录每个RPC的schema值。
原Service/store门禁的现场schema计数保持独立。
不以两个不同门禁相加冒充同一现场测量。
预置源由旧Batch类型和旧JSON样本一次序列化。
它不是旧Collector离线生成的数据。
新增主机样本才证明实际旧Collector运行。

驱动无数据库回归扩展至6项。
覆盖旧进程顺序、凭据隔离和活发送时外层超时。
冻结产品文件发生变化时，验收明确拒绝。
CI完整main门禁启用该矩阵并归档脱敏证据。
没有触发远程CI或变更运行服务。
完整Control注册的后续证据见[组合验收](2026-11-26-control-ota.md)。
同版本多副本实际部署和原生服务仍待验收。

复核命令：

```sh
rtk proxy python3 scripts/check_emissary_current_main.py --with-legacy-sender --database-container artifactory-timescaledb-test --evidence /tmp/emissary-current-main-legacy-evidence.json --keep-temporary
rtk proxy python3 -m unittest discover -s scripts -p test_emissary_current_main.py -v
```

## Control与OTA组合证据

后续真实Control注册、故障重放、旧清扫保护、
受控全量刷新和普通进程OTA见[组合验收](2026-11-26-control-ota.md)。
资产续签、进程清理与构建产物恢复证据也保存在该记录。
两份记录共同描述本地边界，不替代原生服务与部署验收。

## Linux原生持久补报的工作流接线

两份原生工作流为Linux amd64和arm64接入独立补报。
原生命周期仍由六平台顺序执行。
TimescaleDB仅在Linux创建，绑定当前任务回环端口。
不挂载持久卷，不启动本机共享服务脚本。
数据库连接变量只传给补报步骤。
生命周期和补报分别使用全新证据目录。
发行任务将已核对版本的最终资产作为维护及OTA候选。
初始服务仍为Cargo测试二进制，不冒充最终资产安装。

补报步骤20分钟，包含15分钟组合预算与清理余量。
原生任务增加到50分钟，容纳编译和两种场景。
固定完整测试名并使用`--exact`。
Cargo成功后必须读取本次`durable_recovery.json`。
原生执行、真实迁移、事务围栏、原字节、ACK、
操作日志保持及服务账户目录和数据库清理均须为true。
还要求原2批次、3清单及至少4次真实启动。
缺失、未完成或错误证据会阻断发行任务。
两组阶段JSON分目录归档，不把诊断归档当作成功门禁。

本次actionlint、YAML解析与差异检查通过。
直接提取两份工作流的内联检查器，共86个案例通过。
案例包括完整证据、缺失文件、坏JSON、缺失字段、
false、整数冒充布尔、错误源数量和不足启动覆盖。
证据保存在`/tmp/emissary-native-ci-static-evidence.json`。
文件契约检查覆盖225个变更文件、159个业务源码，零缺口。
mdBook和链接检查通过：122章、124个HTML、20947个本地链接及资源。

没有启动原生服务、创建账户或执行sudo。
没有触发远程CI、签名、发布或部署。
DockerHub清单请求超时，双架构远程拉取仍待确认。
本机缓存有arm64镜像，不据此推导GitHub任务成功。
macOS和Windows的持久补报仍需专用回环数据库。
物理故障与统一升级部署保持独立出口。
用户已取消不同功能版本回退要求。


## 遥测冲突与积压增量

后续修复满8192条时主机等量替换误淘汰历史。
也补齐独立清单永久冲突的持久隔离与诊断。
真实Service/store、重开和完整main积压证据见
[遥测冲突与积压验收](2026-11-26-telemetry-conflicts.md)。
该记录保留最终测量、清理和原生/生产边界。

## Linux虚拟机原生增量

用户授权Linux虚拟机测试账户与服务。
生命周期、默认候选补报及显式release候选补报均已通过。
原服务运行中完成事务ACK，提交前保留原源。
维护后身份、账户、文件权限与原队列保持。
独立读回确认服务、账户、目录及随机数据库均已清理。
Linux整轮发现OTA邮箱瞬时锁冲突终止runtime。
已改为单槽等待与重试，缓存未知时也保留请求。
另修复操作日志销毁后继承描述符暂时保留锁。
两项均先由真实锁回归复现，再验证修复。
源码清单、候选摘要与阶段结果见[Linux原生记录](2026-11-26-linux-native.md)。
Mac mini和Windows既有服务按用户要求保留。
只读查询显示两者仍在运行，没有执行维护。
两平台、物理故障、远程CI和统一升级仍待验。
