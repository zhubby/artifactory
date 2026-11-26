<!--
[INPUT]: 演进计划、固定源码及Linux原生满盘、健康保存与CI选择证据。
[OUTPUT]: 对应需求、实现、证据与剩余交付条件。
[POS]: Emissary演进的收口索引；详细结果保留在原验收记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary 演进收口核对

日期：2026-11-26。
结论：U1–U7已有实现和本地证据，Linux原生补报已通过。
U8仍待其余平台、远程CI与统一部署验收。
本页核对需求与证据，执行细节见各验收记录。
Linux复核发现邮箱锁竞争及操作日志解锁缺陷。
已修复并补回归；快照与结果见E9。
Linux ext4小卷随后完成真实满盘及持续采集恢复，见E10。
Runner健康保存失败退出已修复，新候选原生与ext4复核见E11。
Linux系统服务真实满盘及恢复随后通过，见E12。
工作树为`codex/emissary-evolution`，含未提交变更。
基线为`16535fdf1f7b2f10baaab36d58fb666d73903118`。

用户已确定下次部署强制升级全部测试设备。
旧功能版本回退、新旧混部、兼容桥接不再是出口。
已完成的旧端测试保留，不再扩充兼容矩阵。
身份、未确认数据和OTA失败保护仍是必需条件。
早期授权仅含Linux虚拟机的临时账户与服务测试。
此前按要求保留Mac mini和Windows既有服务。
用户随后授权全部修改合入main、推送并跟进CI。
用户又授权测试时卸载各构建节点旧版并安装最新Emissary。
升级必须保留身份、证书和未确认队列。
当前仍未执行这些节点的维护，不把授权写成验收结果。

## 需求与实现对应

各单元复选框表示完整出口，不能只凭本地测试勾选。
具体场景与限制以链接中的验收原文为准。

| 需求／单元 | 当前实现落点 | 已有证据 | 尚需读回 |
|---|---|---|---|
| R1、R2／U1 | `emissary-protocol/src/{commands,telemetry_negotiation}.rs`；独立Telemetry协商、类型解码、摘要ACK | E1及[本地协议记录](2026-11-26-evolution.md) | 部署后Control、Telemetry及全部设备的版本与能力 |
| R3、R4／U2 | `emissary/src/telemetry/{inventory_delivery,inventory_lane,spool}.rs`；`telemetry-service/src/store/inventory.rs` | E1、E2、E9、E11、E12；独立回执、提交前保留、冲突隔离及Linux原生满盘后补报 | 其他目标系统维护后补报、实际长离线恢复 |
| R4／U3 | `emissary/src/telemetry/{durability,storage_observation}.rs`；`node-telemetry-status.tsx` | [本地指标与Web记录](2026-11-26-evolution.md)、E1、E7、E10–E12；Linux普通进程与系统服务满盘采集恢复 | 其他目标盘故障、物理断电和长期持续运行观测 |
| R9、R11／U4 | `emissary/src/ota.rs`、`ota/`、`service/maintenance/`；平台安装器与真实进度 | E5、E6、E9、E11、E12；健康保存重试与Linux服务、账户、文件权限和身份保持 | macOS、Windows专用环境的原生维护 |
| R2、R6–R8／U5 | `emissary/src/operations/`、`emissary/src/runner/operations.rs`；Control有限操作接口与持久状态 | E3、E4；真实Runner与重连恢复，刷新不改调度快照 | 设备真实工具链结果、重启和存储故障下的操作串联 |
| R5／U6 | `emissary/src/telemetry/{scheduler,inventory_upload,upload}.rs`；4/1/27MiB分配 | E1；新鲜和历史均有业务ACK | 部署环境持续新鲜度；本地结果不承诺生产容量 |
| R10／U7 | `emissary/src/ota/{download,storage}.rs`；Control冻结资产续签 | E5、E6；严格Range、总期限与保护资产回归 | Windows持有句柄、目标盘断电和GC恢复 |
| R11／U8 | 原生服务夹具、两份工作流、统一升级手册 | E4、E8、E9、E11、E12；普通进程切换、CI静态门禁和Linux原生满盘恢复 | 远程CI、macOS/Windows原生补报、全设备统一升级及业务读回 |
| R12／延后 | IPC、日志源、压缩、升级活动保留准入条件 | [演进计划](../../plans/2026-11-25-001-feat-emissary-evolution-plan.md#延后能力及重新评估条件) | 不作为首批出口，不增加插件或通用Action引擎 |

表内Rust路径从`crates/`起算。
Web组件路径为`web/src/components/`。
U3的未知值仍显示未知，不把无证据状态转为零。
U5首批只有`collect_diagnostics`与`refresh_inventory`。

## 证据索引与快照边界

以下JSON在本机`/tmp/`，不是已发布的长期归档。
本次重新读取结果与摘要；通过状态来自原执行。
各行对应自己的源码快照，不能合并为同轮通过数。
临时目录被清除后，按原验收入口重建证据。

| ID | JSON文件名（位于`/tmp/`） | 证明内容与限制 |
|---|---|---|
| E1 | `emissary-current-main-combined-evidence.json` | 完整Telemetry main、双库、受限角色、真实Runner及积压公平发送；没有原生服务或生产容量证据 |
| E2 | `emissary-inventory-conflict-evidence.json` | 真实mTLS与存储重开后冲突仍隔离，原正文和检查点保持，合法刷新可确认 |
| E3 | `emissary-controlled-refresh-evidence.json` | 真实Control和Runner的28项受控探测更新；夹具工具不证明宿主SDK可用 |
| E4 | `emissary-safe-cutover-evidence.json` | 四个旧进程停止后迁移，两个当前组合接管；两个接收端分别查询和删除，不表示两者都接收过真实Runner写入 |
| E5 | `emissary-request-handoff-final-control-ota-isolated-evidence.json` | 完整Control、网关、私有S3、冻结URL续签及普通runtime OTA；起点为当前源码受控版本号 |
| E6 | `emissary-request-handoff-final-{apfs,hfs}-evidence.json` | 两种真实小卷的低空间、首字节后ENOSPC、EROFS及监督器保全；默认模式不含数据库或持续采集 |
| E7 | `emissary-{apfs,hfs}-supervisor-active-complete-evidence.json` | 较早快照的原队列、真实采集失败及恢复业务ACK；监督器与采集阶段顺序执行，不证明账户ACL |
| E8 | `emissary-native-ci-static-evidence.json` | YAML、actionlint与正反证据门禁；`native_service_executed`及`remote_workflows_executed`均为false |
| E9 | `emissary-linux-native-r90_6pfy/release-durable-final-evidence/durable_recovery.json` | 最终Linux arm64候选实际服务、维护与失败恢复后事务ACK；另有同候选生命周期及独立清理，不证明其他OS或数据库受限角色 |
| E10 | `emissary-linux-ext4-final/evidence/ext4-evidence.json` | Linux arm64 ext4真实满盘、只读、supervisor保持、同Handle采集恢复及原源事务ACK；普通进程，不证明原生服务满盘或受限角色 |
| E11 | `emissary-runner-health/linux-evidence/run/health-{native-*,ext4}-*.json` | Runner健康、排空及重连保存故障修复；新候选Linux原生生命周期、事务补报及ext4持续采集恢复；不证明其他平台或原生服务满盘 |
| E12 | `emissary-native-disk/evidence/run/native-disk-final-evidence.json` | 同release候选Linux系统服务在私有ext4真实满盘时PID及会话保持，释放后健康、采集和原源事务ACK恢复；不证明断电或受限数据库角色 |

E9逐阶段、修复快照及独立清理见[Linux原生记录](2026-11-26-linux-native.md)。
初始1443份源码的原生组合均已逐文件核对。
邮箱竞争与Journal解锁后的最终源码清单为
`f5d321e3e632e19f61946511b79b427cf60b210c0fd3624fee9a1789d0fecf52`。
最终Linux整轮326通过、0失败、15忽略。
最终候选原生补报146.378秒、生命周期98.171秒，均通过。
各次原生结果按候选分别记录，不合并不同快照。
不把E9追记成E1至E8的同轮结果。
E10耗时71.240秒，独立挂载、loop、数据库和转发清理均确认。
源码及二进制摘要见[ext4故障记录](2026-11-26-linux-ext4.md)。
它只扩展测试与CI，未修改E9产品运行逻辑。

E11修复健康与排空保存失败退出、失败确认被消费的问题。
重连先保存空会话围栏，成功后才改变会话并连接。
真实mTLS进程回归确认故障期间PID与会话保持。
中间快照335通过、0失败、16忽略，Clippy风格问题随后修正。
最终库153通过、0失败、2忽略，新增进程目标和Clippy通过。
最终1448份源码清单为
`b3c6c8e7123d1673f0c8ab087bd6939f9aeb1a3b817d17422c45a610951cb4c9`。
release候选摘要为
`8be53f9bc51610d9b8f69156557b221c94e2ee954df6a920c862ad6694e3034f`。
该候选原生生命周期95.032秒、事务补报151.275秒，均通过。
同源码ext4完整组合82.976秒，阶段与清理均通过。
安全归档下载后58份成员摘要一致，临时数据库和转发已清理。
详细边界见[健康保存记录](2026-11-26-runtime-health.md)。
E9、E10保留各自历史快照，不把其通过数归入E11。

E12原生组合212.712秒通过，观察到两次磁盘错误。
服务、runtime会话、launcher实例、身份及原源保持。
恢复后健康重新落盘，原记录与恢复样本分别取得业务ACK。
两轮资源均独立确认清理，安全归档50份成员摘要一致。
最终镜像0600权限另行真实挂载补验，不冒充同次源码。
快照、归档及边界见[原生满盘记录](2026-11-26-linux-native-disk.md)。

E1持续110.37秒，读取当前投影384次。
新鲜ACK为4次，历史ACK为9次。
最大新鲜ACK延迟15.717秒，最大投影年龄42.173秒。
它满足该本机场景的60秒／90秒目标。
不能据此推导生产吞吐、无限离线或故障时延保证。

E5耗时105.242秒，有4个同会话健康ACK。
固定launcher PID及实例保持，候选生成新startup。
真实403后续签仍绑定原对象、清单及摘要。
其余五平台资产是非执行夹具，不计为五平台OTA成功。

| 证据 | 源码清单SHA-256 |
|---|---|
| E1 | `318a373c329b136a85709778cd9333e7ecfc44e4dc8d714dd1fc00285ef1f82a` |
| E3 | `4994c1f44b6c07f55b0227909a3c581e1a5eb8aa974b104c0cd88a2f12a14ef0` |
| E4 | `4dd9d635d5c117bcfd5040bd001f7932eaa1a12d0e58bc443afe67ee3ece63be` |
| E5、E6 | `fc4c9ec0f3dae8aa575a90843250b54352bb50b6c765843d8a44fba493ed79d0` |
| E7 | `3633bc15cf22dcd17ad002fab9079db7963a92c0bb3bd3f82baaefddb4192ae4` |

E5 JSON摘要为`748d0ab5ceca9f91365be65d0462c057c0bf8db5d6cd20ace2fe7ae1dc5d8d73`。
E6 APFS摘要为`fd7a4d9190bc6e8ee7004f48db35582718660fdd26ca89d5c89840e075dec164`。
E6 HFS+摘要为`ab5605b36151725836fa8d7649505632e857501acac4a28a8948322c6c06d768`。
两种文件系统使用相同测试二进制，摘要见[请求验收](2026-11-26-ota-requests.md)。
当前`emissary/src/ota.rs`与`emissary/src/ota/storage.rs`
均与最终检查点摘要一致。
两个测试文件随后只更新L3契约，当前摘要与检查点一致。
本页新增晚于上述运行，不宣称全部证据来自最终文档快照。

## 不变量核对

| 计划不变量 | 实现和现存验证入口 | 仍然有效的边界 |
|---|---|---|
| 1、2：构建围栏与日志闭锁 | 现有assign／accept／start grant、attempt／lease；维护排空与进程树回归 | 部署后仍需真实构建与停止后代读回 |
| 3、4：提交后ACK、原字节补交 | E1、E2、E7、E9、E11、E12及Telemetry事务围栏 | Linux原生已通过；其余系统和长期离线仍需各自证据 |
| 5：mTLS身份与归属 | E1受限业务角色；有限操作权限回归；E9文件权限保持 | E9数据库授权标记为false，不移用E1结论 |
| 6：心跳只读缓存 | 有界心跳与后台采集；真实编码预算回归 | 不从本地测试推导生产租约时延 |
| 7、8：显式OTA、launcher边界 | E5管理员发起、导入不触发升级、固定进程保持 | 下次同时维护launcher与runtime，不能只升级runtime |
| 9：资源上限与可见原因 | 队列额度、心跳预算、磁盘故障与拒收回归 | 异常损坏来源保留原字节，不承诺物理磁盘严格总上限 |
| 10：观察不获得调度权限 | E3刷新revision不改启动metadata或摘要 | 真实工具探测失败仍如实显示部分失败 |

## 剩余执行顺序

1. 固定拟交付源码与资产摘要，再运行远程六平台门禁。
2. Linux arm64虚拟机已完成默认及release候选原生组合。
3. macOS、Windows改用独立宿主后再执行原生维护与失败恢复。
4. 两平台预配私有数据库，再验证事务ACK与原源删除。
5. 在目标平台补断电、Windows句柄及实际长离线证据。
6. 部署获授权后停旧、迁移，统一升级全部测试设备。
7. 读回身份、launcher能力、runtime版本与未确认队列。
8. 全部设备通过后恢复接单，验证构建、终端、遥测和显式OTA。

专用宿主的账户、服务和目录清理必须独立读回。
详细环境变量与阶段标准见[原生手册](../../emissary-native-acceptance.md)。
现有工作流六平台运行生命周期，Linux另有隔离数据库补报。
macOS、Windows完整补报仍需预配私有数据库或专用runner。
不能把其他GitHub任务的`localhost`当作可用数据库。
双架构TimescaleDB镜像能否拉取仍待远程任务确认。
Linux ext4完整磁盘组合已接入两份工作流。
本次完成虚拟机普通进程、系统服务运行与静态门禁。
这些结果不计为远程CI通过。

收口审查发现常规CI未排除新增原生满盘用例。
旧参数在Linux真实测试二进制中选中该专用用例。
已补`--skip native_service_disk_recovery`。
新参数保留21项普通用例，排除三项原生服务用例。
专用驱动仍精确选中满盘用例；未执行额外服务测试。
选择结果保存在`/tmp/emissary-delivery-audit/`。
这次修正晚于E12快照，不改变其产品或测试二进制。

Linux原生测试创建的服务、账户与目录已独立确认移除。
自有随机数据库、回环relay与反向转发均已清理。
Mac和Windows既有服务只读核对仍在运行。
此前用户要求保留这些服务，后续已授权统一升级。
先前GitHub远端查询连接超时，未据此判断分支状态。
2026-11-26 08:06 UTC重新查询GitHub API成功。
当前功能分支引用返回404，该分支CI记录为0。
读回保存在`/tmp/emissary-delivery-audit/remote-ci-readback.json`。
远程门禁需先提交并推送当前源码。
用户已授权所有修改合入main、推送并跟进CI。
此处仍保留推送前的远程读回，不代表最新CI状态。
本轮代理未收到任务正文，不计为独立审查。
先前共享S3超时bucket状态仍未知，不写成全部资源清理完成。

当前无需等待旧功能回退目标，也无需新增产品范围决定。
下一步合并全部改动，推送并跟进远程CI。
CI通过后逐台排空、维护launcher与runtime并读回结果。
完整目标保持未完成；不以重复本地测试替代这些出口。
