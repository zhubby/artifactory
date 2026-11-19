<!--
[INPUT]: 固定工作区镜像与全新 Kubernetes 实例。
[OUTPUT]: 记录模块重构的部署身份、验证与环境边界。
[POS]: 本轮独立实例部署记录；保留旧实例。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 模块重构 Kubernetes 新实例部署

日期：2026-11-19，Asia/Shanghai。
目标集群为 Kubernetes `1.28.3`，节点均为 amd64。
本轮采用新命名空间 `artifactory-20261119`。
旧 `artifactory` 实例继续运行。
新旧实例不共享数据库、PVC、主密钥或 CA。

## 源码与镜像身份

本轮源码包含用户尚未提交的工作区改动。
基准提交为 `595f3166d4f72b3117b9570331fb5a7cd5832019`。
冻结快照包括 1041 个文件。
内容摘要为 `a137b8cf952f516c6324ce41572076fb6889c28d9cc2b7a555d88ecbd84a636e`。
镜像标签为 `workspace-a137b8cf952f516c`。
镜像使用固定 digest 部署，未移动发布标签。
该快照不能描述为上述基准提交的纯净产物。

部署镜像位于内网 Harbor 的 `pre-release` 项目。
十项镜像均完成发布和匿名摘要读回。
仓库摘要与本地 OCI 归档摘要一致。
镜像保留 `planning-operator` 与 `planning-worker` 新名称。
远端构建器下载基础镜像超时。
最终镜像在 arm64 宿主通过 amd64 仿真构建。
目标集群执行原生 amd64 运行验证。
两种环境不混写为原生 amd64 构建。
Emissary 镜像已交付，本轮未向现有设备安装。

## 持久身份与入口

业务数据库基线为 `2026111902`。
规划数据库基线为 `2026111901`。
遥测库沿用现有三项迁移基线。
TimescaleDB 最大连接数设置为 200。
两个 PVC 分别为数据库和 SeaweedFS 服务提供独立存储。
内建 Registry 使用独立私有 Bucket 和存储身份。

控制台和 Registry 使用公开网关 NodePort `30820`。
访问地址为 `http://10.169.1.239:30820`。
设备 mTLS 入口使用 `30853`。
设备遥测入口使用 `30854`。
数据库、S3 和内部规划入口保持 ClusterIP。
本轮沿用已有内网 HTTP 试用模式。

规划协调器仍内嵌于 Control。
它通过 `/internal/planning` 提供内部接口。
规划 CRD 使用 `planning.artifactory.dev`。
旧 Agent CRD 保留，现有 Image CRD 保持原值。
两个 Operator 按新命名空间限制工作范围。
三个服务账号均不能读取旧实例 Secret。
集群级权限仅包含节点读取和 TokenReview。

Registry 关闭未配置的 OpenTelemetry 追踪导出。
该配置来自私有 `registry-chart/` 覆盖层。
源码冻结快照保持原值。

## 已完成的现场验证

三个 Control 副本与双副本 Gateway 就绪。
规划和镜像 Operator 各两个副本就绪。
四个 Helm release 均为 deployed。
九个 Deployment、十五个常驻 Pod 全部就绪。
容器重启次数为零。
PlanningPool/default 返回 Ready。
BuilderPool/default 使用本轮固定镜像摘要。
Image 控制器不写入 BuilderPool 就绪状态。
两个 Operator 的独立 Lease 均已创建。
公开登录页仅显示语言和主题选项。
登录、退出和会话撤销通过。
租户与项目访问、匿名访问拒绝通过。
OCI blob、manifest 的推送、读取和 HEAD 通过。
临时 Registry Token 已撤销。
匿名读取 OCI manifest 和 S3 Bucket 均返回拒绝。
内部规划接口通过 CA 和主机名校验。
服务令牌认证返回 200。
相同内部路径通过公开入口返回 403。
业务、规划和遥测数据库基线与当前源码一致。
临时源码存储与空闲任务清理检查通过。
旧实例 49 项对象的 UID 和内容指纹均未变化。

## 环境边界与回退

集群到 GitHub 的 HTTPS 探测仍超时。
本轮不把隔离环境的测试记为线上公网构建通过。
本轮没有重注册现有设备，没有执行原生设备构建。
旧实例继续使用原入口和持久身份。
本记录在源码冻结后编写。
已部署文档来自冻结快照，不包含本记录。

需要停止新实例时，先关闭池准入并取消任务。
确认执行 Pod 停止后再缩容新业务工作负载。
保留数据库、对象存储、PVC 和全部 Secret。
不要删除命名空间或共享 CRD。
不要将旧二进制连接到新基线数据库。

私有部署目录保存固定镜像回执、四组 values 和回退命令。
初始登录凭据保存在该目录的 `bootstrap/access.json`。
凭据、私钥和会话未写入 Git。

## 初始账号修正

同日按用户要求将新实例账号设为 `admin / 123456`。
数据库密码散列、初始 Secret 和私有凭据文件已同步。
重置时撤销管理员旧会话，保留用户和租户数据。
API 登录和浏览器进入租户大厅均验证通过。
旧实例保持原值。

本机、单机和 Helm 初始化源码同步固定初始账号。
Control 为首次初始化单独接受该密码。
注册和修改密码仍要求 12–256 字节。
启动时只在没有用户的业务库创建初始管理员。
已有用户密码不受重启影响。
本次在线修复更新数据库与 Secret，未替换已部署镜像。
后续镜像构建采用上述初始化源码修改。

[PROTOCOL]: Update this header on change, then check AGENTS.md
