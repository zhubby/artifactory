/**
 * [INPUT]: 依赖节点快照、身份摘要、状态组件及 UI 词条。
 * [OUTPUT]: 提供 NodeCard 与节点状态格式函数。
 * [POS]: Standard 设备卡；身份、硬件与容量只消费列表快照。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Apple, ArrowUpRight, Layers3, Monitor, Terminal } from "lucide-react";
import { Status } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import type { Node } from "@/lib/types";
import { nodeHardware, nodeHostname } from "@/lib/node-identity";

export const nodePlatform = (os: string) => ({ darwin: "macOS", linux: "Linux", windows: "Windows" })[os] || os;
export const nodeStatus = (node: Node) => node.revoked ? "revoked" : node.draining || node.ota_draining ? "draining" : node.online ? "online" : "offline";

export function NodeCard({ node, onDetails }: { node: Node; onDetails: () => void }) {
  const { t, number } = useUI();
  const utilization = node.slots > 0 ? Math.min(100, Math.max(0, node.active_jobs / node.slots * 100)) : 0;
  const hostname = nodeHostname(node);
  const identity = [hostname && hostname !== node.name ? hostname : undefined, `${nodePlatform(node.os)} / ${node.arch}`, node.id.slice(0, 8)].filter(Boolean).join(" · ");
  const hardware = nodeHardware(node);
  const PlatformIcon = node.os === "darwin" ? Apple : node.os === "linux" ? Terminal : Monitor;
  return <article className="node-card" aria-label={node.name} data-node-platform={node.os}>
    <div className="node-identity">
      <span className="node-icon"><PlatformIcon size={23} aria-hidden="true" /></span>
      <div className="min-w-0">
        <h2 className="truncate text-sm font-medium" title={node.name}>{node.name}</h2>
        <p className="node-identity-line mt-1 font-mono text-[11px] text-muted-foreground" title={identity}>{identity}</p>
        {!!hardware.length && <p className="node-identity-line mt-1 text-[11px] text-muted-foreground" title={hardware.join(" · ")}>{hardware.join(" · ")}</p>}
      </div>
    </div>
    <div className="node-status"><Status small value={nodeStatus(node)} /></div>
    <div className="node-pool flex min-w-0 items-center gap-2 text-xs text-muted-foreground" title={node.pool_name}>
      <Layers3 size={14} className="shrink-0" aria-hidden="true" /><span className="truncate">{node.pool_name}</span>
    </div>
    <div className="node-capacity">
      <div className="mb-2 flex justify-between gap-2 text-[11px] text-muted-foreground">
        <span>{t("nodes.capacity")}</span><span className="font-mono tabular-nums">{number(node.active_jobs)} / {number(node.slots)}</span>
      </div>
      <div className="node-capacity-track" data-empty={node.active_jobs === 0 || undefined} role="meter" aria-label={t("nodes.capacity")}
        aria-valuemin={0} aria-valuemax={Math.max(node.slots, node.active_jobs, 1)} aria-valuenow={node.active_jobs}
        aria-valuetext={t("nodes.capacityValue", { used: node.active_jobs, total: node.slots })}>
        <div className="node-capacity-fill" style={{ width: utilization + "%" }} />
      </div>
    </div>
    <Button variant="ghost" size="sm" onClick={onDetails} aria-label={t("nodes.viewNamed", { name: node.name })}>
      {t("nodes.viewDetails")}<ArrowUpRight size={14} aria-hidden="true" />
    </Button>
  </article>;
}
