/**
 * [INPUT]: 依赖 Control 的有限操作公开契约。
 * [OUTPUT]: 提供操作输入、探测结果、清单回执和分页类型。
 * [POS]: 节点运维组件的类型边界；不包含认证凭据。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
export type NodeOperationKind = "collect_diagnostics" | "refresh_inventory";
export type NodeOperationState = "requested" | "accepted" | "running" | "succeeded" | "failed" | "canceled" | "expired" | "interrupted" | "rejected";
export interface NodeProbeReport {
  status: "available" | "unsupported" | "permission_denied" | "timeout" | "error";
  observed_at: string;
  duration_ms: number;
  probe_version: number;
  version: string | null;
  error_code: string | null;
}
export interface NodeOperation {
  id: string;
  request_id: string;
  kind: NodeOperationKind;
  state: NodeOperationState;
  reason: string | null;
  delivery_state: string;
  cancel_requested_at: string | null;
  created_at: string;
  updated_at: string;
  finished_at: string | null;
  result_expires_at: string | null;
  result: {
    probes: Record<string, NodeProbeReport>;
    skipped: Record<string, "budget_exhausted" | "canceled">;
    inventory: { observation_id: string; revision: number; delivery: "pending" | "confirmed" } | null;
    truncated: boolean;
    omitted: number;
  } | null;
}
export interface NodeOperationPage { items: NodeOperation[]; next_cursor: string | null }
export interface NodeOperationRequest {
  request_id: string;
  kind: NodeOperationKind;
  schema: 1;
  input: { probes: string[] } | { mode: "full" };
}
