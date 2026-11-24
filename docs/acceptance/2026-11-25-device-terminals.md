<!--
[INPUT]: 本地源码测试、真实 macOS PTY、多副本 TLS 和浏览器证据。
[OUTPUT]: 区分设备终端源码验证、环境验证和未验收目标。
[POS]: 本次设备终端的交付证据；不声明生产部署。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备终端本地验收 · 2026-11-25

基线为本地 main `6ca171b`，实施分支为 `codex/device-terminals`。只覆盖控制台打开被控设备终端。本记录先描述合并前的本地阶段；该阶段未提交、推送、部署或安装真实设备。后续获授权的合并与 Kubernetes 验收单独记录。

## 已验证层次

| 层次 | 证据 | 结果与边界 |
| --- | --- | --- |
| 共享协议 | `emissary-protocol --test terminal` 3项 | 帧尺寸、偏移、身份与过期校验通过 |
| 真实 macOS PTY | `execution-support --features terminal --test terminal_process` 14项 | TTY、Ctrl+C、resize、中文、真实退出码、信号、后台后代、父Worker强杀、慢读取消和注册期限通过 |
| Linux容器PTY | 本地Docker VM linux/arm64、真实PTY 14项 | 与macOS相同源码和测试通过；不等于systemd服务账户或物理Linux验收 |
| 真实 Emissary | `emissary --test terminal_protocol` 4项 | 真进程+mTLS+PTY，设备租约回收、输入缺口不重放和立即退出7的profile重复4次通过；Control为测试peer |
| Emissary状态机 | `emissary --lib` 47通过、2忽略 | 含OTA终端排空与取消/失败后准入恢复；忽略项不是终端验收 |
| 三副本与权限 | `control-plane --test database terminal -- --ignored --test-threads=1`，21项通过 | 真实PostgreSQL、独立TLS端口、原Session权限与内部签名流；设备字节peer为模拟端 |
| 大量小帧恢复 | 1200×512B积压、单次附着、exit7 | 完整offset、字节、真实退出码、实际账户和审计计数通过 |
| 授权故障 | 直连/三副本慢读撤权、owner停止、DB锁等待 | 授权硬截止不依赖满输出队列，取消不再排放旧数据 |
| 租户生命周期 | pending、曾附着及刚关闭终端 | 首次drain保存50秒安全期限；不刷新期限，项目删除不关租户终端 |
| 网关 | `python3 tests/images/terminal_gateway.py` | 真Pingora升级、中文、16KiB二进制、ping、独立配额及32秒空闲存活通过；上游为隔离echo peer |
| Web自动回归 | 39个终端case、16个既有设备/路由case | 桌面、平板、320px手机、权限、首帧、粘贴、焦点和恢复；业务传输为fixture |
| 浏览器完整链 | Chromium→Pingora→Control→真实Emissary→macOS PTY | 登录后显式创建、实际账户/目录/系统、TTY、中文、Ctrl+C、resize和exit7通过 |

浏览器完整链使用独立schema、bucket、loopback端口和临时设备注册。Shell使用当前开发账户；没有用专用launchd服务账户。T3 preview连续两次snapshot超时后，按工具规则切换agent-browser，完成交互及截图检查。物理输入法不由composition事件替代。

真实浏览器执行 `test -t 0 && test -t 1` 得到 `REAL_PTY_OK`。窗口从18×125调整为18×109，并由设备 `stty size` 确认。`sleep 120` 被Ctrl+C中断，后续命令继续执行。中文composition输入到达真实PTY并正确显示。`vi`进入Vim全屏画面，Escape和`:q!`返回Shell。`exit 7`显示`shell_exited`及退出码7。检查1280px、920px及320px布局，无页面横向溢出。关闭真实浏览器WebSocket后重新附着，前后Shell PID均为25055；恢复继续执行，退出码7保留。最终Control二进制与生产Web构建重新启动后，浏览器经同一网关链执行TTY检查得到FINAL_PTY_OK，并正确排空尾部和显示退出码7。

## 评审与修复

按ce:review只读模式检查正确性、可靠性、安全、规范、测试、维护性、对抗边界、接口、迁移、性能、TypeScript、异步界面与agent可达性。已修复以下问题：

- PTY排空失败不再先发正常退出；清理任务失败和超时有明确结果。
- 本地注册握手阻止快速退出抢先reap。Darwin僵尸身份使用有界系统查询核对真实出生时间，避免PID复用误杀。
- 恢复积压采用渐进发送，正常退出等待已消费字节，权限撤销立即丢弃队列。
- 各跳独立授权截止取消挂起发送，慢读不能拖延撤权。
- 租户删除先关闭终端并等待安全期限，再删票据与会话，避免孤儿记录毒化清扫。
- OTA按实际阶段控制准入，取消和失败后允许新会话。
- 短断丢失输入帧后允许严格递增序号缺口；旧序号去重，不重放遗漏输入。
- ACK突发等待有界通道容量，慢发送随授权续期更新deadline。
- owner设备出口采用有界outbox，合并ACK、resize和lease，避免暂满丢ACK或误关会话。
- 审计保存实际账户和字节计数，测试确认不存输入输出内容。
- 新增文件补齐地图，拆分本次新增超800行的传输测试与Runner。

## 10会话负载

使用10个虚拟mTLS设备、10个登录用户、三套独立Control状态和TLS监听，位于同一测试进程。每会话输出2MiB，共20MiB；5个快速消费，5个每帧延迟15ms。最大未确认窗口1,048,576字节，5个慢消费者均触发credit暂停。

最终全组通过的debug样本中，30次输入回显RTT P50为793.7ms、P95为2123.1ms；434次独立Control心跳P50为64.1ms、P95为927.4ms、最大1072.0ms。关闭请求到设备关闭帧最大1980.1ms，到Control关闭帧最大1980.7ms。测试进程RSS从60,932,096字节增至采样最大99,614,720字节；整测试库连接采样峰值13，活跃峰值7。设备模拟端传输字节和传输耗时另见可复现测试，不能视作真实设备出口带宽。

持久摘要见[负载证据](2026-11-25-terminal-load.json)，原始样本由测试写入`.local/terminal/load-report.json`。该测试没有PTY、网关或真实构建；心跳与关闭帧证明应用队列独立，不能证明设备CPU/网络隔离或生产SLO。

## 合并前复审

用户授权合并与部署后，复审补充三个回归并先观察失败：授权查询旧MVCC快照晚返回不能延长五秒窗口；满1MiB窗口的最后ACK丢失后，原渲染器附着偏移释放credit；浏览器入口到owner内部流中断使用1012恢复，原Session与设备流保持。授权截止改为查询开始时间加五秒。恢复偏移单调推进已消费位置，旧偏移不回退。明确退出与撤权继续终态。建流首帧的临时失败同样使用恢复码，每次重试签发新票据。转发停止后的关闭记录写入失败按退避重试，直到持久化成功或owner停止；重试不恢复转发或设备续租。

最终合并前21项终端数据库回归全部通过，Control全目标Clippy和终端API单元测试通过。

同时检查真实试用策略的允许规则并集。宽泛入站规则排除Control，让专用策略约束8446；新增Helm回归由失败转为通过，完整28项通过。

## 未验收边界

Windows原语的MSVC交叉检查与真实ConPTY运行是两层证据。完整Emissary的Windows交叉构建遇到本机Windows SDK缺少`assert.h`，未通过；不能由PTY库交叉检查代替。尚未在Windows x64专用服务账户、Session0和嵌套Job环境验证ConPTY、全屏程序、原生IME与强退回收。

原生Linux systemd服务账户、macOS专用launchd服务账户、真实远程设备、Kubernetes1.28与当前支持版本的部署验收单列。现有环境中的历史成功记录不构成本功能验收。

10会话负载使用模拟设备字节端，独立记录在途窗口、延迟和资源；不会当作真实PTY吞吐、真实构建并行或生产容量结论。大输出仍与设备构建共享CPU、网络和磁盘。真实构建续租/构建取消与终端大输出并行、真实设备出口带宽仍待环境验收；Control健康心跳与终端关闭帧不能替代这些证据。

## 源码与构建检查

Emissary库47通过、2项既有忽略；真实Runner生命周期6项通过；共享终端协议3项通过。终端原语、Emissary、协议与Gateway的all-targets Clippy在`-D warnings`下通过。Control单独执行all-targets Clippy。`cargo fmt --all -- --check`与统一版本0.2.0检查通过。

39项Web终端case已在最终界面重跑通过，既有16项设备与路由case在本轮实现中通过。Next生产构建、TypeScript检查和mdBook构建通过；mdbook-mermaid仅提示既有工具版本差异。临时Next distDir配置已恢复。既有Gateway完整回归通过，覆盖TLS监听、对象Range/HEAD、Manifest发布围栏、配额与POST不重放。

## 复现与环境

遵循[设备终端](../terminals.md)运行PTY、Emissary和网关测试。数据库测试需要隔离的`ARTIFACTORY_TEST_DATABASE_URL`及本地测试S3设置，不能指向生产库。Helm用三个Control副本渲染，私有8446端口不进入公网Service。

界面依赖仅新增固定版本的xterm与fit addon；第三方许可证登记在`web/THIRD_PARTY_NOTICES.md`。终端迁移为兼容追加，不重置main基线或原工作区规划文件。
