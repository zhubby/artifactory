/**
 * [INPUT]: 依赖节点类型、状态组件及 UI 词条。
 * [OUTPUT]: 提供 NodeCard 与节点状态格式函数。
 * [POS]: 设备列表的资源行展示入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { ArrowUpRight, Layers3, Monitor } from "lucide-react";
import { Status } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import type { Node } from "@/lib/types";

export const nodePlatform = (os: string) => ({ darwin: "macOS", linux: "Linux", windows: "Windows" })[os] || os;
export const nodeStatus = (node: Node) => node.revoked ? "revoked" : node.draining || node.ota_draining ? "draining" : node.online ? "online" : "offline";

export function NodeCard({ node, onDetails }: { node: Node; onDetails: () => void }) {
  const { t, number } = useUI();
  const utilization = node.slots > 0 ? Math.min(100, Math.max(0, node.active_jobs / node.slots * 100)) : 0;
  return <article className="node-card" aria-label={node.name}>
    <div className="node-identity">
      <span className="node-icon"><Monitor size={17} aria-hidden="true" /></span>
      <div className="min-w-0">
        <h2 className="truncate text-sm font-medium" title={node.name}>{node.name}</h2>
        <p className="mt-1 font-mono text-[11px] text-muted-foreground">{nodePlatform(node.os)} / {node.arch}</p>
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
      <div className="node-capacity-track" role="meter" aria-label={t("nodes.capacity")}
        aria-valuemin={0} aria-valuemax={Math.max(node.slots, node.active_jobs, 1)} aria-valuenow={node.active_jobs}
        aria-valuetext={t("nodes.capacityValue", { used: node.active_jobs, total: node.slots })}>
        <div className="h-full bg-primary" style={{ width: utilization + "%" }} />
      </div>
    </div>
    <Button variant="ghost" size="sm" onClick={onDetails} aria-label={t("nodes.viewNamed", { name: node.name })}>
      {t("nodes.viewDetails")}<ArrowUpRight size={14} aria-hidden="true" />
    </Button>
  </article>;
}
