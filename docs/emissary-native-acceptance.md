<!--
[INPUT]: 原生服务夹具、签名候选、专用宿主与Unix隔离磁盘及数据库。
[OUTPUT]: 定义原生维护、Linux满盘、文件系统门禁与阶段读回要求。
[POS]: 测试宿主操作手册，不用于替换既有生产服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary 原生服务演进验收

本手册把[演进计划](plans/2026-11-25-001-feat-emissary-evolution-plan.md)的原生出口落到执行步骤。
当前结果见[本地实施记录](acceptance/emissary/2026-11-26-evolution.md)。
Linux arm64虚拟机已完成[原生服务与持久补报](acceptance/emissary/2026-11-26-linux-native.md)。
默认及显式release候选均已实际运行。
早期按要求保留Mac mini与Windows既有服务。
后续用户授权测试时卸载构建节点旧版并安装最新Emissary。
现有节点按维护流程保留身份和队列，再逐台升级。
本手册的临时账户夹具仍不能直接覆盖已有服务。
命令只有在宿主获得测试授权后执行。
普通构建、交叉编译和夹具编译不构成原生服务验收。
2026-11-26用户确定下次部署统一升级全部测试设备。
旧功能版本回退和新旧混部不再要求。
当前夹具中的回退指升级失败保护，不是旧功能回退承诺。

## 宿主与输入

优先使用独立 Linux arm64、macOS arm64、Windows x64。
每个平台单独记录实际系统、架构、源码与候选摘要。
Windows x64 在协议和环境变量中写作`windows/amd64`。
模拟器、Rosetta 和真实硬件分别记录。

| 输入 | 要求 |
|---|---|
| 服务 | `artifactory-emissary`尚未安装 |
| 账户 | `artifactory-ci`尚不存在 |
| Unix 操作员 | 普通账户，具备本次测试所需的`sudo -n`权限 |
| Windows 操作员 | 管理员 PowerShell；测试账户不加入管理员组 |
| 工具 | Rust 1.95.0、Protobuf、Git；Windows 另需`pwsh`及目标运行库 |
| 源码 | 固定工作树快照；记录完整Git提交和未提交差异 |
| 候选 | 同平台、可执行、报告真实40位构建提交的二进制 |
| 证据 | 使用全新的宿主外部目录，不复用旧阶段JSON |

夹具拒绝覆盖既有服务或账户。
测试仍会创建账户、安装服务并替换测试启动器。
已有 Artifactory 节点不符合本手册的宿主条件。
不要为了通过前置检查而删除既有节点或账户。

`ARTIFACTORY_NATIVE_BINARY`用于指定最终发行候选。
显式候选必须与夹具安装的测试二进制摘要不同。
缺省候选只证明同一构建的机械维护路径。
本测试使用临时签名密钥，不能证明正式发行签名。
最终发行来源和签名验证仍按[发布流程](versioning.md)执行。

## Unix 执行

从固定工作树进入仓库根目录。
保持普通操作员身份，不以root启动测试进程。
将平台值改为宿主真实值。

```sh
export ARTIFACTORY_BUILD_COMMIT="$(git rev-parse HEAD)"
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export ARTIFACTORY_NATIVE_SERVICE_TESTS=1
export ARTIFACTORY_NATIVE_SERVICE_SCOPE=dedicated-host
export ARTIFACTORY_NATIVE_EXPECTED_PLATFORM=linux/arm64
export ARTIFACTORY_NATIVE_EVIDENCE_DIR="$(mktemp -d /tmp/emissary-native-evidence.XXXXXX)"
cargo test --locked -p artifactory-emissary --test native_service --no-run
cargo test --locked -p artifactory-emissary --test native_service native_service_lifecycle -- --ignored --nocapture
```

macOS arm64使用`darwin/arm64`。
最终候选验收还需将`ARTIFACTORY_NATIVE_BINARY`设为资产绝对路径。
构建提交来自当前源码，不使用占位提交绕过候选检查。

## Windows x64 执行

在专用宿主的管理员PowerShell中进入仓库根目录。
使用实际Windows x64工具链，不用交叉编译结果替代运行。

```powershell
$env:ARTIFACTORY_BUILD_COMMIT = (git rev-parse HEAD).Trim()
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:ARTIFACTORY_NATIVE_SERVICE_TESTS = '1'
$env:ARTIFACTORY_NATIVE_SERVICE_SCOPE = 'dedicated-host'
$env:ARTIFACTORY_NATIVE_EXPECTED_PLATFORM = 'windows/amd64'
$evidence = Join-Path $env:TEMP ('emissary-native-evidence-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $evidence | Out-Null
$env:ARTIFACTORY_NATIVE_EVIDENCE_DIR = $evidence
cargo test --locked -p artifactory-emissary --test native_service --no-run
if ($LASTEXITCODE -ne 0) { throw 'Native fixture compilation failed.' }
cargo test --locked -p artifactory-emissary --test native_service native_service_lifecycle -- --ignored --nocapture
if ($LASTEXITCODE -ne 0) { throw 'Native service acceptance failed.' }
```

临时服务密码由夹具生成，经环境变量传递。
不要把密码加入命令、截图或验收记录。
账户清理使用SID归属标记，禁止按同名账户直接删除。

测试构建禁用调试段，避免把DWARF计入维护读取预算。
两份原生工作流使用相同设置，正式release资产保持原构建策略。
签名候选的128MiB产品上限仍然生效。

## 阶段读回

测试退出码与全部必需阶段共同决定结果。
先检查每份JSON的`passed`，再核对其资产摘要和检查项。
中断留下的`passed=false`不能被其他成功阶段抵消。
同一目录中的旧JSON不能证明本次运行。

| 阶段 | 必需读回 |
|---|---|
| `prepared` | 原资产与候选摘要；有效队列已完成本地准备 |
| `maintain` | 实际平台安装器成功；原身份、队列、账户、权限及OTA指向保持 |
| `runtime_ota` | generation、运行程序摘要和启动身份变化；固定启动器PID及实例保持 |
| `explicit_rollback` | 独立良好工具恢复原启动器；采用OTA完成后的合法指向基线 |
| `automatic_rollback` | 已签名坏候选失败；原启动器与原账户恢复，恢复日志为`rolled_back` |
| 生命周期 | 长任务后代在重启、停止后退出；重连后后续任务成功 |
| 清理 | 测试服务、测试账户和自有临时目录均不存在 |

队列类别与正文摘要从维护阶段的`checks.queue_records`读回。
`prepared`阶段不包含这些队列证据。

runtime OTA的`same_asset_mechanical_reapply`只证明缓存资产重新激活。
不同摘要候选必须实际命中测试下载端。
同版本不同构建仍不能冒充跨产品版本升级。
阶段JSON只保存类别、数量、长度和摘要。
原配置、证书、私钥和队列正文不进入公开证据。

退出后单独只读检查服务和账户已移除。
Linux检查`systemctl show`与`getent passwd artifactory-ci`。
macOS检查`launchctl print system/artifactory-emissary`与`dscl`账户记录。
Windows检查`Get-Service`与`Get-LocalUser`。
异常退出时先核对资源归属，再执行针对该次测试的恢复。
停止或卸载未确认成功时，夹具保留账户与目录。
账户删除失败时保留目录及Windows SID归属标记。
命令超时的`cleanup unconfirmed`要求独立读回进程状态。
不使用通配符清理账户、服务或其他临时目录。

生命周期与独立补报在成功返回前均执行清理读回。
账户或目录残留、账户查询失败都会阻断测试。
Linux夹具用useradd创建本地账户。
其门禁读取/etc/passwd并校验格式与精确账户名。
macOS要求账户枚举成功；Windows显式区分不存在与异常。
三项普通回归只验证这些判断，不执行系统清理。

## 本门禁之外的出口

`native_service_lifecycle`只验证原生维护和队列保持。
独立的`native_service_durable_recovery`接入真实Telemetry存储。
它在维护、runtime OTA和两种回退后放行原服务补交。
Linux arm64虚拟机已执行该原生组合并读回业务ACK。
其他系统仍需独立原生执行，不沿用Linux结果。
普通Runner两次启动与真实事务ACK已单独运行通过。

完整补交门禁需要每个平台可访问隔离TimescaleDB。
数据库凭据只给测试父进程，不写入服务配置。
Linux任务可使用本机回环容器端口。
macOS和Windows任务需预配数据库或专用runner。
跨宿主数据库通过私有回环隧道接入。
不同GitHub任务不能共享彼此的`localhost`。
缺少数据库的任务不能记为业务ACK门禁通过。

独立组合场景执行以下步骤：

1. 提取现有真实接收夹具，复用迁移、授权和读回。
2. 同一测试CA签发Control与Telemetry服务证书。
3. 保存固定的非空Telemetry端点，运行中不改配置。
4. 传输门放行真实协商，暂缓上传；不构造成功ACK。
5. 真实发送器先固定新鲜批次的发送身份，再启动服务。
6. 按每次真实Hello登记startup，再执行维护及回退。
7. 原生服务仍运行时放行上传，验证事务ACK及原ID读回。

传输门关闭时，采集仍会生成新记录。
原新鲜批次必须先由发送器改名为固定ID文件。
不能手工改名或把新采样覆盖误判为维护丢失。
只冻结原源集合，不把后续采样混入原字节断言。
新增startup清单仍遵守真实限流，不改数据库token。
放行预算按实际清单数计算，并设置总数与时间上限。
短事务围栏验证提交前保留，不跨整个维护过程持锁。
最后清理服务、账户、目录和自有随机数据库。
读回资源不存在，再写`durable_recovery.json`成功证据。
同名证据已存在时拒绝运行，要求使用全新外部证据目录。

在上文宿主授权与环境准备完成后，另选新证据目录。
只给测试父进程注入私有`ARTIFACTORY_TEST_DATABASE_URL`。
它必须连接本机回环TimescaleDB，允许创建和删除自有随机库。
不要把完整数据库URL加入命令、日志或服务配置。
执行以下命令；没有数据库时立即失败。

```sh
cargo test --locked -p artifactory-emissary --test native_service native_service_durable_recovery -- --ignored --nocapture
```

该场景15分钟总预算内运行，清单最多16条。
清单放行预算为45秒加超出4条部分每条30秒。
不调整真实接收端限流，不按原源优先顺序假设完成时间。
普通六平台CI和发行工作流均执行原生命周期场景。
Linux amd64和arm64另行执行独立持久补报场景。
TimescaleDB属于当前任务，不使用命名持久卷。
端口固定绑定当前宿主的`127.0.0.1:15432`。
其他平台使用空镜像条件，不创建数据库服务。
数据库连接变量仅传给独立补报步骤。
两个场景顺序运行，分别使用全新证据目录。
独立步骤预算20分钟，包含15分钟测试预算。
整个原生任务预算50分钟，包含编译与生命周期。
测试成功后再检查原生执行、事务ACK与资源清理证据。
证据缺失、false或未完成均使该步骤失败。
发行任务把已核对版本的最终资产作为维护和OTA候选。
初始安装仍使用Cargo测试二进制，不冒充直接安装最终资产。
补报门禁失败会阻断签名发布。
任务结束由GitHub回收数据库容器及临时卷。
工作流已完成接线与静态检查，尚未运行远程CI。
Linux虚拟机手工执行已通过，不计为GitHub任务成功。
TimescaleDB标签的双架构拉取仍需远程任务确认。
macOS和Windows没有预配数据库，仍只运行生命周期。
Linux工作区数据库CI包含普通Runner恢复组合。

该门禁使用测试Control对端，不覆盖真实Control注册事务。
真实部署需独立验证停旧迁移与同版本多副本。
普通进程的冻结旧端与当前接收端双向门禁已单独运行。
反向门禁含CREATE后外层超时的定点清理读回。
其Service/store证据不计入原生服务出口。

## Linux原生服务满盘

独立入口为`scripts/check_emissary_native_disk.py`。
它复用Linux挂载身份校验及原生维护夹具。
运行前拒绝既有服务和`artifactory-ci`账户。
沿用上文宿主授权、普通操作员和私有回环数据库条件。

```sh
python3 scripts/check_emissary_native_disk.py --evidence /tmp/emissary-native-disk.json
```

专用宿主保留`ARTIFACTORY_NATIVE_SERVICE_SCOPE=dedicated-host`。
GitHub托管runner沿用真实系统及架构准入检查。
最终候选仍由`ARTIFACTORY_NATIVE_BINARY`指定。
数据库连接只进入测试父进程，不进入系统服务。

驱动创建768MiB私有ext4镜像，镜像权限固定0600。
服务目录、证书和队列全部位于该卷。
挂载父目录允许账户穿过，私有文件独立限制读取。
构建、日志、证据和数据库位于卷外。
维护与runtime OTA先完成，再执行满盘阶段。

压力只写私有卷，逐级填至单字节返回ENOSPC。
接收端暂拒上传，原队列持续保留。
原服务必须连续心跳并报告至少两次磁盘错误。
释放压力后，同一服务与runtime会话必须恢复采集。
健康确认必须重新落盘，身份、权限和launcher实例保持。
放行真实事务后，原源和恢复后样本分别核对业务ACK。
采样替换pending文件不构成ACK证据。

退出后先确认自有子进程和数据库清理。
服务、账户或服务目录仍在时保留镜像。
挂载和loop身份读回一致后才卸载并删除镜像。
驱动只输出汇总JSON，不公开配置、私钥或队列正文。
两份工作流的Linux任务按顺序执行该门禁。
当前结果见对应日期的验收记录。
此场景不证明物理断电、长期离线或其他平台。
当前Handle及两次Runner到完整Telemetry进程的双库与受限角色已单独通过。
两个独立冻结旧进程到同一完整main的重放也已单独通过。
它使用两个逻辑数据库，不替代本原生服务组合。
磁盘满、断电、Windows持有句柄和长期离线分别验收。
每项保留实际证据，不能由本门禁的成功推导。

macOS普通进程另有私有HFS+/APFS小卷磁盘门禁。
它验证低空间、预检后ENOSPC及只读EROFS。
执行`python3 scripts/check_emissary_ota_filesystem.py`。
驱动只创建160MiB自有临时镜像，不需要sudo。
确认进程退出、镜像卸载后才删除镜像。
失败保留恢复路径，证据缺失或不完整均失败。
本机arm64已运行通过，macOS CI已接线但尚未远程运行。
默认文件系统为HFS+，用`--filesystem APFS`选择APFS。
每次挂载读回文件系统类型和确切挂载路径。
两种文件系统默认入口均已本机通过，CI按顺序执行。
默认模式不覆盖真实spool，且不代替原生服务或断电。

Linux使用同一入口，默认文件系统为ext4。
显式选择用`--filesystem ext4`。
需要sudo无交互权限、e2fsprogs和util-linux。
驱动只格式化自有160MiB镜像文件。
挂载后核对镜像与loop、文件系统和挂载点。
卸载后核对挂载及loop都已消失，再删除镜像。
Linux arm64虚拟机完整组合已通过，见[ext4验收](acceptance/emissary/2026-11-26-linux-ext4.md)。
CI和发行工作流已接入完整组合，远程运行仍待执行。

本机可选真实队列组合：

```sh
python3 scripts/check_emissary_ota_filesystem.py --with-spool \
  --database-container artifactory-timescaledb-test \
  --evidence /tmp/emissary-ota-filesystem-spool-evidence.json
```

指定容器必须是已授权的本机隔离TimescaleDB。
驱动保留共享容器，仅创建和删除精确随机数据库。
同一真实源集合贯穿三种OTA磁盘故障与只读重挂。
恢复可写后，真实发送器与mTLS Service/store完成事务ACK。
原ID、时间、摘要、操作日志与身份配置逐项读回。
数据库清理未确认时保留恢复镜像。
本机组合已通过，证据见[增量验收](acceptance/emissary/2026-11-26-telemetry-conflicts.md)。
它证明静置队列保全，不证明满盘时持续采集或原生服务。
macOS CI无预配数据库，默认模式不包含此组合。

`--with-active-spool`包含原队列组合并持续运行真实采集器。
驱动把私有卷填至单字节写入也返回ENOSPC。
两次采集失败后，读回磁盘错误、缺口和原队列保全。
释放空间后，同Handle落下新主机样本。
重开后确认计数保持，新样本与原记录均由真实事务ACK确认。
该模式本机已通过，不代替原生服务或断电验收。
用该选项替换上例的`--with-spool`即可执行。
附加`--filesystem APFS`执行APFS的同一组合。
HFS+和APFS完整组合均已本机通过。
APFS在真实ENOSPC时仍报告数MiB余量，不要求统计值为零。
满盘证明要求单字节ENOSPC及真实采集写盘失败。
APFS本轮无法写入失败进度，原进度保持download。
下载失败保护通过，不据此证明supervisor终态持久化。
真实supervisor另有受控状态保存和保护标记释放故障回归。
保存失败时保持当前runtime，每秒重试，恢复后继续。
成功候选已有排空确认时，也不能绕过耐久状态保存。
激活意图保存或存储锁取得失败时，也保持当前runtime。
恢复后才停止当前runtime并启动候选，三次健康确认后提交。
健康候选的成功状态保存失败时保持候选，恢复后提交版本指向。
state和storage.lock两种真实路径故障均有Unix进程回归。
候选在提交保存受阻时退出，先恢复原runtime，再重试保存终态。
候选无法启动和真实120秒健康超时也已注入保存故障。
两项均在状态路径仍不可写时恢复原runtime，解除后保存终态。
请求接受保存与取消清理故障保持原程序，邮箱交接采用私有锁。
邮箱锁故障不阻止已运行候选的健康提交，见[请求验收](acceptance/emissary/2026-11-26-ota-requests.md)。
该Unix进程回归不代替服务监督、断电或其他平台验收。
默认磁盘门禁也执行真实supervisor的同卷ENOSPC阶段。
下载首字节后填满私有卷，失败期间保持当前runtime。
未保存终态时保留原状态和下载保护，释放空间后再保存。
HFS+/APFS默认模式及持续采集完整模式均已本机通过。
supervisor与持续采集阶段顺序执行，不计为并发压力验收。
修复后的普通Control、网关、私有S3及runtime OTA已通过。
它不计入原生服务出口，详见增量验收第12节。

实际交付顺序为停旧、迁移Control、Telemetry、全部测试设备。
启用独立清单前，排空并停止全部原版旧Telemetry。
当前Telemetry接管写入、查询和删除后再启用发送端。
正文外键只阻止旧清扫删数，不支持随机新旧混部。
原样历史二进制会拒绝未知迁移，不能直接回退重启。
本轮不建设旧功能回退构建或兼容桥接。
全部测试设备升级固定launcher与runtime后再恢复接单。
升级失败保留身份、耐久回执、未确认结果与可用资产。
见[OTA手册](emissary-ota.md)与[运维手册](operations.md)。
