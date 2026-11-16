# Artifactory - 跨平台构建与制品交付
Rust 2024 + Toasty 0.11.0 + PostgreSQL 17 + Next.js 16.4.5 + React 19.3.1

<directory>
crates/ - 控制平面、源码快照、执行节点与遥测服务。
operator/ - Kubernetes 规划任务和设备工作负载编排。
packages/ - 独立 Node.js AI 规划 Worker。
web/ - 多租户构建控制台。
proto/ - 设备与运行时通信协议。
schemas/ - 持久化数据与执行计划契约。
charts/ - Artifactory Helm 部署清单。
deploy/ - 容器服务和运维配置。
scripts/ - 构建、验收与版本检查入口。
tests/ - 跨服务集成和负载验收。
docs/ - mdBook 文档与真实环境验收记录。
examples/ - 构建配方示例。
vendor/ - 带连接回收修复的 Toasty 源码。
</directory>
<config>
VERSION - 全项目版本 0.2.0。
Cargo.toml - Rust 工作区和统一依赖。
Dockerfile - 控制平面与遥测运行镜像。
compose.yaml - 本机完整服务栈。
book.toml - mdBook 文档构建。
</config>

源码凭据通过凭据引用传入。禁止把密钥放入仓库 URL。
HTTP 仓库只在用户指定该协议时接入。
变更后检查文件契约、模块地图与项目地图。
[PROTOCOL]: Update this header on change, then check AGENTS.md
