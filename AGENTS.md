# Artifactory - 跨平台构建与制品交付
Rust 2024 + Toasty 0.11.0 + PostgreSQL 17 + Next.js 16.4.5 + React 19.3.1 + Recharts 3.11.1

<directory>
.github/ - 平台验证与不可变镜像交付。
crates/ - 控制平面、规划协调、共享协议、临时源码检出、执行节点与网关。
operators/ - 独立规划与镜像构建 Operator。
packages/ - Kubernetes Pod 使用的 Node.js AI 规划 Worker。
web/ - 多租户构建控制台。
proto/ - 设备通信协议。
schemas/ - 持久化数据与执行计划契约。
charts/ - Artifactory Helm 部署清单。
deploy/ - 容器服务和运维配置。
scripts/ - 构建、验收与版本检查入口。
tests/ - 跨服务集成和负载验收。
todos/ - 实施任务状态与待执行的环境验收。
docs/ - mdBook 文档与真实环境验收记录。
examples/ - 构建配方示例。
vendor/ - 带连接回收修复的 Toasty 源码。
</directory>
<config>
VERSION - 全项目版本 0.2.3。
Cargo.toml - Rust 工作区和统一依赖。
Dockerfile - 限定控制平面与遥测依赖的运行镜像。
compose.yaml - 本机完整服务栈。
book.toml - mdBook 文档构建。
</config>

源码凭据通过凭据引用传入。禁止把密钥放入仓库 URL。
平台最低支持 Kubernetes 1.28，高版本使用同一执行实现。
内建 OCI 仓库由 Distribution 提供，使用独立私有 S3 Bucket。
统一 Pingora 双副本网关反向代理内建仓库与制品。
公开、内部和设备入口使用独立端口。设备连接透传 mTLS。
公网协议共用网关 Service，内部 HTTPS 保留独立 ClusterIP。
遥测查询与写入共用私有 Service。
HTTP 仓库只在用户指定该协议时接入。
项目 Release 仅定义不可变 SemVer 与可编辑版本说明。
每次构建独立选择源码引用与目标，并固定自己的源码清单。
原生目标必须有项目授权的对应系统与架构设备，离线设备保留排队。
首次安装固定签名版本与平台，设备公开地址仅系统管理员在设置中修改。
全部一方组件共用 Artifactory 统一版本，Emissary 按此版本判断升级。
Emissary 升级由租户管理员在控制台发起，版本差异不自动触发执行。
OCI 镜像架构独立选择，由 BuildKit 构建池执行。
模型名称由供应商定义，请求兼容能力显式配置。
Release 只交付一次完整构建，正式版本不可覆盖。
镜像执行只写隔离候选，正式推广受发布租约与摘要准入约束。
界面操作反馈统一使用顶部居中 notify，不占页面布局。
通知中心按账户跨租户聚合，读取与外发复核当前权限。
通知事件与业务同事务提交，邮件和 Webhook 默认关闭。
首次读取使用内容骨架，前台请求与导航共用顶部细进度条。
后台轮询与日志订阅不显示加载进度。
所有下拉使用自定义触发器与弹层，禁止原生选择菜单。
概览由服务端按授权范围聚合完整窗口，不从截断列表推算统计。
构建过程使用持久阶段与真实执行历史，缺失记录不得推造。
设备终端只授予租户 owner/admin 的原登录 Session，设备默认关闭。
终端字节走独立 mTLS 与私有认证流，Control 不执行 Shell。
终端固定 owner，授权五秒截止，设备四十五秒租约回收进程树。
变更后检查文件契约、模块地图与项目地图。
[PROTOCOL]: Update this header on change, then check AGENTS.md
