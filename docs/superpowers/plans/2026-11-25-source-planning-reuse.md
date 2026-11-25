<!--
[INPUT]: 已确认的同 Pod 源码准备设计。
[OUTPUT]: 规定实施分工、内部接口和验收边界。
[POS]: 源码准备复用的执行计划。
[PROTOCOL]: Update this header on change, then check AGENTS.md
-->
# 源码准备与规划复用实施计划

目标：轻量 resolve 固定 commit；规划 init 一次检出并生成清单；AI 只读复用同一 emptyDir。

## 固定约束
- Kubernetes >=1.28；源码无持久缓存、PVC、S3；不新增人工介入。
- source manifest 4 MiB；全部内部 JSON 8 MiB；现有凭据、路径及执行围栏保持。
- 每次 root resolve 900 秒；provisioning 每代 900 秒；最多三个 generation；首次 acquire 才启动模型 deadline。
- 原生与镜像后续仍按清单独立检出；完整 Git 历史、祖先 tags、子模块和 LFS 语义保持。
- 排空后切换；追加迁移，不重置数据库或修改旧基线。

## 共用接口合同
- planning-protocol RunIntent 新增 source_resolution: Option<SourceResolution>，默认 None；SourceResolution {source_id: Uuid, repository_url: String, commit_sha: String}。
- pending 仅 initial；fixed 继续 source_manifest_id/source_digest + context.source_manifest；placement 两者为空。
- queue descriptor / PlanningRun CR 增加 source_required/sourceRequired；不可变 CR 无需在清单绑定后 patch。
- init 命令 prepare；/v1/prepare 返回既有 source-checkout::PrepareResponse，pending 时 manifest=None、pinned_ref_oid=固定commit、frozen_tags=已持久化tags。
- init RPC 全部使用既有 identity{run_id,generation},pod{uid,name,namespace},bootstrap_token 信封。
- /v1/prepare/pin 输入 pin{kind:tags,path,tags}，返回 {tags}；首次冻结后不可修改。
- /v1/prepare/commit 输入 result{manifest}，返回 {accepted:bool,pending:bool,source_manifest_id,source_digest}；重复轮询相同manifest收敛跨库回执，不重新下载。
- /v1/prepare/ready 输入 source_manifest_id/source_digest；要求清单已绑定和当前Execution匹配，返回 {ok:true}。
- /v1/prepare/progress 输入 progress: SourceProgress；sequence按generation单调；最多256条/run，逻辑聚合，不逐文件发送。
- /v1/prepare/fail 输入 diagnostic: SourceDiagnostic；固定分类；失败经既有 failed outbox结束resolving构建。
- 新 SourceStage: Fetch/Submodules/Lfs/Verify/Ready，保留旧枚举；SourceProgress wire现有sequence/stage/state不变。
- Coordinator -> Control 既有 /internal/planning/event 新事件 source_ready(payload{manifest,execution})，source_progress(payload{execution,progress,created_at})，source_local_ready(payload{execution,source_manifest_id,source_digest,ready_at})，planning_started(payload{execution,started_at})。
- source_ready run级持久承诺可跨generation收敛；所有新Pod请求必须匹配当前完整Execution；local-ready只能用于当前generation/UID。
- Control source_ready 回执 {accepted:true,source_manifest_id,source_digest}；取消或终态回执 {accepted:false,error}；Coordinator不得复活终态。
- Coordinator /v1/get 增加白名单 preparation 摘要，供Control准备材料前后校验与界面运行时投影。
- Control /internal/planning/source-material 支持 pending initial，输入增加完整 execution.name 或 pod_name；按当前Execution核验后使用冻结构建凭据生成 PrepareResponse。
- acquire 用绑定的manifest补全context，source_required时必须有本代ready；placement无ready要求。

## 数据与界面合同
- 两库追加2026112501；业务 builds.source_preparation_version SMALLINT NOT NULL DEFAULT 1，新建构建显式2；planning_runs.source_preparation JSONB默认{}，source_preparation_events JSONB默认[]最多256。
- Coordinator planning_runs追加source_manifest JSONB、source_frozen_tags JSONB默认{}、source_commit_event UUID、source_bound BOOL默认false、source_ready_execution JSONB、source_preparation JSONB默认{}；已有fixed intent初始化完整manifest及bound=true。
- GET build detail公开source_preparation_version；activity.source保留旧字段，增加protocol_version和preparation，preparation包括run_id/generation/pod_uid/pod_name/current_stage/ready_at/started_at/diagnostic；事件独立于500条时间线摘要。
- process v2绝不凭commit/manifest/PlanningRun存在推断ready；与runtime generation对齐，无法确认时保守显示。
- AI审计唯一标签仍在计划后，过程/计划不重复。

## 实施任务
- [x] 1. Coordinator/规划协议/Operator：pending intent、清单outbox绑定、本代ready、准备RPC、迁移与真实数据库测试。
- [x] 2. Source checkout/preparer：最终嵌套目录解析检出、tag收敛、LFS、阶段、轻量resolve、prepare RPC与测试。
- [x] 3. Control：commit-only接纳、初始run/intent、pending材料、source事件回执/失败、业务迁移与测试。
- [x] 4. Web/Gateway：真实过程显示、独立审计回归、排空开关及测试。
- [ ] 5. 集成与审阅：Rust/Worker/Web/文档/边界检查，隔离K8s真实链，修复审阅发现。
- [ ] 6. 交付：合并main提交；排空入口、备份两库、固定镜像升级、线上验证、恢复接单。

## 验收
轻量resolve无fetch；单次规划只准备一次；子模块/tag/LFS一致；ready前acquire拒绝且calls=0；重放/回执丢失/取消/旧代拒绝；Pod丢失/超时/OOM/清理；双语主题三视口+320px和axe；K8s1.28真实规划和后续执行。
