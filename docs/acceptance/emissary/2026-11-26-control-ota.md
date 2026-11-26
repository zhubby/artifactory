<!--
[INPUT]: 当前Control、Telemetry、网关与Emissary的真实组合门禁。
[OUTPUT]: 记录注册、操作恢复、清单完整性与普通进程OTA证据。
[POS]: 演进本地记录的集成证据；原生服务与部署分别验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary Control与OTA组合验收

日期：2026-11-26。
状态：本地组合已验证，完整计划仍在实施。
工作树与基线见[演进本地记录](2026-11-26-evolution.md)。
本记录使用普通进程与隔离数据库。
它不替代原生服务、历史发行兼容或生产部署。

## 真实Control注册与有限操作组合

新增`check_emissary_control_main.py`普通进程入口。
它重新编译当前Control、Telemetry与Emissary。
证据绑定源码清单和三个实际二进制摘要。
真实Control执行迁移并创建初始化管理员。
管理员经HTTP登录后签发单次注册token。
真实CLI经Enrollment RPC取得节点证书和遥测地址。
同token向新目录再次注册必须返回Unauthenticated。
配置不生成，业务节点数量保持一条。
token使用时刻、节点ID和证书摘要保持不变。
拒绝目录不留下节点私钥或证书。
没有预写设备身份、startup或操作结果。

Control、Telemetry和Runner使用独立私有PKI。
设备连接与内部查询保持严格CA和IP验证。
临时CA声明签发用途，服务器声明密钥标识。
OpenSSL严格证书校验在服务启动前通过。
Control公开HTTP只监听回环开发端口。
它不证明公网网关TLS或浏览器Secure Cookie。
运行环境只传递明确白名单。
Runner不继承数据库、S3、规划和通知凭据。

两次真实Runner使用同配置、目录、节点和证书。
每次启动产生新的startup。
首次启动前，独立psql事务锁住本次节点行。
真实Control登记事务必须等待该锁。
Telemetry实际返回startup_unregistered，可重试拒收。
此时node_startups和独立清单回执均为零。
真实落盘清单和未确认检查点保持原字节。
普通主机批次不要求等启动登记，不作零批次断言。
取消自有锁会话后，真实Control提交启动摘要。
原观察ID、交付摘要、startup和时间逐项读回。
设备检查点必须确认同一原观察ID。
原清单文件删除后，才记录客户端ACK。
首次Runner停止后再次读回，才启动第二次。
真实Control提交的启动摘要逐次从业务库读回。
每个startup的启动清单和主机样本进入遥测库。
Control真实查询代理返回当前startup和native清单。

管理员经HTTP分别发起诊断和完整清单刷新。
同request_id重发返回同operation_id。
诊断成功，结果摘要和终态确认读回。
设备journal逐项匹配节点、startup和会话。
操作类型、输入摘要、终态与序号必须一致。
同结果摘要确认且正文清除另行检查。
result_expired必须为零，排除本地过期清理。
服务端结果持久确认不代替设备收到ACK。
本机xcodebuild返回exit_failure。
完整刷新如实保存部分失败，并确认结果。
它没有生成新revision或覆盖启动调度摘要。
本项不声称完整刷新成功。
成功刷新由前述受控运行时用例独立覆盖。

两库为同一测试容器内的独立逻辑数据库。
Telemetry使用实际四表只读授权。
事务只读设置为off时，业务写入、私有用户读取和建表仍失败。
Control使用随机独占的空S3 bucket。
它只证明启动依赖，不证明制品上传下载。
普通进程停止后，精确双库、角色和空bucket均读回已删除。
共享测试容器保持运行。

驱动拒绝无标记数据库或角色的清理。
PUT响应不明时保留bucket并报告cleanup_unconfirmed。
每轮先写失败初态，早期依赖失败不复用旧成功证据。
十项无服务回归覆盖环境、归属、原清单身份和ACK。
有界私有探针区分启动探测与已准入诊断。
父驱动SIGKILL和恶意进程组逃逸不在保证内。
没有运行系统服务、创建系统账户或执行sudo。
完整刷新成功另见下述受控场景。
OTA、构建、终端和统一升级部署分别验收。
U1至U8的完整出口仍未据此勾选。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --database-container artifactory-timescaledb-test --s3-container artifactory-s3-test --evidence /tmp/emissary-real-control-main-evidence.json
rtk proxy python3 -m unittest discover -s scripts -p test_emissary_control_main.py -v
```

## 普通Control进程故障后的结果恢复

真实组合增加`--with-control-restart`门禁。
它使用同一套临时双库、PKI和节点身份。
启动前把git配置为本次私有有界探针。
此配置用于验收，不冒充系统git。
启动探测立即返回合法版本，不计操作调用。

管理员经HTTP发起只含git的诊断。
探针进入后，监督器向本次Control进程组发SIGKILL。
确认该进程组退出后，一秒内释放探针。
Runner和Telemetry保持运行。
设备journal保存成功正文、序号和结果摘要。
它尚未确认，result_expired仍为零。
业务行尚未confirmed，完成时刻仍为空。

相同二进制、配置、PKI和数据目录重启Control。
真实Runner重连，节点与startup保持不变。
连接owner和会话均发生替换。
重连后的操作保留原输入摘要与签发、截止时刻。
执行截止不得延长。
原正文、序号和摘要进入业务库并confirmed。
设备收到同摘要ACK后清除结果正文。
同request_id再提交返回原操作ID。
探针执行恰好一次，创建和完成审计各一条。
探针计数持续到重放、ACK和重复请求检查结束。
两个既有诊断与刷新操作随后继续验收。

本项证明普通Control故障后的结果重放。
它不证明设备崩溃、原生服务或多副本滚动。
完整刷新仍记录本机xcodebuild部分失败。
自有进程、双库、角色和空bucket均完成清理读回。
共享测试容器保持运行。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-control-restart --database-container artifactory-timescaledb-test --s3-container artifactory-s3-test --evidence /tmp/emissary-real-control-restart-evidence.json
```

## 旧清扫的真实数据丢失与完整性保护

本轮冻结基线的完整Control和Telemetry源码。
导出文件逐一计算摘要，编译后再次复核。
没有修改旧产品SQL或迁移台账。
先由真实旧main建立旧业务与遥测数据库。
自有psql事务锁住旧观察表。
真实旧main的立即清扫必须等待该锁。
等待检查绑定数据库、SQL、application_name和阻塞PID。
随后由当前main迁移、真实Runner注册并提交独立清单。
旧Telemetry在迁移后继续运行，没有启动时复验。

未加保护时，释放锁后清单正文为零。
独立回执仍为一，证明确有孤立引用。
失败证据为`/tmp/emissary-legacy-retention-unprotected-evidence.json`。
该轮进程、双库、角色和bucket均完成清理。

追加迁移`2026112601`完整验证两类复合外键。
它保护delivery和startup引用的节点与正文摘要。
当前purge先删除引用，再删除正文。
受保护门禁38.955秒通过。
旧清扫的外键错误使事务整体回滚。
事务中更早应删除的过期ledger也仍存在。
原正文、观察ID和摘要逐项保持一致。
认证HTTPS调用旧purge返回500。
多张目标表计数不变，节点墓碑不存在。
两个真实startup、有限操作和设备ACK随后继续通过。
完整刷新仍如实记录本机xcodebuild部分失败。

该门禁使用独立临时认证S3。
当前三程序和旧程序摘要写入结果证据。
自有进程、双库、角色和空bucket均完成清理读回。
结果证据为`/tmp/emissary-legacy-retention-protected-evidence.json`。

故障恢复与旧清扫保护的组合门禁41.415秒通过。
原结果在Control故障后重放，探测仅执行一次。
三个当前二进制摘要与本地文件一致。
完成时源码清单与运行前一致。
结果为`/tmp/emissary-final-combined-integrity-evidence.json`。
认证读取临时S3，确认bucket列表为空。
再按容器ID和归属标记删除自有容器。
删除读回成功，私有配置与数据目录已移除。
回执为`/tmp/emissary-private-s3-cleanup.json`。

真实TimescaleDB的两项清单测试通过。
分别以delivery和startup孤立引用升级0003到0004。
ForeignKey失败不改变引用、预算和迁移台账。
索引与第一条已执行约束全部回滚。
恢复保存的原正文后，原样重试迁移成功。
两个完整RESTRICT外键生效，重复迁移不改台账。
当前purge保留另一节点的正文与引用。
生命周期独立回归一项通过。

当前发送到冻结旧完整main的门禁再次通过。
测试侧追加0003与0004后，旧main拒绝未知迁移。
拒绝启动不修改既有业务和遥测数据。
随机数据库和授权角色均完成删除读回。
结果为`/tmp/emissary-real-legacy-main-integrity-evidence.json`。

完整性保护不使旧功能兼容。
旧写入不维护startup_hosts，旧查询不理解当前startup。
发布须先排空并停止全部旧Telemetry。
当前版本接管写入、查询和删除后再启用独立清单。
本轮改为停旧后统一升级全部测试设备，不建设兼容桥接。
本轮没有执行多副本部署、生产迁移或原生服务。

共享S3所在TUF磁盘已满。
一次保护门禁因对象存储无可写卷而失败。
该轮进程、双库和角色已清理。
空bucket删除无法确认，保留原失败证据。
bucket为`emissary-acceptance-68155b220a2042bc9d34164908f4adf3`。
失败结果另存`/tmp/emissary-legacy-retention-s3-full-evidence.json`。
私有诊断保留于`/tmp/emissary-real-control-i24s_3zw`。
没有重启、删除或修改共享存储服务。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-legacy-retention --database-container artifactory-timescaledb-test --s3-container <本轮独占认证S3容器> --evidence /tmp/emissary-legacy-retention-protected-evidence.json
rtk proxy python3 scripts/check_emissary_legacy_receiver.py --receiver-main --database-container artifactory-timescaledb-test --evidence /tmp/emissary-real-legacy-main-integrity-evidence.json
```

## 真实三程序的受控全量刷新

`--with-controlled-refresh`使用明确配置的工具夹具。
共享PROBE_IDS中的28项各有私有可执行脚本。
每项独立计数，参数不作为Shell程序执行。
启动时各返回1.2.3；启动观察确认后原子改为9.8.7。
配置路径、节点、证书与Runner进程保持不变。
工具夹具不证明宿主安装了对应SDK。
硬件和加速器仍使用已有有界真实探测。

管理员经真实HTTP发起完整刷新。
操作通过原Control会话与mTLS发送到真实Runner。
自有事务对inventory_deliveries取得SHARE锁。
真实接收事务在INSERT处等待确切阻塞PID。
操作成功并收到设备结果ACK时，清单仍为pending。
此时零交付回执，查询仍返回原revision0。
原revision1文件存在且字节保持不变。

从操作结果指定的观察文件读取真实Protobuf。
按共享域分隔和大端长度前缀重算交付摘要。
观察ID、startup、revision1、schema和native来源逐项核对。
只取消本库、自有application_name和确切PID。
SQLSTATE57014、进程回收和backend消失均读回。
事务提交后读回同一观察的原摘要、时间和完整正文。
匹配ACK删除原文件，Control查询返回同一revision1。
操作结果ACK没有替代独立清单ACK。

metadata JSON、能力、接收时间和调度摘要保持不变。
node_startups整行保持不变。
原native revision0正文和观察ID继续存在。
成功后重复同request_id返回原结果、序号和时间。
两秒及后续诊断窗口内，工具计数无额外刷新调用。
revision检查点仍为1，正revision回执只有一条。
操作创建与完成审计各一条。

完整门禁39.169秒通过。
结果为`/tmp/emissary-controlled-refresh-evidence.json`。
源码清单与三个当前二进制摘要均完成读回。
此后执行SIGTERM故障门禁。
在实际刷新锁与待发revision1同时存在时中断驱动。
驱动明确失败，不写成功；全部自有子进程已回收。
随机双库、角色和空bucket已确认删除。
独立查询再次确认双库与角色不存在。
结果为`/tmp/emissary-refresh-interrupt-evidence.json`。
认证读取临时S3确认bucket列表为空。
按容器ID与归属标记删除自有S3，并读回不存在。
私有配置和数据目录已移除，共享容器保持运行。
回执为`/tmp/emissary-refresh-s3-cleanup.json`。

35项驱动回归通过，含错误交付摘要拒绝。
首轮真实门禁的最终revision0查询漏掉native条件。
legacy回填使查询返回两行；加入source_batch_id IS NULL后重跑通过。
该失败轮资源已清理，没有修改产品实现来绕过失败。
原生服务、真实工具链验收、OTA与生产部署仍未覆盖。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-controlled-refresh --database-container artifactory-timescaledb-test --s3-container <本轮独占认证S3容器> --evidence /tmp/emissary-controlled-refresh-evidence.json
```

## 真实Control、网关与普通launcher的OTA

`--with-runtime-ota`编译真实Control、Telemetry和网关。
当前候选复制到私有路径后，才构建版本号夹具。
起点使用同一源码、`0.0.0-acceptance`编译版本。
它不证明历史发行二进制的兼容性。
本机候选报告统一版本和完整Git提交。
其余五平台为明确标识的不可执行字节夹具。
本轮不证明六平台发布或原生服务验收。

真实发行CLI使用本轮随机Ed25519密钥签名。
只读回环发行API提供清单、签名与六个资产。
管理员经真实HTTP导入，Control验签和校验资产。
六个精确对象进入本轮独占的私有S3 bucket。
重复导入返回原发行ID，没有创建升级行。
发行源在显式升级前关闭并确认回收。
候选摘要目录在升级前不存在，排除缓存复用。

真实旧版本号程序通过CLI初始化OTA目录。
普通运行入口启动固定launcher与独立runtime组。
管理员显式提交当前产品版本和发行ID后才升级。
网关兑换Control票据，再从私有S3下载候选。
本轮观察到下载、排空、激活和成功四个阶段。
未模拟设备命令、request邮箱或数据库升级状态。

候选startup与原startup不同，节点身份保持不变。
健康文件观察到同一Runner标识下三个非空ACK ID。
Runner连接标识与服务端授权Session分别核对。
本地state和真实HTTP更新均为succeeded。
数据库终态、generation与冻结清单摘要一致。
固定launcher PID、实例和原文件摘要保持不变。
previous保留原运行程序，current等于签名候选摘要。
配置和证书保持不变，Control撤销ota_draining。
再次请求相同当前版本返回409，不新增升级。

候选清单事务与健康ACK分别验收。
真实Telemetry有候选startup的回执和主机记录。
客户端检查点确认后，原独立清单源消失。
Control查询的主机和native清单均指向候选startup。
node_startups保存对应启动摘要。
后续普通launcher再次启动仍使用升级后的current。

最终完整门禁101.294秒通过。
结果为`/tmp/emissary-runtime-ota-evidence.json`。
源码清单为`a045201be1f32d3a2239f79b0fb19d2481255db562f0c57eab3a2d86732c8851`。
证据记录四个真实程序及候选、起点摘要。
该清单对应运行时源码，后续文档更新另行检查。
本机完整刷新仍如实报告xcodebuild部分失败。

候选处于activating时，另行SIGTERM中断驱动。
驱动失败，不写成功；原升级尚未健康确认。
清理先保留未reap的launcher固定会话身份。
全部独立runtime组退出后才reap launcher。
启动延迟中断从Supervisor已登记子进程回收。
六对象逐一删除并HEAD=404，再删除独占bucket。
双库和角色独立读回不存在，共享服务保持运行。
结果为`/tmp/emissary-ota-interrupt-evidence.json`。
父驱动SIGKILL和恶意新建会话不在保证内。

38项驱动回归通过，耗时16.320秒。
新增真实进程测试覆盖launcher先退出、runtime独立组残留。
其余清理失败不跳过下个自有会话。
已reap身份不再次查询或发送信号。
只读修复复核未发现新的必须修复项。
所有自有会话、六对象、bucket、双库与角色清理完成。
本轮仍无系统服务安装、提权、提交、推送或部署。

临时S3认证读回bucket列表为空后按ID删除。
容器不存在和私有配置、数据目录移除均已确认。
回执为`/tmp/emissary-ota-s3-cleanup.json`。
CI新增独立普通进程组合任务。
它依次验证故障重放与旧清扫、成功刷新、真实网关OTA。
阶段脱敏JSON分别归档，未触发远程CI。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-runtime-ota --database-container artifactory-timescaledb-test --s3-container <本轮独占认证S3容器> --evidence /tmp/emissary-runtime-ota-evidence.json
rtk proxy python3 -m unittest discover -s scripts -p 'test_emissary*.py'
```

## 真实冻结资产URL续签

`--with-ota-renewal`在上述OTA门禁启用独占S3传输夹具。
夹具只接受本轮精确bucket，成功正文来自真实S3。
签名头原样透传，不记录认证头或URL查询正文。
管理员发起升级后，首次资产GET收到一次受控403。
后续GET返回同一冻结对象，最终摘要匹配才激活。
该403不证明自然票据过期。

launcher将原update、generation和冻结资产写入私有邮箱。
真实Runner用原节点证书和当前授权Session请求Control。
Control复验冻结资产后返回新网关票据。
只读观察器在显式apply前启动，不写入邮箱。
它核对实际请求与回复的完整冻结身份。
证据只输出request ID、冻结字段和回复URL的SHA-256。
成功后两个私有邮箱消失。

仅比较S3查询摘要不能独立证明续签RPC。
网关每次兑换旧票据也会重新签发S3查询。
首轮117.187秒结果只证明403后成功下载。
补充真实邮箱回执后，最终门禁106.697秒通过。
观察到恰好一个匹配请求与回复、两次同对象GET。
候选三次非空健康ACK、真实Control终态和清单ACK均通过。
结果为`/tmp/emissary-runtime-ota-renewal-evidence.json`。
运行源码清单为`2d55223d62014edc61b3014a4bd9a934b952b6362cbdda8a15bc58babb051014`。
该结果早于后续构建产物恢复检查，最终检查另列。

原程序、六对象、bucket、双库与角色均清理确认。
临时S3认证读回为空，按ID删除并读回不存在。
私有配置与数据目录移除，共享服务保持运行。
回执为`/tmp/emissary-renewal-final-s3-cleanup.json`。
两轮只读审查确认邮箱回执补足RPC证据。
新增回归覆盖单次拒收、原正文、签名头和URL脱敏。
CI普通OTA组合任务启用此续签模式，未触发远程CI。

构建旧版本号夹具后重新编译正常候选。
实际version输出恢复为原候选的版本、平台和提交。
恢复构建摘要独立记录，并要求区别于旧版本号夹具。
单包构建与组合构建的依赖特性不同，不要求字节相同。
默认Cargo输出不保留旧版本号夹具。
这项恢复不修改产品源码或版本。

恢复构建独立门禁实际通过。
日志目录为`/tmp/emissary-restore-check-grf_iebp`。
version读回为0.2.3、darwin/arm64和实际Git提交。
恢复摘要为`e37bc96e8ef5adb721a4f6435032ffb7380ecb0c4a3ff6bca31673d64644e6b9`。
106.697秒组合结果早于此项恢复检查。
本项不宣称已重跑包含恢复字段的完整组合。

最新驱动回归41项通过，耗时17.830秒。
覆盖进程会话回收、S3透传与私有续签回执。
漏失匹配回复必须使验证失败。
早先38项与35项结果保留为阶段证据。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-runtime-ota --with-ota-renewal --database-container artifactory-timescaledb-test --s3-container <本轮独占认证S3容器> --evidence /tmp/emissary-runtime-ota-renewal-evidence.json
```

## 停旧迁移、双副本与同源码兼容恢复

`--with-safe-cutover`复用原门禁的精确资源归属。
四个冻结旧完整main同时运行并通过各自健康检查。
旧业务与遥测数据库的完整迁移台账先读回。
随后停止两个旧Telemetry，再停止两个旧Control。
四个进程与活后代均回收后才执行当前迁移。
每个副本的实际数据库application_name先出现、再消失。
迁移边界保留原管理员行，没有重写旧迁移。

当前两个Control与两个Telemetry同时就绪。
每组使用独立地址、同一双库与受限业务角色。
两组Control分别查询同一真实Runner的native启动清单。
非owner Control创建完整刷新，owner副本派发。
跨副本重复request_id返回同一操作。
两个接收端分别经认证HTTPS删除牺牲节点。
夹具在同一repeatable-read事务复制真实行。
旧清单投影和startup资源另作明确SQL夹具。
每张必需表非空，删除后行数为零并有墓碑。
另一个牺牲节点和原节点的不可变行保留。
活跃节点的合法新采样允许增长。
本项不覆盖build/process与连续聚合删除。

两个当前Telemetry先停止，真实刷新仍完成操作ACK。
原revision1文件保持原ID、时间、正文和交付摘要。
接收库没有该观察的回执，不能把操作ACK当清单ACK。
非owner Control停止后，owner实际SIGKILL并回收。
真实诊断仅探测一次，结果在设备耐久保存且未确认。
全部当前副本退出后，才启动私有冻结兼容副本。
它使用迁移前复制的当前源码产物，接管原地址。
Runner进程、startup、配置与证书均保持不变。

恢复后新owner与Session重授权原结果。
原序号、摘要、输入和截止保持，执行截止不延长。
设备实际结果ACK与独立清单ACK分别读回。
原revision1正文与时间入库后，原文件才消失。
重发原刷新请求仍返回原结果、序号与时间。
探测计数不增，revision检查点仍为1。
正revision回执仅一条，创建与完成审计各一条。
启动metadata、能力、调度摘要与node_startups整行不变。
恢复前后迁移台账不变，外键继续完整验证。

实际组合46.909秒通过，清理错误为空。
结果为`/tmp/emissary-safe-cutover-evidence.json`。
运行源码清单为`4dd9d635d5c117bcfd5040bd001f7932eaa1a12d0e58bc443afe67ee3ece63be`。
证据保存旧、当前与恢复产物摘要及十个副本进程身份。
每个身份包含可执行路径、PID、应用名、库名和绑定地址。
Runner摘要与PID单独记录，全部产物最终再次核对。
本轮47项驱动回归通过，耗时17.054秒。
旧驱动测试的就绪竞态已修正，信号处理先于就绪声明。
旧超时夹具不再提前发布正常停止请求。
两轮只读审查确认四项证据缺口已补齐。
自有进程、双库、角色与空bucket均清理读回。

临时S3认证读回bucket列表为空后按ID删除。
容器不存在与私有配置、数据目录移除均确认。
回执为`/tmp/emissary-cutover-s3-cleanup.json`。
独立进程检查确认本轮十个副本和Runner均已退出。

另轮在原revision1仍待补交时向父驱动发送SIGTERM。
驱动34.057秒退出，明确记录failed与KeyboardInterrupt。
没有启动恢复副本，没有写出成功阶段。
全部自有进程、双库、角色和空bucket完成清理。
独立查询确认数据库、角色与应用会话均为零。
结果为`/tmp/emissary-cutover-interrupt-evidence.json`。
临时S3清理回执为`/tmp/emissary-cutover-interrupt-s3-cleanup.json`。
首次准备轮使用镜像ID，被Seaweed镜像名检查拒绝。
另一次S3尚未就绪，连接关闭，均未创建业务资源。
确认认证端口就绪后才完成上述中断注入。

本项证明全部停旧后的当前版本独占切换。
同源码复制品只证明兼容恢复，不证明历史功能回退。
没有随机新旧混部、负载均衡或不停机结论。
只有一个Telemetry实际接收Runner上传，另一个验证查询与删除。
CI新增同一门禁并归档结果，没有触发远程运行。

```sh
rtk proxy python3 scripts/check_emissary_control_main.py --with-safe-cutover --database-container artifactory-timescaledb-test --s3-container <本轮独占认证S3容器> --evidence /tmp/emissary-safe-cutover-evidence.json
```

## 尚未闭合的完整出口

U1至U7未发现新的必修产品代码缺陷。
这不等于全部故障矩阵与完整交付已通过。
2026-11-26用户取消不同功能版本回退要求。
下次部署统一升级全部测试设备，不再等待旧功能候选。
原样历史二进制拒绝未知迁移，不是成功回退。
历史签名资产与旧launcher交叉兼容不再是本轮出口。
三平台原生服务、账户、ACL与维护后业务ACK仍待专用宿主。
物理满盘、断电、Windows持有句柄与长离线分别验收。
六平台远程CI、生产容量、迁移与部署没有执行。
现有复选框继续保持未完成，不用本地通过覆盖上述边界。
