/**
 * [INPUT]: 依赖 Planning coordinator 租约和稳定事件身份。
 * [OUTPUT]: 提供幂等提案、审计报告与终态错误确认。
 * [POS]: Kubernetes Worker 的唯一平台传输。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { safeError, type Rpc, type Task } from "../planning.js";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
export interface Lease { run_id: string; generation: number; uid: string; token: string }
export type Post = (path: string, body: Record<string, unknown>) => Promise<Record<string, unknown>>;
export async function reportTerminalFailure(error: unknown, task: Task, lease: Lease, post: Post, signal: AbortSignal): Promise<boolean> {
  if (signal.aborted) return false;
  const message = error instanceof Error ? error.message : String(error);
  // Transport failures may have an unknown outcome. Planning coordinator retries the same
  // logical run under its persisted budget/deadline and bounded generations.
  // Only explicit budget/configuration failures may end it here.
  if (!/model call limit reached|model call budget exhausted|planning (?:deadline|context budget) reached|\b40[0134]\b|model (?:authentication rejected|not found)|invalid (?:model|api key)/i.test(message)) return false;
  try { await planningRpc(lease, post, signal)("report", { kind: "failed", payload_json: JSON.stringify({ error: safeError(error, task.model.api_key) }) }); return true; } catch { return false; }
}
export function httpPost(origin: string, signal: AbortSignal): Post {
  return async (path, body) => {
    const response = await fetch(`${origin.replace(/\/$/, "")}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), signal: AbortSignal.any([signal, AbortSignal.timeout(30000)]) });
    if (!response.ok) throw new Error(`planning request rejected (${response.status})`);
    const text = await response.text();
    if (Buffer.byteLength(text, "utf8") > 8 * 1024 * 1024) throw new Error("planning reply exceeds limit");
    return JSON.parse(text) as Record<string, unknown>;
  };
}
export function planningRpc(lease: Lease, post: Post, signal: AbortSignal): Rpc {
  let sequence = Date.now();
  return async (method, payload) => {
    if (signal.aborted) throw new Error("task canceled");
    if (method === "report") {
      const body = { lease, event_id: randomUUID(), sequence: ++sequence, kind: payload.kind, payload: JSON.parse(String(payload.payload_json || "{}")) };
      // Retry the same event identity; out-of-order reports cannot replace newer state.
      for (let attempt = 0; ; attempt++) {
        try { return await post("/v1/report", body); } catch (error) { if (attempt >= 2 || signal.aborted) throw error; await delay(250, undefined, { signal }); }
      }
    }
    const name = String(payload.name);
    const args = JSON.parse(String(payload.arguments_json || "{}")) as Record<string, unknown>;
    const request_id = randomUUID();
    const submission = name === "submit_plan" || name === "submit_placements" || name === "request_input";
    let result: Record<string, unknown> | undefined;
    try { result = await post("/v1/tool", { lease, name, arguments: args, request_id, tool_call_id:payload.tool_call_id }); }
    catch (error) {
      if (!submission) throw error;
      // The send may have committed. Query and re-send the original ID only.
      while (!result && !signal.aborted) {
        try { result = await post("/v1/proposal", { lease, proposal_id: request_id }); }
        catch {
          try { result = await post("/v1/tool", { lease, name, arguments: args, request_id, tool_call_id:payload.tool_call_id }); } catch { await delay(500, undefined, { signal }); }
        }
      }
    }
    if (!result) throw new Error("task canceled");
    while (result.pending && !signal.aborted) {
      await delay(500, undefined, { signal });
      try { result = await post("/v1/proposal", { lease, proposal_id: request_id }); } catch { /* Durable submission stays pending; never restart inference. */ }
    }
    if (signal.aborted) throw new Error("task canceled");
    return result;
  };
}
