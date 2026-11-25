<!--[INPUT]: 用户批准的统一安装弹窗与系统设置地址配置。
[OUTPUT]: 固定首次安装接口、交互与验证范围。
[POS]: 节点加入流程的实施依据。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 节点安装与加入实施计划

目标：在同一弹窗选择节点池、系统、架构和固定版本，生成安装命令与一次性凭证。
设备公开地址由系统管理员在设置中保存。复用签名版本、私有 S3 和制品网关。
当前工作区既有终端与 Windows 清理修改保持原状。不安装宿主服务，不部署或提交。

## 接口

- `GET/PATCH /api/v1/system/node-installation`：`{device_endpoint,can_edit}`。仅真实系统管理员修改全局地址；租户管理员读取。地址格式为 `host:port`，IPv6 使用方括号；拒绝协议、路径、凭据和控制字符。
- `GET /api/v1/node-installation/options`：`{device_endpoint,releases:[{id,version,platforms:string[]}]}`。只列当前租户导入的版本，平台为 `linux/amd64` 等六种组合。
- `POST /api/v1/node-installation`：输入 `{pool_id,release_id,os,arch}`。准备 CA、可信发布公钥、固定资产及网关票据后，最后生成一次性凭证。
- 返回 `{token,expires_at,device_endpoint,ca_pem,ca_sha256,version,os,arch,download_url,sha256,size,public_keys,installer_url,installer_sha256}`。响应禁止缓存。使用窗口至多 15 分钟。凭证截止不晚于下载票据，锁等待不延长截止时间。
- `GET /api/v1/node-installation/installer.sh` 和 `installer.ps1`：返回仓库内固定安装脚本，不包含凭证。脚本摘要随生成结果返回，命令先下载校验再执行。
- 安装脚本使用参数传公开材料，凭证只使用 `ARTIFACTORY_ENROLLMENT_TOKEN` 环境变量。Shell 参数：`--download-url --sha256 --size --endpoint --ca-base64 --ca-sha256 --public-keys --os --arch`。PowerShell 对应 PascalCase 参数。

## Task 1：持久配置与安装材料

- [x] 覆盖地址校验、权限、跨租户、缺失材料、固定资产、密钥轮换和过期时间测试。
- [x] 新增兼容追加迁移及系统配置存储，系统管理员权限查真实账户角色。
- [x] 新增安装 API，重新核验版本签名、摘要与平台，复用网关下载票据。
- [x] 更新 API/store/migration 地图并执行 Rust 与数据库测试。

## Task 2：安装脚本

- [x] 隔离执行失败测试，验证平台、摘要失败、身份保护与凭证清理。
- [x] 编写 Shell 与 PowerShell 安装器：校验系统/架构和材料，准备私有目录，以专用账户注册、初始化 OTA，再安装和启动服务。
- [x] 已有身份或服务立即停止；失败保留身份以便诊断，不重新注册。
- [x] 安装命令提示先准备专用账户与 SDK；系统授权和服务账户密码仅在目标机输入。
- [x] 语法检查和隔离执行不改变开发机账户或服务。

## Task 3：统一弹窗与设置

- [x] Playwright 覆盖统一流程、失败、倒计时、修改选项和迟到响应。
- [x] 系统设置的 Emissary 分区添加连接地址；系统管理员可保存，其他管理员只读。
- [x] 独立加入弹窗组件，保留同一弹窗，提供平台/架构/版本/池选择、命令复制和折叠凭证。
- [x] 生成后锁定选择；过期和重新生成期间禁用复制；修改选项清除材料；关闭或租户切换清除秘密。
- [x] 缺少地址、版本或节点池时提供设置引导；失败保留选择并通过 notify 反馈。
- [x] 中英文、320px、键盘、首读骨架、顶部进度与后台静默遵循现有模式。

## Task 4：集成验证与文档

- [x] 独立审查凭证、权限、Shell 转义、CA 信任和重复执行边界；修复三项 P2。
- [x] 执行 Rust 专项、数据库验收、前端类型检查、生产构建与浏览器专项。
- [x] 更新使用文档、L3/L2，并记录三系统真实安装仍需实机验收的边界。

## 本地验收 · 2026-11-25

- Rust 安装单元 2 项，真实 PostgreSQL/HTTP 5 项，兼容基线 3 项通过。
- 安装器隔离执行 12 项、`sh -n`、ShellCheck 通过。
- PowerShell 解析和内嵌 C# 编译通过。生成命令使用桩验证摘要拒绝、字面传参、环境清理与进程策略恢复。
- Web 类型检查和生产构建通过。生产模式安装与 OTA 回归 54/54 通过，覆盖三视口、中英文明暗主题、320px 和 WCAG AA。
- 数据访问、模块依赖、文档导航与生成链接检查通过。
- 新增 Unix 隔离执行与 Windows PowerShell 语法的 CI 入口。

未提交、推送或部署。未创建宿主账户，未安装宿主服务。
三系统的真实注册、服务启停、OS 重启、SDK 和签名权限仍需实机验收。
