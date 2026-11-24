/**
 * [INPUT]: 依赖按 sequence 排序的持久审计消息和工具调用身份。
 * [OUTPUT]: 提供分片合并、平台提示识别和调用结果关联的阅读投影。
 * [POS]: 审计展示的数据边界；保留原文，不按工具名称推测对应关系。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { ConversationMessage } from "@/lib/types";

export type AuditKind = "system" | "context" | "user" | "assistant" | "tool" | "proposal" | "runtime" | "partial" | "unknown";
export interface AuditEntry {
  id: string;
  sequence: number;
  kind: AuditKind;
  text: string;
  name?: string;
  callId?: string;
  arguments?: unknown;
  result?: unknown;
  outcome?: "completed" | "failed" | "accepted" | "rejected";
  durationMs?: number;
  records: ConversationMessage[];
}

export function auditObject(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
}

export function auditText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  return value.flatMap(block => {
    const item = auditObject(block);
    return item?.type === "text" && typeof item.text === "string" ? [item.text] : [];
  }).join("\n\n");
}

export function auditJson(text: string): unknown {
  try { return JSON.parse(text); } catch { return undefined; }
}

interface AuditMessage { message: ConversationMessage; records: ConversationMessage[]; partial?: boolean }
function combine(messages: ConversationMessage[]): AuditMessage[] {
  const output: AuditMessage[] = [];
  const chunks = new Map<string, { item: AuditMessage; parts: Map<number, string>; total: number; kind: string; invalid: boolean }>();
  for (const message of messages) {
    const p = message.payload;
    if (message.kind !== "message_chunk" || typeof p.message_id !== "string"
      || !Number.isInteger(p.total) || Number(p.total) < 1 || Number(p.total) > 8192
      || !Number.isInteger(p.index) || Number(p.index) < 0 || Number(p.index) >= Number(p.total)
      || typeof p.text !== "string" || typeof p.kind !== "string") {
      output.push({ message, records: [message], partial: message.kind === "message_chunk" });
      continue;
    }
    let group = chunks.get(p.message_id);
    if (!group) {
      const item = { message, records: [] as ConversationMessage[], partial: true };
      group = { item, parts: new Map(), total: Number(p.total), kind: p.kind, invalid: false };
      chunks.set(p.message_id, group);
      output.push(item);
    }
    group.item.records.push(message);
    if (group.total !== p.total || group.kind !== p.kind || (group.parts.has(Number(p.index)) && group.parts.get(Number(p.index)) !== p.text)) group.invalid = true;
    group.parts.set(Number(p.index), p.text);
  }
  for (const group of chunks.values()) {
    if (group.invalid || group.parts.size !== group.total) continue;
    const value = auditObject(auditJson(Array.from({ length: group.total }, (_, index) => group.parts.get(index)).join("")));
    if (!value) continue;
    group.item.message = { ...group.item.message, kind: group.kind, payload: value };
    group.item.partial = false;
  }
  return output;
}

// 旧 Worker 以 user 角色注入预算提示。只识别它的固定完整前缀。
const budgetHint = /^Planning budget: \d+ model calls? remains?, including the next request; \d+ seconds remain\. (?:Keep (?:source reads|node queries) focused and reserve calls to finish with |Finish now with )/;

export function auditEntries(messages: ConversationMessage[]): AuditEntry[] {
  const combined = combine(messages);
  const output: AuditEntry[] = [];
  const calls = new Map<string, AuditEntry[]>();
  const proposals = new Map<string, AuditEntry[]>();
  const deferred: AuditMessage[] = [];
  const add = (item: AuditMessage, kind: AuditKind, suffix = ""): AuditEntry => {
    const entry: AuditEntry = { id: `message/${item.message.sequence}${suffix}`, sequence: item.message.sequence, kind, text: "", records: [...item.records] };
    output.push(entry);
    return entry;
  };
  const register = (map: Map<string, AuditEntry[]>, id: unknown, entry: AuditEntry) => {
    if (typeof id === "string" && id) map.set(id, [...(map.get(id) || []), entry]);
  };
  const unique = (map: Map<string, AuditEntry[]>, id: unknown) => typeof id === "string" && map.get(id)?.length === 1 ? map.get(id)![0] : undefined;
  const attach = (entry: AuditEntry, item: AuditMessage) => {
    for (const record of item.records) if (!entry.records.some(prior => prior.sequence === record.sequence)) entry.records.push(record);
  };
  for (const item of combined) {
    const { message, partial } = item;
    const p = message.payload;
    if (partial) { add(item, "partial"); continue; }
    if (["proposal_submitted", "proposal_result"].includes(message.kind) || p.role === "toolResult" || p.role === "tool") { deferred.push(item); continue; }
    const text = auditText(p.content) || (typeof p.text === "string" ? p.text : "");
    const blocks = Array.isArray(p.content) ? p.content.map(auditObject).filter(block => block?.type === "toolCall") : [];
    if (p.role === "assistant") {
      if (text || !blocks.length) add(item, "assistant").text = text;
      blocks.forEach((block, index) => {
        const entry = add(item, "tool", `/tool/${index}`);
        entry.name = typeof block?.name === "string" ? block.name : undefined;
        entry.callId = typeof block?.id === "string" ? block.id : undefined;
        entry.arguments = block?.arguments;
        register(calls, entry.callId, entry);
      });
      continue;
    }
    const context = auditObject(auditJson(text));
    const kind: AuditKind = message.kind === "system" || p.role === "system" ? "system"
      : p.role === "user" && budgetHint.test(text) ? "runtime"
      : p.role === "user" && context && typeof context.build_id === "string" && typeof context.goal === "string" ? "context"
      : p.role === "user" ? "user" : "unknown";
    const entry = add(item, kind);
    entry.text = text;
    if (kind === "context") entry.result = context;
  }
  for (const item of deferred.filter(item => item.message.kind === "proposal_submitted")) {
    const p = item.message.payload;
    const entry = unique(calls, p.tool_call_id) || add(item, "proposal");
    attach(entry, item);
    entry.kind = "proposal";
    entry.name = typeof p.name === "string" ? p.name : entry.name;
    entry.arguments ??= p.arguments;
    register(proposals, p.proposal_id, entry);
  }
  for (const item of deferred.filter(item => item.message.kind !== "proposal_submitted")) {
    const p = item.message.payload;
    const proposal = item.message.kind === "proposal_result";
    const entry = (proposal ? unique(proposals, p.proposal_id) : unique(calls, p.toolCallId)) || add(item, proposal ? "proposal" : "tool");
    attach(entry, item);
    entry.name ??= typeof (p.toolName || p.name) === "string" ? String(p.toolName || p.name) : undefined;
    entry.callId ??= typeof p.toolCallId === "string" ? p.toolCallId : undefined;
    const text = auditText(p.content);
    const result = proposal ? p.result ?? p : auditJson(text) ?? (text || p);
    const object = auditObject(result);
    entry.result = result;
    entry.outcome = proposal ? object?.accepted === true ? "accepted" : object?.accepted === false || object?.error ? "rejected" : "completed"
      : p.isError === true || object?.error ? "failed" : "completed";
    if (typeof p.durationMs === "number" && Number.isFinite(p.durationMs) && p.durationMs >= 0) entry.durationMs = p.durationMs;
  }
  return output.sort((a, b) => a.sequence - b.sequence);
}

export function auditSubject(entry: AuditEntry): string {
  const args = auditObject(entry.arguments), result = auditObject(entry.result);
  const value = args?.path ?? args?.query ?? args?.target ?? args?.node_id ?? result?.path;
  if (typeof value === "string") return value;
  if (entry.kind === "context") return typeof result?.goal === "string" ? result.goal : "";
  return entry.text.split(/\r?\n/).find(line => line.trim())?.slice(0, 160) || "";
}
