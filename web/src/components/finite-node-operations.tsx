/**
 * [INPUT]: 依赖当前管理员权限、固定操作API与有界探测结果。
 * [OUTPUT]: 提供诊断、清单刷新、状态分页与请求取消。
 * [POS]: 节点运维的有限操作入口；刷新不改变启动调度快照。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useRef, useState } from "react";
import { Loading } from "@/components/common";
import { NodeDataError, NodeDisclosure, NodeFacts, NodeSection, NodeTimestamp } from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { api, mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { createIdempotencyKey } from "@/lib/idempotency";
import { notify } from "@/lib/notify";
import type { NodeOperation, NodeOperationKind, NodeOperationPage, NodeOperationRequest } from "@/lib/node-operation-types";
import type { Node, NodeMetadataSnapshot, User } from "@/lib/types";
const terminal = new Set(["succeeded", "failed", "canceled", "expired", "interrupted", "rejected"]);
const states = new Set(["requested", "accepted", "running", ...terminal]);
const reasons = new Set(["busy", "invalid_input", "identity_mismatch", "clock_unsynchronized", "receive_expired", "storage_unavailable", "journal_full", "input_conflict", "execution_budget_exhausted", "canceled", "startup_changed", "result_expired", "inventory_unavailable", "cleanup_failed"]);
const statuses = new Set(["available", "unsupported", "permission_denied", "timeout", "error", "budget_exhausted", "canceled"]);
function Result({ operation }: { operation: NodeOperation }) {
  const { t } = useUI();
  const result = operation.result;
  if (!result) return operation.finished_at ? <p className="node-section-note">{t("nodeActions.resultExpired")} <NodeTimestamp value={operation.result_expires_at} /></p> : null;
  return <div className="space-y-3">
    {result.inventory && <NodeFacts entries={[
      [t("nodeActions.observed"), <NodeTimestamp key="observed" value={Object.values(result.probes).map(probe => probe.observed_at).sort().at(-1)} />],
      [t("nodeActions.inventoryRevision"), result.inventory.revision],
      [t("nodeActions.inventoryDelivery"), t(result.inventory.delivery === "confirmed" ? "nodeActions.confirmed" : "nodeActions.pending")],
    ]} />}
    {(result.truncated || result.omitted > 0) && <p role="status">{t("nodeActions.partial", { count: result.omitted })}</p>}
    <div className="node-tool-list">
      {Object.entries(result.probes).map(([id, probe]) => <div className="node-tool" key={id}>
        <div className="node-tool-heading"><h4>{id}</h4><span className="node-tool-version">{probe.version || t("nodes.unknown")}</span><span>{t(`nodeActions.probes.${statuses.has(probe.status) ? probe.status : "unknown"}`)}</span></div>
        <p className="node-section-note"><NodeTimestamp value={probe.observed_at} /> · {t("telemetryDelivery.milliseconds", { count: probe.duration_ms })}</p>
      </div>)}
      {Object.entries(result.skipped).map(([id, reason]) => <div className="node-tool" key={id}>{id} · {t(`nodeActions.probes.${statuses.has(reason) ? reason : "unknown"}`)}</div>)}
    </div>
  </div>;
}
export function FiniteNodeOperations({ node, user }: { node: Node; user: User }) {
  const { t } = useUI();
  const admin = user.tenant_role ? ["owner", "admin"].includes(user.tenant_role) : user.role === "admin";
  const supported = node.capabilities.includes("node-operations-v1");
  const [cursor, setCursor] = useState<string | null>(null);
  const [previous, setPrevious] = useState<(string | null)[]>([]);
  const remote = useRemote<NodeOperationPage>(admin && supported ? `/nodes/${node.id}/operations?limit=20${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}` : null, 3000);
  const startup = useRemote<NodeMetadataSnapshot>(admin && supported ? `/nodes/${node.id}/metadata` : null, 30000);
  const [pending, updatePending] = useState<NodeOperationRequest>();
  const [requestReady, setRequestReady] = useState(false);
  const requestKey = `artifactory:node-operation:${user.id}:${node.id}`;
  function setPending(request?: NodeOperationRequest) {
    updatePending(request);
    try {
      if (request) sessionStorage.setItem(requestKey, JSON.stringify(request));
      else sessionStorage.removeItem(requestKey);
    } catch { /* 保留内存中的请求身份；不保存结果正文。 */ }
  }
  useEffect(() => {
    try {
      const saved = JSON.parse(sessionStorage.getItem(requestKey) || "null") as NodeOperationRequest | null;
      if (saved && /^[a-f0-9-]{36}$/.test(saved.request_id) && saved.schema === 1 && ["collect_diagnostics", "refresh_inventory"].includes(saved.kind)) updatePending(saved);
    } catch { /* 无有效请求时保持空状态。 */ }
    setRequestReady(true);
  }, [requestKey]);
  const [canResend, setCanResend] = useState(false);
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<Error>();
  const submitting = useRef(false);
  const active = remote.data?.items.some(operation => !terminal.has(operation.state) && operation.delivery_state !== "outcome_unknown");
  const allowed = requestReady && cursor === null && node.online && !node.revoked && !node.draining && !node.ota_draining && !active && !!remote.data && !remote.error;
  async function submit(kind?: NodeOperationKind) {
    if (submitting.current) return;
    const request = pending || (kind ? { request_id: createIdempotencyKey(), kind, schema: 1 as const, input: kind === "refresh_inventory" ? { mode: "full" as const } : { probes: [] } } : undefined);
    if (!request) return;
    submitting.current = true; setBusy("create"); setError(undefined); setPending(request); setCanResend(false);
    try {
      await mutation<NodeOperation>(`/nodes/${node.id}/operations`, request);
      setPending(undefined); setCursor(null); setPrevious([]);
      notify.success(t("nodeActions.requested")); await remote.reload();
    } catch (cause) { setError(asError(cause)); }
    finally { submitting.current = false; setBusy(undefined); }
  }
  async function recover() {
    if (!pending || submitting.current) return;
    submitting.current = true; setBusy("recover"); setError(undefined);
    try {
      let cursor: string | null = null;
      for (let page = 0; page < 10; page++) {
        const result: NodeOperationPage = await api(`/nodes/${node.id}/operations?limit=100${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`);
        if (result.items.some(operation => operation.request_id === pending.request_id)) {
          setPending(undefined); setCursor(null); setPrevious([]); await remote.reload();
          notify.success(t("nodeActions.recovered")); return;
        }
        cursor = result.next_cursor;
        if (!cursor) break;
      }
      if (!cursor) setCanResend(true);
      setError(new Error(t("nodeActions.notRecovered")));
    } catch (cause) { setError(asError(cause)); }
    finally { submitting.current = false; setBusy(undefined); }
  }
  async function cancel(operation: NodeOperation) {
    if (submitting.current) return;
    submitting.current = true; setBusy(operation.id); setError(undefined);
    try { await mutation(`/node-operations/${operation.id}/cancel`, {}); notify.success(t("nodeActions.cancelRequested")); await remote.reload(); }
    catch (cause) { setError(asError(cause)); }
    finally { submitting.current = false; setBusy(undefined); }
  }
  if (!admin) return null;
  return <NodeSection title={t("nodeActions.title")}>
    <div className="space-y-4" data-testid="finite-node-operations">
      {!supported ? <p className="node-empty">{t("nodeActions.unsupported")}</p> : <>
        <p className="text-sm leading-6 text-muted-foreground">{t("nodeActions.scope")}</p>
        <p className="node-section-note">{t("nodeActions.observationOnly")}</p>
        <NodeFacts entries={[[t("nodeActions.startupSnapshot"), <NodeTimestamp key="startup" value={startup.data?.received_at} />]]} />
        <NodeDataError error={remote.error || error} retry={async () => { setError(undefined); await remote.reload(); }} />
        {!remote.data && !remote.error ? <Loading variant="form" /> : <>
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" disabled={!allowed || !!busy || !!pending} onClick={() => void submit("collect_diagnostics")}>{t("nodeActions.collect")}</Button>
            <Button variant="secondary" disabled={!allowed || !!busy || !!pending} onClick={() => void submit("refresh_inventory")}>{t("nodeActions.refresh")}</Button>
          </div>
          {pending && <div role="status" className="space-y-2"><p className="text-sm">{t("nodeActions.uncertain")}</p><Button variant="secondary" disabled={!!busy} onClick={() => void recover()}>{t("nodeActions.retrySame")}</Button>{canResend && <Button variant="secondary" disabled={!!busy} onClick={() => void submit()}>{t("nodeActions.resendSame")}</Button>}</div>}
          {!node.online && <p className="node-section-note">{t("nodeActions.offline")}</p>}
          {!remote.data?.items.length && <p className="node-empty">{t("nodeActions.empty")}</p>}
          {!remote.error && remote.data?.items.map(operation => <NodeDisclosure key={operation.id} title={`${t(operation.kind === "refresh_inventory" ? "nodeActions.refresh" : "nodeActions.collect")} · ${t(`nodeActions.states.${operation.delivery_state === "outcome_unknown" ? "outcome_unknown" : states.has(operation.state) ? operation.state : "unknown"}`)}`}>
            <div className="space-y-3">
              <NodeFacts entries={[
                [t("common.time"), <NodeTimestamp key="created" value={operation.created_at} />],
                [t("nodeActions.delivery"), t(`nodeActions.deliveryStates.${["confirmed", "awaiting_result", "outcome_unknown"].includes(operation.delivery_state) ? operation.delivery_state : "unknown"}`)],
                ...(operation.cancel_requested_at && !terminal.has(operation.state) ? [[t("nodeActions.cancel"), t("nodeActions.cancelRequested")] as [string, string]] : []),
              ]} />
              {operation.reason && <p role="status" className="node-section-note">{t(`nodeActions.reasons.${reasons.has(operation.reason) ? operation.reason : "unknown"}`)}</p>}
              <Result operation={operation} />
              {!terminal.has(operation.state) && operation.delivery_state !== "outcome_unknown" && <Button size="sm" variant="secondary" disabled={!!busy || !!operation.cancel_requested_at} onClick={() => void cancel(operation)}>{t(operation.cancel_requested_at ? "nodeActions.cancelRequested" : "nodeActions.cancel")}</Button>}
            </div>
          </NodeDisclosure>)}
          <div className="flex gap-2">
            {!remote.error && previous.length > 0 && <Button variant="secondary" onClick={() => { setCursor(previous[previous.length - 1]); setPrevious(previous.slice(0, -1)); }}>{t("nodeActions.newer")}</Button>}
            {!remote.error && remote.data?.next_cursor && <Button variant="secondary" onClick={() => { setPrevious([...previous, cursor]); setCursor(remote.data!.next_cursor); }}>{t("nodeActions.older")}</Button>}
          </div>
        </>}
      </>}
    </div>
  </NodeSection>;
}
