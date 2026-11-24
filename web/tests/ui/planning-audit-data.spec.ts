/**
 * [INPUT]: 依赖真实审计投影与显式调用、提案和分片样本。
 * [OUTPUT]: 验证关联身份、缺失记录、分片完整性与提示分类。
 * [POS]: 审计数据边界的纯函数回归，不启动模型或请求服务。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { auditEntries } from "../../src/lib/planning-audit";
import type { ConversationMessage } from "../../src/lib/types";

const msg = (sequence: number, payload: Record<string, unknown>, kind = "message"): ConversationMessage => ({ sequence, kind, payload, created_at: "2026-11-24T00:00:00Z" });

test("same-name calls retain their own result even when the result order is reversed", () => {
  const raw = [msg(1, { role: "assistant", content: [{ type: "toolCall", id: "a", name: "read_file", arguments: { path: "a.md" } }, { type: "toolCall", id: "b", name: "read_file", arguments: { path: "b.md" } }] }),
    msg(2, { role: "toolResult", toolCallId: "b", toolName: "read_file", isError: true, content: "missing b.md" }),
    msg(3, { role: "toolResult", toolCallId: "a", toolName: "read_file", content: JSON.stringify({ path: "a.md", content: "# Source A\nText" }), durationMs: 0 })];
  const rows = auditEntries(raw);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toMatchObject({ callId: "a", arguments: { path: "a.md" }, result: { path: "a.md", content: "# Source A\nText" }, outcome: "completed", durationMs: 0 });
  expect(rows[1]).toMatchObject({ callId: "b", result: "missing b.md", outcome: "failed" });
  expect(rows[0].records).toEqual([raw[0], raw[2]]);
});

test("missing or ambiguous identity never pairs a tool by its name", () => {
  const rows = auditEntries([
    msg(1, { role: "assistant", content: [{ type: "toolCall", name: "read_file", arguments: { path: "a.md" } }, { type: "toolCall", id: "duplicate", name: "read_file" }, { type: "toolCall", id: "duplicate", name: "read_file" }] }),
    msg(2, { role: "toolResult", toolName: "read_file", content: "unbound legacy result" }),
    msg(3, { role: "toolResult", toolCallId: "duplicate", toolName: "read_file", content: "ambiguous result" }),
  ]);
  expect(rows).toHaveLength(5);
  expect(rows.slice(0, 3).map(row => row.outcome)).toEqual([undefined, undefined, undefined]);
  expect(rows[3].result).toBe("unbound legacy result");
  expect(rows[4].result).toBe("ambiguous result");
});

test("submission and receipt join the exact proposal without duplicating the tool action", () => {
  const rows = auditEntries([
    msg(1, { role: "assistant", content: [{ type: "toolCall", id: "call-a", name: "submit_plan", arguments: { plan: { summary: "first" } } }, { type: "toolCall", id: "call-b", name: "submit_plan", arguments: { plan: { summary: "second" } } }] }),
    msg(2, { name: "submit_plan", proposal_id: "proposal-a", tool_call_id: "call-a" }, "proposal_submitted"),
    msg(3, { name: "submit_plan", proposal_id: "proposal-b", tool_call_id: "call-b" }, "proposal_submitted"),
    msg(4, { name: "submit_plan", proposal_id: "proposal-b", result: { accepted: true, plan_revision_id: "plan-b" } }, "proposal_result"),
    msg(5, { name: "submit_plan", proposal_id: "proposal-a", result: { error: "invalid target" } }, "proposal_result"),
  ]);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toMatchObject({ kind: "proposal", outcome: "rejected", result: { error: "invalid target" } });
  expect(rows[1]).toMatchObject({ kind: "proposal", outcome: "accepted", result: { plan_revision_id: "plan-b" } });
});

test("only complete consistent chunks become a readable message and keep every source record", () => {
  const chunk = (sequence: number, index: number, text: string, total = 2) => msg(sequence, { message_id: "m", kind: "message", index, total, text }, "message_chunk");
  const a = chunk(1, 0, '{"role":"assistant","content":"Hello '), b = chunk(2, 1, 'world"}');
  expect(auditEntries([a])[0].kind).toBe("partial");
  const complete = auditEntries([a, b])[0];
  expect(complete).toMatchObject({ id: "message/1", kind: "assistant", text: "Hello world" });
  expect(complete.records).toEqual([a, b]);
  expect(auditEntries([a, chunk(2, 1, 'world"}', 3)])[0].kind).toBe("partial");
  expect(auditEntries([a, chunk(2, 0, "conflicting"), b])[0].kind).toBe("partial");
  expect(auditEntries([chunk(1, -1, "invalid")])[0].kind).toBe("partial");
});

test("fixed Worker budget reminders are platform hints while ordinary user text stays user text", () => {
  const rows = auditEntries([
    msg(1, { role: "user", content: "Planning budget: 1 model call remains, including the next request; 12 seconds remain. Finish now with submit_plan with recipe.version=1 or request_input for a necessary question." }),
    msg(2, { role: "user", content: "Planning budget: prioritize native builds." }),
    msg(3, { role: "user", content: [{ type: "text", text: JSON.stringify({ build_id: "build-a", goal: "Build the application" }) }] }),
    msg(4, { role: "assistant", content: [{ type: "thinking", thinking: "provider-private" }] }),
  ]);
  expect(rows.map(row => row.kind)).toEqual(["runtime", "user", "context", "assistant"]);
  expect(rows[2].result).toEqual({ build_id: "build-a", goal: "Build the application" });
  expect(rows[3].text).toBe("");
  expect(rows[3].records[0].payload).toEqual({ role: "assistant", content: [{ type: "thinking", thinking: "provider-private" }] });
});
