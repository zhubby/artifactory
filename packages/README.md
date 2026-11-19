<!--
[INPUT]: 独立 Node.js 规划包与 Kubernetes 任务入口。
[OUTPUT]: 定义规划 Worker 的职责、构建和验证命令。
[POS]: Node.js 包目录入口；各包独立锁定依赖。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Node.js 包

各包独立维护依赖、锁文件和构建脚本。

`planning-worker` 使用 Node.js 24 和 Pi，命令为 `artifactory-planning-worker`。它只在 Planning Operator 创建的 Kubernetes Pod 中运行，通过投射令牌向 Control 内嵌的规划协调器领取任务。源码 init container 提供只读工作区；Worker 不持有上游源码凭据，也不提供常驻 Poll 入口。

在仓库根目录验证：

```sh
npm --prefix packages/planning-worker ci
npm --prefix packages/planning-worker run check
npm --prefix packages/planning-worker test
```

以仓库根目录作为镜像构建上下文：

```sh
docker build -f packages/planning-worker/Dockerfile -t artifactory-planning-worker .
```

配置、预算和身份契约见[规划与源码准备协议](../docs/planning-protocol.md)。
