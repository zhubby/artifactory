<!--
[INPUT]: Linux arm64虚拟机、固定源码、原生服务夹具和隔离数据库。
[OUTPUT]: 记录维护、OTA、失败恢复、原队列补报与资源清理证据。
[POS]: 演进计划的Linux原生检查点，不替代其他系统或部署验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Linux arm64 原生服务验收

日期：2026-11-26。
用户要求保留Mac mini和Windows现有服务。
本轮只在Linux虚拟机创建测试账户与服务。
没有提交、推送、发布或部署。

## 宿主与源码

宿主为Mac mini上的OrbStack虚拟机。
虚拟机名称为`artifactory-linux-arm64`。
系统为Ubuntu noble，架构为aarch64。
内核为`7.2.12-orbstack-00394-gbecdd6130068`。
systemd实际运行；操作员为普通账户。
Rust版本为1.95，protoc版本为3.21.12。
这是Linux虚拟机原生执行，不是物理设备验收。

工作树为`codex/emissary-evolution`，包含未提交实现。
Git基线为`16535fdf1f7b2f10baaab36d58fb666d73903118`。
源码快照包含1443个文件，不含Git目录与凭据。
源码清单SHA-256为：

```text
ee03b80cd8c615a5fb9bd48b5433cb1a33eb0fd7072ab0ff399065d4c6f0e9c9
```

远程提取后及验收后逐文件核对，均无差异。
该快照对应下表最初三个原生场景。
后续修复及新快照在下文单独记录。
627份离线依赖均按Cargo.lock校验归档摘要。
依赖导出没有改动锁文件。

release配置候选为`artifactory-emissary 0.2.3 linux/arm64`。
候选报告protocol=1及上述40位Git基线。
其大小为13,842,672字节，SHA-256为：

```text
0017c2660a6aeeda92e358562975a799e0c182cd2f8d5f2ab0ea2d039cdccde5
```

该候选是本轮源码的release配置构建。
夹具使用临时签名密钥，不证明正式发行签名。
同版本不同资产不证明跨历史功能版本兼容。

## 原生执行结果

三个场景顺序使用独立证据目录。
每个场景退出码为0，且成功前执行自身清理。

| 场景 | 结果 | 时间 | 候选边界 |
|---|---|---|---|
| `native_service_lifecycle` | 1通过，0失败 | 126.913秒 | 默认夹具候选，实际账户、服务与进程树 |
| `native_service_durable_recovery` | 1通过，0失败 | 175.169秒 | 默认夹具候选，真实Telemetry存储 |
| 显式release候选持久补报 | 1通过，0失败 | 147.017秒 | 通过`ARTIFACTORY_NATIVE_BINARY`指定上述资产 |

每轮五个阶段JSON的`passed`均为true：
`prepared`、`maintain`、`runtime_ota`、
`explicit_rollback`、`automatic_rollback`。
后两项验证升级失败保护，不恢复旧功能版本。

维护调用真实Linux安装器，使用临时系统账户。
读回身份、原队列、账户与文件权限保持。
OTA指向和维护围栏逐阶段检查。
生命周期场景还验证任务后代退出与重连后任务。

release候选runtime OTA实际下载1次。
候选取得同会话3次健康确认后成功。
runtime摘要、路径与startup变化。
固定launcher的PID、实例与已安装字节保持。
原可用资产保持，显式恢复及坏候选恢复均通过。
`runtime_ota`阶段只证明OTA与队列保全。
事务ACK证据来自其后的`durable_recovery.json`。

## 原队列持久补报

两轮均应用真实业务迁移与Telemetry存储。
每轮登记5个实际原生启动身份。
固定原源为2个批次及3条独立清单。
原源覆盖历史、普通批次及旧批次提取清单。
另含startup清单、刷新清单和未确认操作终态。
维护与恢复期间，采集器继续生成后续清单。
放行时总待发清单分别为12条和11条。
该总数不改变原源集合的字节断言。

真实事务围栏确认提交前原源仍在。
放行后由运行中的系统服务补交。
接收端读回原ID、时间、摘要及幂等结果。
服务收到业务ACK后才删除原源。
未确认操作终态的原字节保持。

`authorization_acl_proven=false`保持原值。
它表示本组合未证明数据库受限角色授权。
原生维护的文件权限检查为true。
数据库受限角色另见[收口证据E1](2026-11-26-readiness.md)。
不能把两个ACL边界合并。

## 清理与证据位置

各次原生测试结束后另行只读核对：

- systemd为`LoadState=not-found`、`ActiveState=inactive`。
- `FragmentPath`为空。
- `getent passwd artifactory-ci`返回2。
- 没有Emissary进程或测试服务目录。
- 数据库列表没有`durable_receiver_%`随机库。

私有回环relay已按精确PID和命令回收。
Mac上的数据库反向转发已撤销，监听已消失。
共享TimescaleDB与S3容器保持运行。
Mac和Windows原服务只读查询均为运行中。
本轮没有停止、替换或重装这些既有服务。

本机证据位于`/tmp/emissary-linux-native-r90_6pfy/`。
远程原件位于Linux用户目录的`emissary-evolution-r90_6pfy/`。
证据目录不保存数据库URL、配置私钥或队列正文。
源码、依赖、构建输出保留在任务目录供复查。
临时归档不是已发布的长期制品。

| 归档 | SHA-256 |
|---|---|
| `initial-native-evidence.tar.gz` | `967618ce5169d7b61670c0d0aab8454a41e021c937d2967ff3de7dfda10015fe` |
| `release-native-evidence.tar.gz` | `10f6a97fb841f5272c71ce6969039547444bcfe2c5bc1b44d77dda900c524d39` |

初始归档含生命周期与默认候选补报。
release归档含阶段证据、候选摘要和源码清单。
数据库清理另由本机`database-cleanup.json`读回。

## 验收边界

Linux服务维护、失败恢复与原队列业务ACK已通过。
本组合使用测试Control对端，不是完整Control部署。
完整Control与私有S3的普通进程OTA见[组合记录](2026-11-26-control-ota.md)。
两项证据不能拼成同轮完整部署验收。
macOS只完成夹具编译，Windows只传输源码归档。
两平台原生维护与补报按用户选择保留待验。
六平台远程CI、正式签名、物理断电和实际长离线未完成。
Windows持有句柄与全部测试设备统一升级仍待后续执行。
完整U8保持未完成，见[收口核对](2026-11-26-readiness.md)。

## Linux测试发现与修复

最初整轮与确认轮均在runtime OTA用例超时。
两轮分别耗时243.596秒与223.633秒。
每轮完成16套件，229通过、1失败、5忽略。
单独复跑该用例通过，不能抵消整轮失败。
随后带状态采样的8项runtime复跑再次失败。
请求提交时出现`OTA local record busy`。
原runtime退出后，launcher没有消费到新请求。

真实`request.lock`回归稳定复现返回错误。
修复后Runner保留一个待提交请求，每秒重试。
锁或磁盘故障保持当前runtime并重试。
锁竞争不覆盖原邮箱，保存故障不伪造接受状态。
更高generation替换旧请求，同代cancel优先。
缓存未知或维护围栏存在时等待，不终止runtime。
提交成功前不宣称请求已耐久接受。
真实mTLS集成先持锁、再释放，要求实际升级成功。

本机并行库测试还暴露操作日志重开偶发失败。
单独和串行通过不能消除并行失败。
复制日志锁描述符后销毁Journal，稳定复现锁残留。
Journal改为销毁时显式解锁，沿用spool既有方式。
复制描述符回归先失败，再通过。
该修复不改变已接受操作与摘要回执。

邮箱锁修复的中间快照为：

```text
ecfbc25faa699bec5948ee1d3e26481d8e29a2902d6c084849721f48b8052593
```

该Linux轮次37套件，324通过、0失败、15忽略。
耗时349.063秒，命令覆盖Emissary、协议及执行支持。
忽略用例不计通过，数据库与原生场景另行执行。
该中间release候选原生补报亦通过，耗时149.725秒。
其五个维护阶段及独立补报JSON全部为true。
候选SHA-256为：

```text
daeda076b108967aa37c88f3f08fd89cfc874afe3293d5c8cbc4a8263e331ae0
```

上述结果先于缓存等待与Journal显式解锁的最终快照。
不同快照不合并为同轮通过数。

## 最终修复快照

最终源码含1444个文件，清单SHA-256为：

```text
f5d321e3e632e19f61946511b79b427cf60b210c0fd3624fee9a1789d0fecf52
```

清单以files数组的UTF-8 JSON计算摘要。
键排序且使用紧凑分隔符，逐文件摘要另外校验。
产品源码在冻结后保持一致，后续只补验收文档。
最终release构建耗时28.605秒。
资产大小仍为13,842,672字节，SHA-256为：

```text
636ab0802d1bfa0b37436fa152888a4020224cca269dce2e0599f4abb37d56a3
```

本机最终库测试150通过、0失败、2忽略。
最终Emissary库及测试Clippy零警告。
本机库测试不计入Linux原生通过数。

最终Linux命令使用`--locked --offline --lib --tests`。
测试包为`artifactory-emissary`、
`artifactory-emissary-protocol`和`artifactory-execution-support`。
37套件共326通过、0失败、15忽略，耗时356.825秒。
其中runtime的8项包含真实持锁后的升级与失败恢复。
监督器实际执行120秒候选健康超时。
忽略用例要求独立数据库、外部驱动或原生服务。
本轮原生场景另行执行，不并入326通过。

最终release候选持久补报1通过、0失败。
耗时146.378秒，五个维护阶段均为true。
原生启动5次，放行时共有11条待发清单。
原2批次、3清单在真实事务ACK后删除源。
提交前原字节保持，未确认操作终态保持。
文件权限保持；数据库受限角色标记仍为false。
随机数据库已由独立查询确认不存在。
最终回环relay与Mac反向转发已撤销。

同一最终候选的原生生命周期1通过、0失败。
耗时98.171秒，实际建立14个mTLS连接、4批日志。
五个维护阶段全部通过。
服务restart与stop后，长任务后代退出。
重连后后续任务完成，未确认日志得到确认。
随后独立核对服务、账户、进程与测试目录均不存在。
1444个远程源码文件再次核对，无差异。
本机产品源码与最终冻结快照一致。

最终脱敏归档为`final-native-evidence.tar.gz`。
大小341,345字节，SHA-256为：

```text
e1bec0d3de9ace0dbf1cc7cc2d3e7d7b6fe9f9bdc02b61ee55ea28a73ebc82d0
```

归档已下载到上述本机证据目录并核对摘要。
两组阶段、独立补报、最终测试及清理JSON均重新读取。
最终数据库与relay清理另保存在同目录独立JSON。
