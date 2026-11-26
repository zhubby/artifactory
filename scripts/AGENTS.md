# scripts/
> L2 | Parent: ../AGENTS.md

check_data_access.py: 检查数据库访问边界及依赖。
check_module_boundaries.py: 检查纯规划契约、共享基础设施、镜像执行与 supervisor 的依赖方向。
check_ephemeral_source.py: 只读检查直接 Pod、临时卷与空闲清理；镜像验收复用跨版本 AppArmor 安全规则。
check_emissary_legacy_receiver.py: 导出冻结旧路由或完整旧main，以mTLS和隔离库验证发送器并回收角色。
check_emissary_legacy_sender.py: 监督冻结旧发送与编译进程，定点清理自有库并验证反向版本矩阵。
check_emissary_current_main.py: 监督完整Telemetry与冻结旧发送，验证双库、积压新鲜度和事务确认。
check_emissary_ota_filesystem.py: 读回HFS+/APFS/ext4私有卷身份，验证监督器、故障和队列恢复，精确清理库及镜像。
emissary_linux_filesystem.py: 创建160至1024MiB的ext4私有镜像，按loop及挂载身份读回后精确卸载。
check_emissary_native_disk.py: 在自有ext4卷运行Linux系统服务，验证满盘连续性、事务补报与资源清理。
check_emissary_control_main.py: 验证注册、操作ACK、停旧切换与网关OTA，定点回收S3与数据库。
emissary_safe_cutover.py: 验证停旧迁移、双副本读删与冻结同源码兼容副本的原件补交。
emissary_control_ota.py: 构造同源码旧版本号夹具，经签名导入和真实网关验证固定launcher的runtime OTA。
emissary_ota_fixture.py: 提供只读回环发行API，记录签名清单与六平台字节夹具的真实下载。
emissary_ota_mailbox.py: 只读观察真实续签邮箱，核对冻结请求与回复并仅返回URL摘要。
emissary_ota_processes.py: 在未回收leader身份下清扫自有会话，确认独立runtime组全部退出。
emissary_s3_fault.py: 只对精确自有bucket的一次资产GET注入403，透传真实认证S3而不伪造成功。
emissary_legacy_retention.py: 冻结完整旧程序，围栏真实清扫，读回外键SQLSTATE并核对新增清单引用。
emissary_controlled_refresh.py: 用私有探测夹具和真实事务围栏验证刷新观察确认与启动快照不变。
check_docs.py: 校验 mdBook 导航覆盖与生成结果中的本地链接。
compose_acceptance.py: 记录本机中间件容器状态，验证隔离 Compose 的 HTTPS、鉴权与重启。
compose_probe.py: 从隔离容器验证 HTTPS、会话、租户鉴权和重启。
cross-build-emissary.sh: 检查设备运行程序的跨平台构建。
generate-proto.sh: 生成设备协议代码。
init-deploy.sh: 初始化单机固定账号与随机私有凭据；保留已有配置。
init-helm.py: 按资源前缀生成固定初始账号、随机持久凭据和独立签名身份。
init-local.sh: 初始化本机固定账号与随机私有凭据；保留已有配置。
local_gateway.py: 固定控制与 Web 后端，隔离凭据并回收服务进程。
local-services.sh: 启动独立数据库与对象存储。
native-service-account.ps1: 准备 Windows 原生服务验收账号。
run-telemetry.sh: 启动本机遥测服务。
test_ephemeral_source.py: 验证直接 Pod、临时卷、AppArmor 默认化与错误配置拒绝。
test_emissary_legacy_receiver.py: 验证旧接收器门禁的回环准入、目标覆盖拒绝和私有进程回收。
test_emissary_legacy_sender.py: 隔离驱动时钟，用有限睡眠验证超时、中断、编译后代与精确清库回执。
test_emissary_current_main.py: 验证完整main证据门槛、阶段、旧进程凭据隔离、源码冻结与超时回收。
test_emissary_ota_filesystem.py: 验证小卷门禁拒绝缺失ACK源删除及错身份，卸载仅用精确镜像并要求读回。
test_emissary_linux_filesystem.py: 验证错loop及挂载拒绝、卸载后读回与残留阻断。
test_emissary_native_disk.py: 拒绝缺失事务确认、原生服务连续性、恢复样本及清理的通过证据。
test_emissary_control_main.py: 验证资源归属、外键SQLSTATE、原清单摘要、两类ACK与刷新夹具。
test_emissary_ota_processes.py: 以真实独立runtime组验证未reap会话回收、失败隔离与旧SID禁查。
test_emissary_ota_mailbox.py: 验证真实邮箱观察身份匹配，缺失回复拒绝及URL脱敏回执。
test_emissary_s3_fault.py: 验证单次资产拒收、签名头与原正文透传、私有日志和精确bucket准入。
test_emissary_safe_cutover.py: 验证未停副本或残存数据库会话禁止推进，以及恢复副本摘要与私有路径。
test_data_access.py: 验证数据库访问规则。
test_deploy.py: 验证固定初始账号、单机部署清单与已有凭据保留。
test_local_gateway.py: 验证本机网关环境的凭据隔离与固定来源。
test_init_helm.py: 验证初始账号、镜像池权限与 Registry、终端私有入口的相加策略边界。
test_version.py: 验证UTF-8版本读写、两类发布标签与官方镜像。
version.py: 以UTF-8同步统一版本，核对发布标签及官方镜像。
wait_local_services.py: 等待本机依赖服务就绪。

[PROTOCOL]: Update this header on change, then check AGENTS.md
