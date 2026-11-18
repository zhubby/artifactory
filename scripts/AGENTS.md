# scripts/
> L2 | Parent: ../AGENTS.md

check_data_access.py: 检查数据库访问边界及依赖。
check_ephemeral_source.py: 只读检查任务临时卷、材料隔离与空闲清理。
check_docs.py: 校验 mdBook 导航覆盖与生成结果中的本地链接。
compose_acceptance.py: 创建隔离 Compose，验证 HTTPS、鉴权与重启。
compose_probe.py: 从隔离容器验证 HTTPS、会话、租户鉴权和重启。
cross-build-emissary.sh: 检查设备运行程序的跨平台构建。
generate-proto.sh: 生成设备协议代码。
init-deploy.sh: 初始化单机部署的私有配置。
init-helm.py: 初始化新 Helm 部署的持久凭据与独立签名身份。
init-local.sh: 初始化本机开发配置。
local_gateway.py: 固定控制与 Web 后端，隔离凭据并回收服务进程。
local-services.sh: 启动独立数据库与对象存储。
native-service-account.ps1: 准备 Windows 原生服务验收账号。
run-telemetry.sh: 启动本机遥测服务。
test_ephemeral_source.py: 验证持久源码、凭据泄漏与残留任务检测。
test_data_access.py: 验证数据库访问规则。
test_deploy.py: 验证单机部署清单。
test_local_gateway.py: 验证本机网关环境的凭据隔离与固定来源。
test_init_helm.py: 验证统一网关端口隔离、私有遥测后端和持久 Secret。
test_version.py: 验证工作区与交付物统一版本。
version.py: 校验或同步统一项目版本。
wait_local_services.py: 等待本机依赖服务就绪。

[PROTOCOL]: Update this header on change, then check AGENTS.md
