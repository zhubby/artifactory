<!--
[INPUT]: Runner保存故障、mTLS回归及Linux原生和ext4证据。
[OUTPUT]: 记录健康与排空确认保存失败的进程保全修复和验证边界。
[POS]: OTA运行程序侧的增量验收；不覆盖物理断电或其他系统服务。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary runtime 健康保存故障验收

日期：2026-11-26。
此前[邮箱与请求修复](2026-11-26-ota-requests.md)之后复查Runner。
发现健康确认和排空标记的保存失败仍会终止runtime。
本轮修复此路径，并补充重连前会话围栏保存保护。
Mac mini和Windows既有服务保持不变。
没有提交、推送、发布或部署。

## 复现与修复

`State::command`原来直接传播`ota::confirm`和排空保存错误。
Runner主循环通过`?`传播，导致运行程序退出。
此外，保存前已更新`health_at`，失败也消费了确认。
重连前写空健康确认存在相同错误传播。

两项新增回归均使用真实文件路径故障，先失败再修复。
把`health.json`或`drained.json`替换为目录，使原子替换失败。
原实现的两个用例退出码均为101。
断言分别为health/drain persistence failure must not terminate runtime。

修复保持当前进程，未保存成功前不更新`health_at`。
相同未消费确认和后续有效确认均可重新保存。
排空状态读失败同样等待，不生成排空标记。
重连先保存空健康会话围栏，成功后才改变内存会话并建连。
围栏保存失败时每秒重试，不带旧会话确认建立新连接。
仍要求当前有效心跳、同会话三次确认及原120秒健康期限。

## 真实进程与本地验证

新用例`ota_runtime_health`运行真实Runner及mTLS对端。
先等操作日志落盘，确认Runner已进入启动流程。
初始健康路径故障持续1.5秒，进程保持且TCP连接数为0。
解除故障后建立mTLS连接并保存有效确认。
再次阻塞健康保存，连续两个心跳期间进程保持。
恢复路径后，同一会话保存后续确认。
随后阻塞排空标记，保持draining且同一连接继续心跳。
解除故障后保存正确update ID的排空标记。
最终PID和会话不变，SIGTERM正常退出。

macOS本机进程回归通过，耗时42.65秒，mTLS连接数为1。
Runner OTA单元8项通过。
Emissary库153通过、0失败、2忽略。
请求5项、supervisor12项、runtime8项集成均通过。
supervisor实际执行120秒健康超时，耗时125.30秒。
本地Clippy、格式和diff检查通过。

首个进程夹具错误地提供null stdin，触发runtime EOF退出。
改用保持打开的管道后重跑通过，不把该次失败计为产品缺陷。
中间源码的Clippy报告collapsible_if，合并条件后通过。

## Linux源码与验证

环境沿用[Linux原生记录](2026-11-26-linux-native.md)的arm64虚拟机。
首轮冻结源码SHA-256为：

```text
fbeb360006c1a5057d421f1240f317b51cf5f7496781aa4936ac11ede4931d50
```

该轮38套件共335通过、0失败、16忽略，耗时390.190秒。
包含新健康保存进程回归、真实120秒超时及100秒积压公平发送。
首轮Clippy仍含上述条件风格问题，不记为全绿检查点。

最终源码含1448个文件，清单SHA-256为：

```text
b3c6c8e7123d1673f0c8ab087bd6939f9aeb1a3b817d17422c45a610951cb4c9
```

最终快照只修改条件风格、测试启动就绪与连接断言及文档。
没有在首轮后改变产品行为。
最终Linux库153通过、0失败、2忽略，耗时13.208秒。
新健康进程目标3项通过，命令耗时44.262秒。
Linux Clippy零警告，耗时3.572秒。

release候选为0.2.3 linux/arm64，13,842,672字节。
报告protocol=1和基线提交16535fdf1f7b2f10baaab36d58fb666d73903118。
候选SHA-256为：

```text
8be53f9bc51610d9b8f69156557b221c94e2ee954df6a920c862ad6694e3034f
```

这是未提交工作树的验证候选，不是正式签名发行。
最终文档晚于上述源码冻结，不宣称同一完整快照。

## 新候选原生与磁盘复核

上述最终源码与release候选实际通过两项原生组合。
生命周期耗时95.032秒，持久补报耗时151.275秒。
两项均包含维护、不同资产runtime OTA及失败恢复。
此处恢复使用同源码验证资产，不扩展旧功能兼容范围。
维护前后账户、ACL、身份及原队列字节保持。
runtime OTA保留launcher PID和实例，候选获得三次健康确认。
generation只推进一次，runtime startup改变。

持久补报由仍运行的系统服务完成，未另启发送器代发。
五个原生startup已注册，放行时有12份待确认清单。
真实事务锁阻止提交时，原批次和清单仍在。
释放锁后，两个原批次和三份原清单均确认并删除源文件。
操作终态保持，数据库与原生测试资源清理成功。
数据库受限角色不在此组合的证明范围。

同一源码随后通过ext4完整磁盘组合，耗时82.976秒。
低空间、预检后真实ENOSPC和只读挂载均阻止安装。
supervisor在满盘时保留当前runtime和保护资产。
释放空间后保存失败终态，原队列字节保持。
持续采集观察到两次磁盘错误，磁盘计数在重开后保持。
释放空间后，同一Handle恢复采集并确认新样本。
原数据的ID、摘要和时间均由真实事务回执读回。
该组合运行普通进程，不证明原生服务满盘或物理断电。
ext4测试二进制SHA-256为：

```text
07c9f74aef25ca9627bfd514030a00c2e2e8cf80028c677f78d3a0ce8639c68f
```

独立读回确认服务not-found/inactive、账户与服务目录不存在。
没有Emissary遗留进程、私有挂载或loop设备。
本轮镜像已删除，自有数据库均不存在。
临时relay与反向转发已停止，监听端口消失。
共享TimescaleDB与S3测试容器继续运行。
私有诊断目录与编译缓存保留，不称为全部临时文件已删除。

## 边界与证据

本机证据目录为`/tmp/emissary-runner-health/`。
远端工作目录沿用`/home/codehub/emissary-evolution-r90_6pfy/`。
首轮结果为`health-linux-tests-result.json`。
最终库、进程与Clippy分别为`health-final-*-result.json`。
`health-final-candidate.json`记录实际release文件摘要。
本地观察摘要为`local-observed-results.json`。
Linux安全归档为`linux-health-evidence.tar.gz`。
归档排除生成私钥、私有配置及完整源记录。
下载后验证58份成员摘要，并重新读取结果与清理证据。
归档SHA-256为：

```text
22fb0bcf6a34c75f5ae51321ab36279fa79369728778b7e8a38392664b889d00
```

解包目录为`linux-evidence/`。
`run/health-native-*-result.json`记录原生耗时和候选摘要。
`run/health-ext4-evidence.json`保存磁盘阶段证据。
`run/health-independent-cleanup.json`保存Linux独立清理读回。
`local-cleanup-readback.json`保存数据库与转发清理读回。
当前工作树产品和测试文件与冻结清单一致，后续文档单独更新。
临时证据不是长期归档或正式发行材料。
健康路径故障证明保存错误边界，不等于健康文件真实满盘。
代理未收到审查任务正文，不计为独立审查。
macOS、Windows原生维护、远程CI及统一部署仍待执行。
