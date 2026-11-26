/**
 * [INPUT]: 依赖当前OTA状态与实际启动器能力投影。
 * [OUTPUT]: 展示真实资产字节、阶段、重试和排空计数。
 * [POS]: 节点OTA旁路观测；不从阶段推算百分比。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { NodeFacts, NodeTimestamp } from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import type { Node } from "@/lib/types";
import { bytes } from "@/lib/utils";
const phases = new Set(["space_check", "download", "refresh_url", "verify", "verified", "drain", "health", "succeeded", "failed", "rolled_back", "canceled"]);
const errors = new Set(["insufficient_space", "space_check_unavailable", "download_failed", "download_timeout", "download_canceled", "verify_failed", "failed", "rolled_back", "canceled"]);
const terminal = new Set(["succeeded", "failed", "rolled_back", "canceled"]);
function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}
function count(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}
export function EmissaryUpdateProgress({ node }: { node: Node }) {
  const { t } = useUI();
  const status = node.emissary_ota_status;
  const observation = record(status?.observability);
  const capabilities = observation?.launcher_capabilities;
  const supported = observation?.schema_version === 1 && Array.isArray(capabilities) && capabilities.includes("ota-progress-v1");
  const candidate = record(observation?.progress);
  const progress = supported && candidate?.update_id === status?.update_id && candidate?.generation === status?.generation ? candidate : undefined;
  const done = count(progress?.bytes_done);
  const total = count(progress?.bytes_total);
  const percentage = done !== undefined && total !== undefined && total > 0 && done <= total ? Math.floor(done / total * 100) : undefined;
  const phase = typeof progress?.subphase === "string" && phases.has(progress.subphase) ? progress.subphase : "unknown";
  const last = typeof observation?.last_progress_at === "string" ? observation.last_progress_at : undefined;
  const stalled = !terminal.has(phase) && observation?.stalled === true;
  const drain = record(observation?.drain);
  return <div className="space-y-3" data-testid="emissary-update-progress">
    {!supported ? <p className="text-muted-foreground">{t("otaProgress.unsupported")}</p> : <>
      <p role="status">{t(`otaProgress.phases.${phase}`)}{stalled ? ` · ${t("otaProgress.stalled")}` : ""}</p>
      {percentage !== undefined ? <div className="space-y-1">
        <progress className="w-full accent-current" max={100} value={percentage} aria-label={t("otaProgress.download")} />
        <p className="text-muted-foreground">{bytes(done)} / {bytes(total)} · {percentage}%</p>
      </div> : <p className="text-muted-foreground">{t("otaProgress.unknownBytes")}</p>}
      {typeof progress?.error_code === "string" && <p role="status" className="text-destructive">{t(`otaProgress.errors.${errors.has(progress.error_code) ? progress.error_code : "unknown"}`)}</p>}
      <NodeFacts entries={[
        [t("otaProgress.attempt"), count(progress?.download_attempt)],
        [t("otaProgress.lastProgress"), <NodeTimestamp key="progress" value={last} />],
        ...(drain ? [
          [t("otaProgress.workers"), count(drain.active_workers)],
          [t("otaProgress.logs"), count(drain.pending_log_records)],
          [t("otaProgress.terminalRecords"), count(drain.pending_terminal_records)],
          [t("otaProgress.logBytes"), count(drain.pending_log_bytes) === undefined ? undefined : bytes(drain.pending_log_bytes as number)],
          [t("otaProgress.terminals"), count(drain.terminals)],
          [t("otaProgress.operations"), count(drain.operations)],
        ] as [string, number | string | undefined][] : []),
      ]} />
      <p className="node-section-note">{t("otaProgress.hint")}</p>
    </>}
  </div>;
}
