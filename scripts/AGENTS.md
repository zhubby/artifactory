# scripts/
> L2 | Parent: ../AGENTS.md

acceptance.py: 创建独立服务验收，会话 Origin 跟随当前网关入口。
agent-worker.sh: 启动本地规划 Worker。
ai_acceptance.py: 验证真实模型规划和构建交接。
ai_remote_acceptance.py: 验证远端模型与节点构建。
check_data_access.py: 检查数据库访问边界及依赖。
check_docs.py: 构建 mdBook 并验证文档链接。
compose_acceptance.py: 创建隔离 Compose TLS 与 Pingora 验收环境。
compose_probe.py: 从隔离容器验证生产服务和网关制品链。
cross-build-emissary.sh: 检查设备运行程序的跨平台构建。
generate-proto.sh: 生成设备协议代码。
init-deploy.sh: 初始化单机部署的私有配置。
init-helm.py: 初始化新 Helm 部署的持久凭据与独立签名身份。
init-local.sh: 初始化本机开发配置。
legacy_fixture.py: 为兼容验收创建历史执行配方。
local_gateway.py: 固定控制与 Web 后端，隔离凭据并回收服务进程。
local-services.sh: 启动独立数据库与对象存储。
native-service-account.ps1: 准备 Windows 原生服务验收账号。
recovery_acceptance.py: 在新数据服务与 Pingora 入口验证备份恢复。
run-telemetry.sh: 启动本机遥测服务。
ssh_acceptance.py: 复用隔离服务，验证 SSH Git 身份、指纹与构建。
tenant_migrate.py: 执行既有数据的租户迁移。
test_acceptance.py: 验证服务回收、凭据隔离和新进程及恢复入口同源。
test_data_access.py: 验证数据库访问规则。
test_deploy.py: 验证单机部署清单。
test_init_helm.py: 验证网关入口、独立服务和持久 Secret。
test_recovery_acceptance.py: 验证恢复脚本隔离与归属规则。
test_tenant_migrate.py: 验证租户迁移的可重复执行。
test_version.py: 验证工作区与交付物统一版本。
version.py: 校验或同步统一项目版本。
wait_local_services.py: 等待本机依赖服务就绪。

[PROTOCOL]: Update this header on change, then check AGENTS.md
