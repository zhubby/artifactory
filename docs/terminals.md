<!--
[INPUT]: 设备 PTY、登录 Session、终端票据与多副本认证流。
[OUTPUT]: 定义设备终端配置、交互、输入背压、权限与回收边界。
[POS]: 设备终端使用入口；验收证据独立记录。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 设备终端

在租户的构建节点详情中进入“运维”，选择“打开终端”。独立页面显示设备名称、系统、架构、实际账户、目录和状态。选择本地配置的 Shell，再点击“打开终端”。在线设备立即创建交互会话；离线设备不排队。

Shell 在被控设备的 Emissary 账户下运行。Control 只认证和转发字节。设备使用出站 mTLS，不新增入站端口，也不需要 SSH。此功能不提供 Control、容器或宿主机终端，不包含 P2P 和端口转发。

## 设备配置

设备本地默认关闭。向 Emissary 私有配置文件加入以下设置，再按现有服务流程重启：

```json
{
  "terminal": {
    "enabled": true,
    "max_sessions": 2
  }
}
```

Unix 默认 profile 为 `/bin/sh -i`。Windows 默认使用系统 Windows PowerShell，参数为 `-NoLogo -NoProfile`。Windows ConPTY 要求 Windows 10 1809 或 Windows Server 2019 及以上版本。PTY 创建失败会明确报错，不降级执行构建脚本。

需要更换 Shell 时，在设备本地设置 profile。浏览器只提交 profile ID，不能指定命令路径、环境或目录：

```json
{
  "terminal": {
    "enabled": true,
    "max_sessions": 2,
    "cwd": "/srv/emissary/terminal",
    "profiles": {
      "default": {
        "label": "Shell",
        "shell": "/bin/sh",
        "args": ["-i"]
      }
    }
  }
}
```

`cwd` 必须是已存在的绝对目录。省略时使用 Emissary 私有根下的 `terminal-workspace`。启用时校验 Shell 是已存在的绝对文件。最多配置 8 个 profile，`max_sessions` 为 1 或 2。Shell 清空继承环境，只保留运行账户的必要路径、临时目录和语言变量。配置变更通过新设备启动快照生效。

macOS 服务环境没有设置 `LANG`、`LC_ALL` 和 `LC_CTYPE` 时，终端使用 `LANG=en_US.UTF-8`，保证交互式 Shell 接收中文输入。设备显式设置任一语言变量时保留原值。

终端不是项目沙箱。同账户能够访问的配置、私钥和其他构建目录也能从终端读取。设备服务仍使用已有非 root、非 LocalSystem 安装约束。

## 权限与票据

仅当前租户 owner/admin 的登录 Session 能创建终端。项目角色、普通成员、API Token 和旧的全局账户角色均不授予设备 Shell 权限。

会话绑定创建者、原登录 Session、设备 startup/control session 和页面内存中的 renderer ID。其他管理员、同账户的新登录、刷新页面或新标签均不能接管。退出登录和权限撤销会停止输入输出并回收会话。

附着票据有效期 30 秒，只存摘要并原子消费一次。浏览器在 WebSocket 第一帧提交票据；URL、localStorage 和日志均不保存票据。升级须具有与 `ARTIFACTORY_PUBLIC_URL` 完全匹配的 Origin，包含 scheme、host 和 port。升级后 5 秒未完成认证即关闭。

Control 最多每 5 秒重新确认转发授权。数据库挂起、账户撤权、证书失效或目标身份变化均会截止转发；队列中旧数据不能绕过截止。网络分区后，设备最多等待本地 45 秒租约，再终止 Worker。

## 交互与恢复

支持字节输入输出、Ctrl+C、Escape、Ctrl/⌘K、窗口调整和 ANSI 全屏画面。焦点在终端时快捷键交给 Shell。按 Ctrl+Shift+Space 将焦点移回工具栏。多行粘贴先确认；OSC 52 写剪贴板被禁用。

同一页面保留原渲染器时，网络短断允许在 30 秒内恢复。输出按字节偏移补发，xterm 消费后确认。输入从不自动重放。刷新、离开页面和切换租户结束会话，画面不恢复；新会话必须显式创建。

仅真实 PTY 建立且父监督器登记成功后显示“已连接”。结束时显示真实退出码或信号。传输、授权、背压和排空失败有独立原因，不能解释为正常退出。

逐字符输入通过有界队列背压，不因短暂满载断开。输入按顺序交给 PTY，不自动重放。设备传输等待受取消、租约和三秒拥塞截止约束。PTY 写入等待继续监督取消、输出和进程退出；写入停顿超过三秒即回收会话。

## 预算与回收

| 边界 | 预算 |
| --- | --- |
| 未附着 / 启动 | 30 秒 / 15 秒 |
| 同渲染器重连 / 慢消费 | 30 秒 |
| 设备租约 / 续期 | 45 秒 / 10 秒 |
| 输入空闲 / 最长会话 | 15 分钟 / 2 小时 |
| 每账户 / 每设备 | 2 个活动会话 |
| 每 Control owner / 入口流 | 64 / 128 |
| 数据帧 / 未确认输出 | 16 KiB / 1 MiB |
| Gateway 终端流 | 每进程 128，独立于普通 HTTP |

`ARTIFACTORY_GATEWAY_TERMINAL_CONNECTIONS` 配置网关终端配额；Helm 对应 `gateway.terminalConnections`。Unix 回收 PTY SID 内的前后台进程；Windows 使用 ConPTY 和 Job Object。父管道 EOF、设备流中断、取消、停机和租约失效都触发回收。正常退出先有界排空输出，消费确认后结束；撤权立即丢弃未发送数据。

OTA 实际处于 downloading、draining 或 activating 时停止终端准入并关闭现有会话。取消、下载失败或回退后允许新建，已经关闭的会话不复活。终端不占构建 slot，但仍与构建共用设备 CPU、磁盘和出口带宽。

租户归档或删除先停止终端。曾附着的会话保留关闭记录，并等待 50 秒授权与设备租约安全窗口，再删除租户资源。项目删除不关闭租户设备终端。

## 多副本与运维

每个终端固定当前设备控制连接所在的 owner。浏览器入口、设备终端入口和 owner 能分别落在三个副本。各实例在 PostgreSQL 登记短租约、临时 TLS 证书和 Ed25519 公钥；内部委托绑定实例、会话和身份，并防重放。

内部端口默认绑定 loopback 随机端口，适用于单进程开发。多实例设置 `ARTIFACTORY_TERMINAL_RELAY_BIND` 和 `ARTIFACTORY_TERMINAL_RELAY_HOST`；显式 `ARTIFACTORY_TERMINAL_RELAY_ENDPOINT` 只能指向该实例的固定 HTTPS 地址。Helm 注入 Pod IP 与私有 8446 端口，NetworkPolicy 仅允许 Control Pod 访问。公网 Service 不公开此端口。NetworkPolicy 按允许规则求并集；试用命名空间的宽泛入站策略必须排除 Control，再由专用策略限定 8446。只新增专用策略不能覆盖旧的全端口放行。

owner 故障终止旧会话，不迁移 Shell。浏览器入口失效允许原页面恢复；设备终端流或其内部转发流失效则关闭，避免重启第二个 Shell。实例地址复用不能替换原实例私钥。

追加迁移 `2026112403` 创建实例、会话与票据表。会话与审计保存身份、时间、实际账户、字节计数和结束原因；不记录命令、终端内容或会话录制。终态记录保留 30 天，过期实例保留一天后清理。

回退前关闭设备终端、撤销活动会话并确认 Worker 回收，再更换二进制。保留追加表和审计，不执行破坏性降级迁移。配置与部署步骤仍须遵守现有版本的数据库兼容规则。

本地复现：

```sh
cargo test -p artifactory-execution-support --features terminal --test terminal_process
cargo test -p artifactory-emissary --test terminal_protocol
cargo build -p artifactory-gateway
python3 tests/images/terminal_gateway.py
```

协议、真实 PTY、浏览器、多副本和设备服务账户的证据分别记录于[本次验收](acceptance/2026-11-25-device-terminals.md)。本机测试不代表生产部署或原生 Windows 服务验收。
