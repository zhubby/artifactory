/**
 * [INPUT]: 依赖节点心跳缓存与有界遥测链路诊断。
 * [OUTPUT]: 展示队列进展、清单冲突、永久拒收与磁盘观测。
 * [POS]: 节点运维区的只读链路解释；不发起采集请求。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { NodeDisclosure, NodeFacts, NodeSection, NodeTimestamp } from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import type { Node } from "@/lib/types";
import { bytes } from "@/lib/utils";
const states = new Set(["ready", "connecting", "schema_wait", "schema_unsupported", "encoding_unsupported", "overloaded", "database_unavailable", "authentication_failed", "authorization_denied", "startup_unregistered", "clock_future", "expired_sample", "malformed", "bounds_exceeded", "id_content_conflict", "identity_retired", "unclassified_rejection", "connection_unavailable", "storage_unavailable", "storage_or_configuration_unavailable", "waiting_inventory_receiver"]);
const diskStates = new Set(["ready", "scan_failed", "unsafe_entry", "scan_budget_exceeded"]);
export function NodeTelemetryStatus({ node }: { node: Node }) {
  const { t } = useUI();
  const value = node.diagnostics.metrics?.telemetry;
  const number = (value: unknown) => typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
  const size = (value: unknown) => number(value) === undefined ? undefined : bytes(value as number);
  const seconds = (value: unknown) => number(value) === undefined ? undefined : t("telemetryDelivery.seconds", { count: value as number });
  const duration = (value: unknown) => number(value) === undefined ? undefined : t("telemetryDelivery.milliseconds", { count: value as number });
  const state = (value?: string) => t(`telemetryDelivery.states.${value && states.has(value) ? value : "unknown"}`);
  const status = !value ? state() : !node.online && ((number(value.spool_records) || 0) + (number(value.fresh_records) || 0) + (number(value.inventory_records) || 0) > 0)
    ? t("telemetryDelivery.offlineBacklog") : state(value.state);
  return <NodeSection title={t("telemetryDelivery.title")}>
    <div data-testid="node-telemetry-status">
      <p role="status" className="mb-3 text-sm">{status}</p>
      {value?.negotiation_state === "retrying_safe_format" && <p className="node-section-note mb-3">{t("telemetryDelivery.safeFormat")}</p>}
      {value?.last_rejection_reason && <NodeFacts entries={[
        [t("telemetryDelivery.lastRejection"), state(value.last_rejection_reason)],
        [t("telemetryDelivery.lastRejectionAt"), <NodeTimestamp key="rejection" value={value.last_rejection_at} />],
      ]} />}
      <NodeFacts entries={[
        [t("telemetryDelivery.samples"), number(value?.spool_records)],
        [t("telemetryDelivery.sampleBytes"), size(value?.spool_bytes)],
        [t("telemetryDelivery.oldest"), seconds(value?.oldest_age_seconds)],
        [t("telemetryDelivery.schemaWait"), number(value?.schema_wait_records)],
        [t("telemetryDelivery.freshRecords"), number(value?.fresh_records)],
        [t("telemetryDelivery.freshBytes"), size(value?.fresh_bytes)],
        [t("telemetryDelivery.freshOldest"), seconds(value?.fresh_oldest_age_seconds)],
        [t("telemetryDelivery.inventory"), number(value?.inventory_records)],
        [t("telemetryDelivery.inventoryBytes"), size(value?.inventory_bytes)],
        [t("telemetryDelivery.inventoryOldest"), seconds(value?.inventory_oldest_age_seconds)],
        [t("telemetryDelivery.inventorySchemaWait"), number(value?.inventory_schema_wait_records)],
        [t("telemetryDelivery.inventoryState"), state(value?.inventory_state)],
        [t("telemetryDelivery.lastAck"), <NodeTimestamp key="ack" value={value?.last_successful_ack_at} />],
        [t("telemetryDelivery.lastInventoryAck"), <NodeTimestamp key="inventory" value={value?.last_inventory_ack_at} />],
        [t("telemetryDelivery.uploadLatency"), number(value?.upload_latency_ms) === undefined ? undefined : t("telemetryDelivery.milliseconds", { count: value!.upload_latency_ms! })],
        [t("telemetryDelivery.backoff"), seconds(value?.backoff_seconds)],
        [t("telemetryDelivery.collectorStatus"), value?.collector_error ? t("telemetryDelivery.collectorUnavailable") : number(value?.collector_duration_ms) === undefined ? undefined : t("telemetryDelivery.collectorAvailable")],
        [t("telemetryDelivery.collector"), number(value?.collector_duration_ms) === undefined ? undefined : t("telemetryDelivery.milliseconds", { count: value!.collector_duration_ms! })],
        [t("telemetryDelivery.corrupt"), number(value?.corrupt_records)],
        [t("telemetryDelivery.dropped"), number(value?.dropped_batches)],
        [t("telemetryDelivery.quotaEvictions"), number(value?.quota_eviction_batches)],
        [t("telemetryDelivery.expired"), number(value?.expired_batches)],
        [t("telemetryDelivery.malformed"), number(value?.malformed_batches)],
        [t("telemetryDelivery.diskErrors"), number(value?.disk_errors)],
        [t("telemetryDelivery.coalesced"), number(value?.coalesced_batches)],
        [t("telemetryDelivery.superseded"), number(value?.inventory_superseded)],
        [t("telemetryDelivery.inventoryConflicts"), number(value?.inventory_conflict_records)],
        [t("telemetryDelivery.inventoryConflictsTotal"), number(value?.inventory_conflicts_total)],
        [t("telemetryDelivery.inventoryLastConflict"), <NodeTimestamp key="inventory-conflict" value={value?.inventory_last_conflict_at} />],
      ]} />
      <div data-testid="telemetry-delivery-lanes">
        <NodeDisclosure title={t("telemetryDelivery.lanes")}>
          {([
            ["fresh", value?.fresh_state, value?.fresh_last_ack_at, value?.fresh_last_progress_at, value?.fresh_upload_latency_ms, value?.fresh_backoff_seconds],
            ["history", value?.history_state, value?.history_last_ack_at, value?.history_last_progress_at, value?.history_upload_latency_ms, value?.history_backoff_seconds],
            ["inventory", value?.inventory_state, value?.inventory_last_ack_at ?? value?.last_inventory_ack_at, value?.inventory_last_progress_at, value?.inventory_upload_latency_ms, value?.inventory_backoff_seconds],
          ] as const).map(([lane, current, ack, progress, latency, backoff]) => <div key={lane} data-testid={`telemetry-lane-${lane}`} className="mb-5 last:mb-0">
            <h4 className="mb-2 text-sm font-medium">{t(`telemetryDelivery.laneNames.${lane}`)}</h4>
            <NodeFacts entries={[
              [t("telemetryDelivery.laneState"), state(current)],
              [t("telemetryDelivery.laneAck"), <NodeTimestamp key="ack" value={ack} />],
              [t("telemetryDelivery.laneProgress"), <NodeTimestamp key="progress" value={progress} />],
              [t("telemetryDelivery.uploadLatency"), duration(latency)],
              [t("telemetryDelivery.backoff"), seconds(backoff)],
            ]} />
          </div>)}
        </NodeDisclosure>
      </div>
      <div data-testid="telemetry-disk-observation">
        <NodeDisclosure title={t("telemetryDelivery.diskTitle")}>
          <NodeFacts entries={[
            [t("telemetryDelivery.diskState"), t(`telemetryDelivery.diskStates.${value?.disk_state && diskStates.has(value.disk_state) ? value.disk_state : "unknown"}`)],
            [t("telemetryDelivery.diskObserved"), <NodeTimestamp key="disk" value={value?.disk_observed_at} />],
            [t("telemetryDelivery.diskContents"), size(value?.disk_file_bytes)],
            [t("telemetryDelivery.diskRecords"), size(value?.disk_record_bytes)],
            [t("telemetryDelivery.diskAllocated"), size(value?.disk_allocated_bytes)],
            [t("telemetryDelivery.diskOverhead"), size(value?.disk_block_overhead_bytes)],
            [t("telemetryDelivery.diskTemporary"), size(value?.disk_temporary_bytes)],
            [t("telemetryDelivery.diskIndexes"), size(value?.disk_index_bytes)],
            [t("telemetryDelivery.diskCorrupt"), size(value?.disk_corrupt_bytes)],
            [t("telemetryDelivery.diskUnknown"), size(value?.disk_unknown_bytes)],
            [t("telemetryDelivery.diskFiles"), number(value?.disk_files)],
          ]} />
          <p className="node-section-note">{t("telemetryDelivery.diskHint")}</p>
        </NodeDisclosure>
      </div>
      <div data-testid="telemetry-permanent-losses">
        <NodeDisclosure title={t("telemetryDelivery.lossDetails")}>
          <NodeFacts entries={[
            [t("telemetryDelivery.boundsExceeded"), number(value?.bounds_exceeded_batches)],
            [t("telemetryDelivery.idContentConflicts"), number(value?.id_content_conflict_batches)],
            [t("telemetryDelivery.identitiesRetired"), number(value?.identity_retired_batches)],
            [t("telemetryDelivery.omittedHostDevices"), number(value?.omitted_host_devices)],
            [t("telemetryDelivery.truncatedHostFields"), number(value?.truncated_host_fields)],
          ]} />
        </NodeDisclosure>
      </div>
      <p className="node-section-note">{t("telemetryDelivery.hint")}</p>
    </div>
  </NodeSection>;
}
