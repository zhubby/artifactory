<!--
[INPUT]: 正式发行、平台部署、三节点OTA、终端与安装弹窗真实回执。
[OUTPUT]: 固定统一0.2.3交付结果、原身份保留及环境限制。
[POS]: 版本交付记录；关联正式资产、部署来源与实机验收边界。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 统一 0.2.3 与手动 OTA · 2026-11-25

统一0.2.3已部署，三节点Emissary OTA均成功。
三节点终端、原生身份和安装弹窗的核心验收全部通过。
最终健康与清理记录见本文末尾。

## 统一版本与管理员升级

全部一方组件共用 Artifactory 统一版本。
Emissary以该统一版本判断是否需要升级。
版本差异只提示，租户owner/admin确认后才执行。
启动器负责验签、切换、健康检查与失败恢复。
本轮三节点runtime均由0.2.0升级到0.2.3。
固定launcher保留0.2.0，构建元数据不构成新升级版本。

协议见[Emissary OTA](../emissary-ota.md)。
安装见[安装并加入节点](../node-installation.md)。
终端见[设备终端](../terminals.md)。

## 源码与正式发行

产品main：`5b8d69e02b1db4db858de61eaf6f0c99b8fdbcfc`。
origin与ke已精确核对该提交。
不可变标签为`emissary-v0.2.3`。
tag object：`81f54c5fb63eef2093fab1f87466369fe6557ba6`。
旧标签未移动，未复用或改名旧资产。

本轮修复Linux PTY取消阻塞、Darwin退出过渡态登记，
以及Unix Worker取消时并发fork残留导致的日志EOF等待。
修复保留独立SID、出生时间、未回收组长和进程组身份围栏。
正常退出仍排空输出，Ctrl+C继续发送真实0x03。

本机all-targets、Clippy警告作为错误、15项版本测试通过。
版本、模块、数据边界、格式和diff检查通过。
两项Darwin修复独立审阅无阻断。
正式运行[45595618125](https://github.com/zhubby/artifactory/actions/runs/45595618125)的六原生job与publish全部success。
六平台分别完成测试、Clippy、服务生命周期和程序版本身份校验。
七份完整日志已审计，无失败记录。

[Emissary0.2.3正式Release](https://github.com/zhubby/artifactory/releases/tag/emissary-v0.2.3)于10:46:43 UTC发布。
Release ID为457975351。
10:57:11 UTC，六程序、manifest与签名共八资产独立校验通过。
平台、大小、SHA256、统一版本与最终源码全部匹配。
Ed25519签名使用既有公开密钥验证。
manifest SHA256：`4f3fd9c9850bea6872e228b57979100b04674800870f8b9645e1ff5189a94d2b`。

## 平台部署

入口为<http://10.169.1.239:30820>。
Kubernetes1.28.3，namespace为artifactory。
Helm revision41 deployed，九个Deployment就绪。
部署验证时间为10:38:07 UTC。

来源为最终main加已上线节点池overlay。
冻结提交：`91be7d203385c632402329425fe8527167f3443b`。
节点池overlay：`f977d3c8431c3e13d7fcc24945e44b43bf6c1775`。
快照：`e0d3b17932a9fbcbe415d32831b5c0e4918095b81da05a48052bd0d64c32bf98`。
来源逐文件摘要、所有权和Web类型检查通过。

| 服务 | 运行镜像摘要 |
| --- | --- |
| Control、Telemetry | `sha256:a8fc9c95c8b94af7511b511293d2cb173024eea49380fbc7aa6cc73c018bf1c5` |
| Web | `sha256:a81fcaebcad175b84c16e855047814df141b2588db926efa0bd97760fcdd09a5` |

两镜像保摘要发布与匿名读回通过。
三个Control、一个Telemetry和一个Web的实际摘要匹配，重启均0。
其他组件沿用原镜像。
部署前业务、遥测、规划三库均新建备份并完成解码校验。
数据、节点池、设备地址、Secret和非目标Pod守卫通过。
发行信任ConfigMap保持UID、immutable、内容和挂载。
实际变化为三镜像、11个顶层Chart标签和两个checksum。
manifest与审核预览一致，五类越界变更探针被拒绝。
Helm历史保留，未force或盲目rollback。

## 真实导入与出口恢复

首次导入在metadata GET阶段失败。
三个Control到GitHub公开地址的TCP停在SYN_SENT，内部S3可达。
未改产品版本、正式资产、公钥或数据库状态来绕过失败。

本轮临时启用来源限制和认证的CONNECT出口。
仅允许三个正式GitHub主机的443，保持端到端TLS。
三个真实Control先通过407证明路由，再精确增加两个代理变量。
NO_PROXY保留内部服务直连。
管理员原导入API重试耗时79.53秒并成功。
发行ID为`9dd8c02c-145c-4e5c-b3e9-c719aa5af4b2`，状态ready。
导入后约41秒的八次采样均为0更新，三节点仍为0.2.0。
这验证版本差异不会自动触发升级。

导入后Control完整spec恢复为原基线。
临时策略按UID/resourceVersion预条件删除。
relay核对PID身份后停止，专用监听消失。
原信任、Helm41来源与节点身份保持。
稳定公网HTTPS出口仍需环境配置，未来发行导入需要该出口。
已导入的固定资产由平台交付，本次OTA不再依赖临时relay。

## 三节点手动 OTA

| 节点 | 更新任务ID | 完成时间UTC | 最终状态 |
| --- | --- | --- | --- |
| Cassiopeia | `fcbba6f7-4f0f-49b2-aea9-6eb97e65f24b` | 11:19:35 | succeeded |
| Mac mini | `2796f593-c5d0-4e9b-8ddd-ac0780ceee72` | 11:21:28 | succeeded |
| Windows | `c339bf57-ad6d-4781-a284-80e91bf09ef4` | 11:22:33 | succeeded |

三任务按顺序执行，指向同一正式发行，均无error。
Mac mini由真实浏览器“运维→升级至0.2.3→确认升级”发起。
升级前、确认和请求后三张截图保留，未用直接API替代创建。
Cassiopeia与Windows过期版本确认返回409。
重复确认返回原任务ID；三节点同版本再请求均返回409。

Cassiopeia已有终端在OTA请求后结束。
ota_draining期间新终端返回409与device unavailable。
最终聚合确认恰好三个succeeded任务。
三节点均在线、runtime0.2.3、launcher0.2.0。
原UUID和节点池保持，draining与ota_draining均解除。
终端均enabled且online，成功状态如实保留为succeeded。

## 原生身份与运行程序

三节点只读原生核验全部通过，未修改服务。
实际运行PID与正式签名资产路径、SHA256、大小和版本关联。

| 节点 | 原账户 | launcher PID | runtime PID |
| --- | --- | --- | --- |
| Cassiopeia | zhubby | 88567 | 71422 |
| Mac mini | artifactbuild | 41375 | 49419 |
| Windows | emissarybuild | 13108 | 2104 |

CA、证书、私钥摘要与独立升级前基线一致。
原配置、launchd/SCM、UID/SID、程序权限和Windows ACL保持。
固定launcher摘要、上一运行版本、OTA信任与健康回执均通过。
Mac mini原root:wheel启动器0755与UID600配置0600保持。
运行时命令确认最终main提交，健康回执新鲜且无激活中状态。

## 三节点终端与安装弹窗

三节点真实浏览器终端全部passed。
whoami独立输出确认zhubby、artifactbuild与emissarybuild。
命令标记、中文粘贴和真实Ctrl+C均通过。
Ctrl+C打断120秒等待，十秒内后续命令成功。
退出码7如实显示，会话结束。
中文粘贴不代表物理输入法或原生IME验证。

设备公开地址由系统设置配置为10.169.1.239:30853。
API与安装选项读回一致，统一0.2.3的唯一发行包含六平台。
安装命令与一次性凭证在同一弹窗，详情默认折叠。
命令绑定凭证、设备地址、CA摘要、资产摘要、公钥和下载票据。
关闭后材料从DOM清除，明文未写入验收文档。
375×812无页面横向溢出，滚动后完成按钮hit-test通过并实际点击。
两张窄屏截图已视觉复核。
测试unused凭证按tenant、ID、hash精确回收，原五条凭证保持。

当前入口HTTP，secure_context与installer_https均为false。
本轮通过的是材料和弹窗验收，未验证全新设备HTTPS首次安装。
也未将剪贴板权限或既有节点身份保留写成首次注册通过。

## 收尾记录

11:25 UTC最终只读检查确认九个Deployment就绪。
五个目标Pod摘要正确，重启均0，Helm41清单与预览一致。
数据、Secret、信任、设备地址和非目标Pod保持。
唯一发行关联三个成功任务，活动终端为0。
浏览器logout返回200，会话已关闭。
最终原生检查完成后，九个本轮临时文件已删除。
清理覆盖SSH认证、askpass、本机seed副本及临时代理认证材料。
正式GitHub签名Secret、公钥、节点身份、服务和备份保留。
私有证据位于`.deploy/ota-manual-20261125/`。
主要依据为正式八资产验签、ota-final、三终端、native-post、installation-ui-final及出口恢复回执。
认证、签名seed副本、完整安装命令和原始终端内容不进入Git。
