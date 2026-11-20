# Artifactory - 跨平台构建与制品交付
Rust 2024 + Toasty 0.11.0 + PostgreSQL 17 + Next.js 16.4.5 + React 19.3.1

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
VERSION - 全项目版本 0.2.0。
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
项目 Release 固定 SemVer、源码和目标修订。
Release 只交付一次完整构建，正式版本不可覆盖。
镜像执行只写隔离候选，正式推广受发布租约与摘要准入约束。
界面操作反馈统一使用顶部居中 notify，不占页面布局。
首次读取使用内容骨架，前台请求与导航共用顶部细进度条。
后台轮询与日志订阅不显示加载进度。
所有下拉使用自定义触发器与弹层，禁止原生选择菜单。
变更后检查文件契约、模块地图与项目地图。
[PROTOCOL]: Update this header on change, then check AGENTS.md
