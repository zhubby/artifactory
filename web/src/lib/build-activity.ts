/**
 * [INPUT]: 依赖构建快照、源码记录和安全规划历史。
 * [OUTPUT]: 提供过程响应类型与基于实际证据的阶段状态。
 * [POS]: 观察视图的数据边界；未发生阶段保持未开始。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Build, PlanningHistory } from "@/lib/types";

export type ProcessStage = "source" | "planning" | "dispatch" | "execution" | "artifacts";
export type ProcessState = "not_started" | "queued" | "running" | "succeeded" | "failed" | "canceled" | "needs_input";
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
  } | null;
  planning: PlanningHistory;
  events: ActivityEvent[];
}

export function buildStages(build: Build, activity?: BuildActivity): { key: ProcessStage; state: ProcessState }[] {
  const terminal = ["failed", "canceled", "lost"].includes(build.status);
  const stopped: ProcessState = build.status === "canceled" ? "canceled" : "failed";
  const planned = !!build.current_plan_id || !!build.jobs?.length;
  const sourceReady = !!build.commit_sha || !!build.source_manifest_id || activity?.source?.status === "succeeded" || planned || !!activity?.planning.runs.length;
  const attempts = build.jobs?.flatMap(job => job.attempts) || [];
  const images = build.image_executions?.filter(execution => execution.operation === "build") || [];
  const started = attempts.length > 0 || images.length > 0;
  const sourceFailed = activity?.source?.status === "failed" || (terminal && !sourceReady && !planned && !activity?.planning.runs.length);
  const source: ProcessState = sourceReady ? "succeeded" : sourceFailed ? stopped
    : build.status === "resolving" ? "running" : build.status === "canceled" ? "canceled" : "queued";
  const planning: ProcessState = !sourceReady && !planned ? "not_started"
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
