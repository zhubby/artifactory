<!--
[INPUT]: 真实监督器故障注入、Control OTA及双文件系统证据。
[OUTPUT]: 记录请求耐久接受、邮箱交接锁和进程保全验证。
[POS]: Emissary演进的OTA请求增量验收，不替代原生服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary OTA 请求可靠性验收

日期：2026-11-26。
所属计划见[演进路线](../../plans/2026-11-25-001-feat-emissary-evolution-plan.md)。
此前候选恢复证据见[增量验收](2026-11-26-telemetry-conflicts.md)。
后续Runner健康与排空保存故障修复见[健康保存验收](2026-11-26-runtime-health.md)。

## 请求保存与取消清理

请求入口此前先删除request.json，再保存接受状态。
apply或cancel状态保存失败时，监督器退出并停止当前runtime。
取消下载后，下载保护释放失败也会退出监督器。
新增三项真实进程回归均先复现该问题，退出码101。
共同断言为request persistence failure stopped supervisor。

修复先克隆状态并执行请求准入，再耐久保存接受结果。
保存失败保留原内存状态、邮箱和当前进程，每秒重试。
保存成功后再中止旧下载，释放保护并消费同一邮箱请求。
清理失败时保留待完成请求，不重复执行旧下载。
启动器重启仍遵循原中断失败语义，不从邮箱自动续传。

apply状态保存故障期间，真实下载端未收到请求。
恢复后，签名候选通过真实HTTP下载并进入draining。
cancel状态保存故障期间，原downloading与cancel邮箱保持。
取消保护释放故障期间，当前runtime持续运行。
解除故障后，canceled保存，保护与原邮箱移除。
第四项回归在清理重试期间提交generation 2的新取消请求。
最终状态绑定新ID及generation 2，没有吞掉新邮箱。
四项均取消并等待监督器退出，读回原runtime正常停止。

## 邮箱交接锁

仅比较请求内容，不能排除读取与删除之间发生替换。
新增回归先复现提交不遵守交接锁，退出码101。
断言为submission replaced mailbox while handoff held the lock。
修复复用现有文件锁原语，新增私有request.lock。
提交、接受与消费同一邮箱均持有该锁。
持锁时提交失败且原邮箱字节保持；解锁后新提交成功。
请求锁独立于整个下载期间持有的storage.lock。
因此下载期间仍可提交取消，不用等待下载锁。

初版请求锁故障会跳过整个监督周期。
新增真实候选回归复现健康提交被无关邮箱故障阻塞。
断言为unrelated mailbox lock prevented healthy candidate commit。
修复只退避邮箱读取，既有候选健康提交和超时仍执行。
request.lock仍不可用时，候选取得三个ACK后提交succeeded。
本证据要求下次统一升级launcher与runtime，不承诺旧端混部。

## 最终本地验证

契约7、下载11、预算1、存储13、请求5、监督器12项通过。
总计49项集成回归，不是全工作区测试数量。
监督器包含真实120秒健康超时，耗时124.81秒。
OTA库5项、Python文件系统驱动13项通过。
定向Clippy、Rust格式及diff检查通过。

| 文件 | SHA-256 |
|---|---|
| ota.rs | `8dc764413ae48981b8f7440016be635d558d4e53ffb2156c18759ea48ff3ecac` |
| ota/storage.rs | `d30874f3c805c9ef77f5d00afbb6b129b9dfe10e5510e39d251d6cd5eadab421` |
| ota_requests.rs | `c0b2e41cda47e78e7f83e280715532417b6cfe38f661f1e73732d27ecd03a99b` |
| ota_supervisor.rs | `49d3a925ee629bbf6e40f03e02fbb87cda2aebd1e96be665d8bdc86ccb193be2` |

表内测试源码摘要对应运行快照，随后仅更新文件契约。
最终Control与双文件系统运行使用同一源码清单：
`fc4c9ec0f3dae8aa575a90843250b54352bb50b6c765843d8a44fba493ed79d0`。
本页补写晚于该源码快照。

真实Control、网关、私有S3和runtime OTA组合通过。
证据为`/tmp/emissary-request-handoff-final-control-ota-isolated-evidence.json`。
组合耗时105.242秒，四个同会话健康ACK及Control成功终态读回。
固定launcher PID和实例保持，候选生成新startup。
候选清单事务、设备ACK和实际403后的冻结URL续签均确认。
起点为当前源码的受控版本号，不证明历史发行兼容。
外层证据为`/tmp/emissary-isolated-s3-itqw8nk2/evidence.json`。
六对象、空bucket、双库、角色与进程清理确认。
独立读回容器与临时标签不存在，共享容器仍运行。

双文件系统默认门禁均在最终代码重新运行通过。
证据为`/tmp/emissary-request-handoff-final-apfs-evidence.json`。
HFS+为`/tmp/emissary-request-handoff-final-hfs-evidence.json`。
测试二进制摘要均为`c0610ac894fd6f0395c066d226de9546f2e9360604fa7ad7026f79abc41ff6bb`。
下载首字节后ENOSPC，真实监督器保持current运行。
释放空间后终态保存并释放保护，原资产保持。
进程退出、镜像卸载与删除均确认。
本次默认模式不含真实队列、数据库或持续采集组合。
先前完整组合保留原源码边界，不计为本次重跑。

## 失败运行与剩余出口

后续Linux原生验收发现运行程序侧的邮箱锁竞争。
`ota::submit`的瞬时锁错误沿Runner返回并终止runtime。
整轮两次超时，单独OTA用例成功不能抵消失败。
锁竞争回归先复现错误，再验证修复。
Runner改用一个内存待提交槽，每秒重试。
较新generation替换旧请求，同代cancel保持优先。
缓存暂时未知时同样保留请求，恢复后才提交。
该修复不将提交前请求报告为耐久接受。
显式持锁的真实mTLS升级与失败恢复另有集成验证。
源码、结果和原生候选边界见[Linux记录](2026-11-26-linux-native.md)。

首版持久化修复的Control组合通过，耗时115.987秒。
证据为`/tmp/emissary-request-persistence-control-ota-isolated-evidence.json`。
后续中间运行修改了测试，源码冻结检查失败，不计为通过。
证据为`/tmp/emissary-request-handoff-control-ota-isolated-evidence.json`。
该次数据库、对象、进程、容器和标签清理已独立确认。
最后固定源码重跑通过，不能用中间运行替代最终证据。

先前共享S3超时bucket的未知状态未由本次成功覆盖。
独立代理未收到正文，不计为独立审查完成。
原生服务、断电、Windows句柄、远程CI与统一部署仍待验收。
没有提交、推送、发布、部署或创建系统账户。
