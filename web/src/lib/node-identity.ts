/**
 * [INPUT]: 依赖公开 Node 类型与字节格式化工具。
 * [OUTPUT]: 提供主机名兼容读取与去重硬件摘要。
 * [POS]: 列表与详情共用的身份解释；只消费节点快照。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import type { Node } from "@/lib/types";
import { bytes } from "@/lib/utils";

export function nodeHostname(node: Node) {
  return node.identity?.hostname?.trim() || node.diagnostics.hostname?.trim() || undefined;
}

export function nodeHardware(node: Node) {
  const identity = node.identity;
  const seen = new Set([node.name.trim().toLocaleLowerCase(), nodeHostname(node)?.toLocaleLowerCase()]);
  return [identity?.device_model, identity?.cpu_model,
    identity?.memory_total_bytes != null ? bytes(identity.memory_total_bytes) : undefined,
  ].flatMap(value => {
    const text = value?.trim();
    if (!text || seen.has(text.toLocaleLowerCase())) return [];
    seen.add(text.toLocaleLowerCase());
    return [text];
  });
}
