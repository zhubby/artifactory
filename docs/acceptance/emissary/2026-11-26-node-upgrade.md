<!--
[INPUT]: 0.2.5签名发行、集群读回、三节点维护及真实构建回执。
[OUTPUT]: 记录统一升级、旧程序清理、身份保留与恢复接单结果。
[POS]: 本轮交付的现场结论；历史故障证据仍以各自快照为准。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Emissary 0.2.5 统一升级

日期：2026-11-26。
三台现有构建节点已完成launcher和runtime升级。
旧程序已清理，平台已恢复接单。
三台真实原生编译与网关制品下载均通过。
脱敏结果见[结构化回执](2026-11-26-node-upgrade.json)。

## 发行与平台

固定产品源码为`e38f5b25a227245ef7cfd5811d8f43dc2ef64125`。
统一产品版本为`0.2.5`。
[正式发行](https://github.com/zhubby/artifactory/releases/tag/emissary-v0.2.5)
来自不可变标签`emissary-v0.2.5`。
[发行CI](https://github.com/zhubby/artifactory/actions/runs/45801557257)通过。
六平台最终资产均通过原生服务门禁。
Windows x64安装protoc时遇到HTTP 504，补跑通过。

八份发行资产已独立核验签名、大小、平台、来源及摘要。
清单SHA-256为
`1af16039c620b63e12fa55ff26ff7c3ec67fb36ee4f2ae0b16856768d4c82c18`。
GitHub匿名API限流后，导入使用临时认证引用。
导入结束已删除临时Secret与网络策略，并恢复Control定义。

Control三副本、Telemetry和Web使用固定摘要镜像。
Control与Telemetry镜像摘要为
`2c3272bb16a58b40c1b0add284dc7d2f433f6822fee80542dd1efaa406044f08`。
Web镜像摘要为
`0966ac7c8b7c9fe2091ce0121da2c3aab8a8774fde730ec787548d110d9c4d25`。
三个Control副本均完成临时节点池的创建、读取、改名及删除。
业务数据、安装设置、发行信任与原Secret保持。

升级期间网关暂停新构建接单。
节点验收后只关闭该暂停开关。
首次Helm更新遇到此前补丁持有的字段归属冲突。
核对其他资源不变后，精确修正该字段并重新收敛。
最终Helm revision为44，状态为deployed。
两个网关副本的暂停开关均为false。

## 逐台升级与清理

用户授权卸载旧功能版本，不保留旧版本回退。
维护先停原服务，再记录身份和未确认队列。
保留原UUID、配置、证书、服务账户与服务定义。
launcher先更新，runtime由管理员显式OTA更新。
候选未通过健康确认前，原current仍受保护。

| 节点 | 平台 | launcher / runtime | 清除旧程序 | 复扫旧程序 |
|---|---|---|---|---|
| Cassiopeia | macOS arm64 | 0.2.5 / 0.2.5 | 11份 | 0份 |
| Mac mini | macOS arm64 | 0.2.5 / 0.2.5 | 4份 | 0份 |
| LAPTOP-VFIEIL8E | Windows x64 | 0.2.5 / 0.2.5 | 5份 | 0份 |

复扫覆盖各节点既定安装树和已盘点历史程序。
每台保留两个匹配正式摘要的程序：launcher和current。
旧previous引用已清空，配置、证书和队列未删除。
三台停机时均没有待确认batch或操作记录。
Windows的遥测统计文件保留。
此结果不证明三台本次维护发生过积压补交。
积压与满盘恢复由独立故障门禁证明。

Cassiopeia沿用既有用户LaunchAgent。
Mac mini的原生维护入口拒绝既有plist格式。
根因是账户检查依赖紧邻的XML字符串。
实际账户为`artifactbuild`，服务路径正确。
本次保持原plist，用受锁保护的原子替换维护launcher。
身份、服务定义、权限和新鲜健康均再次核对。
该入口的plist格式限制仍需后续修复。

Windows使用正式`launcher-maintain`入口。
维护核对原账户、服务定义、ACL和新鲜健康。
清理脚本的.NET空备份参数首次被转换为空字符串。
当次未删除旧程序，原服务自动恢复。
独立文件验证可空参数后，核对原状态并完成清理。

## 功能读回

三台均在线，launcher与runtime均为0.2.5。
OTA记录均为succeeded，重复请求保持幂等。
过期版本确认与同版本升级请求均返回409。
三台均上报当前startup的原生清单及新鲜主机遥测。
Git、Cargo、Node有限诊断均成功。
真实终端均验证账户、完整随机标记与退出码0。
会话已关闭。
Windows客户端响应ConPTY光标查询后正常执行。

完整刷新保持真实失败语义：

| 节点 | 完整刷新 | 实际原因或读回 |
|---|---|---|
| Cassiopeia | failed | xcodebuild探测返回exit_failure；未发布新清单 |
| Mac mini | failed | jarsigner、java、keytool探测失败；未发布新清单 |
| Windows | succeeded | 同observation与revision已送达 |

三次刷新均保持启动metadata与调度能力不变。
两台macOS的工具探测失败不记为刷新成功。
本次未安装或更改宿主SDK。

## 恢复接单后的真实构建

每台建立独立验收项目，只授权该节点。
源码固定为`d56401a5c86547db0d27456b4737b478fde9376c`。
使用既有源码凭据引用，不在URL中写入凭据。
任务读取README，用已安装rustc编译小程序并执行。
验证文件包含实际程序输出、Git提交与rustc版本。
没有发布应用Release。

| 节点 | 构建ID | 结果 |
|---|---|---|
| Cassiopeia | b0184d02-60cf-4291-87d2-a3b65e4f324a | succeeded，2份制品 |
| Mac mini | 23b458c3-764c-4fe6-80dc-d1768b027640 | succeeded，2份制品 |
| Windows | 48abe8f9-2b33-495e-9e92-887023ef6f8c | succeeded，2份制品 |

成功attempt均属于对应授权节点。
六份制品经真实网关下载，大小与SHA-256全部匹配。
压缩包内验证文件与独立下载的文件一致。
两份Mach-O均为arm64，PE为x64。
程序均实际输出`EMISSARY025_NATIVE_OK`。
这是原生执行烟雾验收，不表示完整业务应用验收。

## CI与证据边界

[产品源码CI](https://github.com/zhubby/artifactory/actions/runs/45801385116)
补跑的六平台原生、Control OTA和Kubernetes门禁通过。
Linux综合任务在45分钟上限取消。
前端共有1230项测试，取消前执行到第458项。
取消前已有19项失败，不能记为通过。
日志测试缺少显式打开“执行”标签。
首读表单和元数据分组测试也未同步界面合同。
本次后续提交修正这些测试，并独立前端CI任务。
产品源码、签名发行和现场二进制不变。
首次Linux arm64满盘门禁未观察到两次采集失败。
相同源码的补跑和正式发行门禁均已通过该场景。

Linux arm64虚拟机的系统服务和真实满盘证据仍有效。
本轮六平台CI与三台现有设备分别记录。
macOS/Windows原生持久补报、物理断电、长期离线
及长期容量观测仍没有本轮证据。
两台macOS的完整探测失败与plist维护限制已明确保留。
本轮自有S3容器已核对所有权并移除。
共享测试数据库和共享S3未停止或删除。
