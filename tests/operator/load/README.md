<!--
[INPUT]: 隔离 Kubernetes 安装与 Runtime 预算。
[OUTPUT]: 定义排队、活动任务和恢复压测入口。
[POS]: 容量观察不替代生产环境验收。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# Runtime 队列与活动任务验证

先运行 `tests/operator/acceptance/prepare.py` 和 `run.py`，创建独立命名空间。准备镜像包括 Source Preparer、Agent Operator、Runtime、Control、Emissary 和 Pingora。源码任务和 AI init 每次直接拉取固定 Git/LFS，不保留源码服务或共享缓存。

`admission.py` 在一次性测试库写入 10,000 个运行，并发调用 Runtime 准入。它验证 pool=8、每项目=2，其余排队任务不消耗模型调用和执行 deadline。

```sh
export ARTIFACTORY_TEST_DATABASE_URL='<disposable PostgreSQL URL>'
python3 tests/operator/load/admission.py
```

`active.py` 启动实际 Node/Pi 循环并阻塞模型请求。它检查活动配额、排队预算、Runtime/Operator 重启后的累计预算、固定源码清单、并发取消和 Pod 清理。恢复验证等待旧 Pod UID 全部消失，并确认新 Runtime 预留模型调用。

```sh
python3 tests/operator/load/active.py \
  --context <saved-context> --namespace <saved-namespace> --active 8 --queued 4
```

脚本记录业务 API、Pod CPU/RSS/网络、PG 连接、AgentRun 数量和 Kubernetes API 变更。启动前检查节点 Pod 上限和请求内存。100/500/1,000 个活动任务需要对应的实际资源，不能用数据库排队数代替。

证据保存在 `.local/operator-load/`。单机 debug 构建和小负载的 P95 仅描述该次环境，不构成生产容量承诺。
