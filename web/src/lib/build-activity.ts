/**
 * [INPUT]: 依赖构建协议版本、本代源码记录和安全规划历史。
 * [OUTPUT]: 提供过程响应类型与基于实际证据的阶段状态。
 * [POS]: 观察视图的数据边界；未发生阶段保持未开始。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Build, PlanningHistory, SourcePreparation } from "@/lib/types";

export type ProcessStage = "source" | "planning" | "dispatch" | "execution" | "artifacts";
export type ProcessState = "unknown" | "not_started" | "queued" | "running" | "succeeded" | "failed" | "canceled" | "needs_input";
export interface ActivityEvent {
  id: string;
  stage: ProcessStage | "build";
  action: string;
  status: string;
  created_at: string;
  details: {
    actor?: string | null; target?: string; attempt?: number; sequence?: number;
    model?: string | null; purpose?: string; calls?: number; revision?: number;
    step?: string; phase?: string; exit_code?: number | null; code?: string | null;
    commit_sha?: string | null; path?: string; sha256?: string; digest?: string;
    attempt_id?: string; execution_id?: string; run_id?: string; plan_id?: string | null;
    generation?: number;
  };
}
export interface BuildActivity {
  build_id: string;
  generated_at: string;
  truncated: boolean;
  source: {
    status: string; created_at: string; finished_at: string | null;
    attempt: number | null; pod: string | null; history_available: boolean;
    diagnostic: { stage: string; code: string } | null;
    protocol_version?: number;
    preparation?: SourcePreparation | null;
  } | null;
  planning: PlanningHistory;
  events: ActivityEvent[];
}

export function sourcePreparation(build: Build, activity?: BuildActivity): SourcePreparation | undefined {
  if ((build.source_preparation_version ?? activity?.source?.protocol_version ?? 1) < 2) return;
  const run = activity?.planning.runs.findLast(run => run.purpose !== "placement");
  const saved = activity?.source?.preparation;
  if (run && ["queued", "leased"].includes(run.state)) {
    if (!run.runtime) return saved?.run_id === run.id ? { run_id: run.id } : undefined;
    const runtime = run.runtime.preparation;
    if (runtime?.run_id !== run.id || runtime.generation !== run.runtime.generation) return { run_id: run.id, generation: run.runtime.generation };
    const events = saved?.run_id === run.id && saved.generation === runtime.generation && saved.pod_uid === runtime.pod_uid ? saved.events : undefined;
    return { ...runtime, events };
  }
  return saved ?? undefined;
}

export function sourceSteps(build: Build, activity?: BuildActivity): { key: string; state: ProcessState }[] {
  const preparation = sourcePreparation(build, activity);
  const stopped: ProcessState = build.status === "canceled" ? "canceled" : "failed";
  return ["fetch", "submodules", "lfs", "verify", "manifest", "ready"].map(key => {
    const event = preparation?.events?.filter(event => event.progress.stage === key && event.execution.generation === preparation.generation && event.execution.uid === preparation.pod_uid).at(-1);
    const state: ProcessState = preparation?.diagnostic?.stage === key ? stopped
      : key === "ready" && preparation?.ready_at ? "succeeded"
      : event?.progress.state === "completed" ? "succeeded"
      : event || preparation?.current_stage === key ? preparation?.current_stage === key && preparation.current_state === "completed" ? "succeeded" : ["failed", "canceled", "lost"].includes(build.status) ? stopped : "running"
      : "not_started";
    return { key, state };
  });
}

export function buildStages(build: Build, activity?: BuildActivity): { key: ProcessStage; state: ProcessState }[] {
  const terminal = ["failed", "canceled", "lost"].includes(build.status);
  const stopped: ProcessState = build.status === "canceled" ? "canceled" : "failed";
  const planned = !!build.current_plan_id || !!build.jobs?.length;
  const version = build.source_preparation_version ?? activity?.source?.protocol_version ?? 1;
  const preparation = sourcePreparation(build, activity);
  const sourceReady = version >= 2 ? !!preparation?.ready_at
    : !!build.commit_sha || !!build.source_manifest_id || activity?.source?.status === "succeeded" || planned || !!activity?.planning.runs.length;
  const attempts = build.jobs?.flatMap(job => job.attempts) || [];
  const images = build.image_executions?.filter(execution => execution.operation === "build") || [];
  const started = attempts.length > 0 || images.length > 0;
  const sourceFailed = !!preparation?.diagnostic || activity?.source?.status === "failed" || (version < 2 && terminal && !sourceReady && !planned && !activity?.planning.runs.length);
  const source: ProcessState = sourceReady ? "succeeded" : sourceFailed ? stopped
    : ["resolving", "planning", "replanning"].includes(build.status) ? "running" : build.status === "canceled" ? "canceled" : version >= 2 && terminal ? "unknown" : "queued";
  const planningStarted = version < 2 || !!preparation?.started_at;
  const planning: ProcessState = version >= 2 && !planningStarted ? sourceReady ? terminal ? stopped : "queued" : "not_started" : !sourceReady && !planned ? "not_started"
    : build.status === "needs_input" ? "needs_input"
    : ["planning", "replanning"].includes(build.status) ? "running"
    : planned ? "succeeded" : terminal ? stopped : "queued";
  const dispatch: ProcessState = !planned ? "not_started" : started ? "succeeded" : terminal ? stopped : "running";
  const execution: ProcessState = !started ? "not_started" : build.status === "replanning" ? "queued" : build.status === "succeeded" ? "succeeded"
    : terminal ? stopped : "running";
  const outputs = (build.artifacts?.length || 0) + images.filter(image => image.status === "succeeded" && image.root_digest).length;
  const artifacts: ProcessState = build.publish_status === "failed" ? "failed"
    : build.publish_status === "pending" ? "running"
    : build.status === "succeeded" ? "succeeded"
    : outputs > 0 ? "running" : "not_started";
  return [
    { key: "source", state: source }, { key: "planning", state: planning },
    { key: "dispatch", state: dispatch }, { key: "execution", state: execution },
    { key: "artifacts", state: artifacts },
  ];
}
