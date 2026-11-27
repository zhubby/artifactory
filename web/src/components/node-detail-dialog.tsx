/**
 * [INPUT]: 依赖节点快照、遥测请求、各标签展示组件和重命名控件。
 * [OUTPUT]: 提供 NodeDetailDialog 的固定标题、标签导航与设备详情。
 * [POS]: 组合设备档案侧面板，维持轮询生命周期和键盘焦点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useRef, useState } from "react";
import { Activity, Database, Info, Monitor, ShieldCheck, Terminal } from "lucide-react";
import { Loading, Status } from "@/components/common";
import { nodePlatform, nodeStatus } from "@/components/node-card";
import { NodeDataError } from "@/components/node-detail-parts";
import { NodeOverview } from "@/components/node-overview";
import { NodeResources } from "@/components/node-resources";
import { NodeTools } from "@/components/node-tools";
import { NodeOperations } from "@/components/node-operations";
import { NodeMetadata } from "@/components/node-metadata";
import { NodeRename } from "@/components/node-rename";
import { useUI } from "@/components/providers";
import { Modal } from "@/components/ui/dialog";
import { useRemote } from "@/lib/api";
import { nodeHostname } from "@/lib/node-identity";
import type { Node, NodeTelemetry, Pool, User } from "@/lib/types";

export function NodeDetailDialog({
  node,
  user,
  pools,
  onClose,
  reload,
}: {
  node: Node;
  user: User;
  pools: Pool[];
  onClose: () => void;
  reload: () => Promise<unknown>;
}) {
  const { t } = useUI();
  const tabs = [
    { id: "overview", label: t("nodes.tabs.overview"), icon: Info },
    { id: "resources", label: t("nodes.tabs.resources"), icon: Activity },
    { id: "tools", label: t("nodes.tabs.tools"), icon: Terminal },
    { id: "metadata", label: t("nodes.tabs.metadata"), icon: Database },
    { id: "operations", label: t("nodes.tabs.operations"), icon: ShieldCheck },
  ] as const;
  const [tab, setTab] = useState<(typeof tabs)[number]["id"]>("overview");
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const panelRef = useRef<HTMLDivElement>(null);
  const id = useId();
  const telemetry = useRemote<NodeTelemetry>(
    `/nodes/${node.id}/telemetry`,
    30000,
  );
  const inventory = telemetry.data?.inventory?.inventory;
  useEffect(() => {
    panelRef.current?.parentElement?.scrollTo({ top: 0 });
  }, [tab]);
  async function refresh() {
    await Promise.all([reload(), telemetry.reload()]);
  }
  return (
    <Modal
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      size="wide"
      placement="side"
      title={
        <span className="node-detail-heading">
          <span className="node-detail-icon"><Monitor size={22} aria-hidden="true" /></span>
          <span className="node-detail-name">{node.name}</span>
          <Status small value={nodeStatus(node)} />
          {user.role === "admin" && <NodeRename node={node} reload={refresh} />}
        </span>
      }
      description={[
        nodeHostname(node) !== node.name.trim() ? nodeHostname(node) : undefined,
        `${nodePlatform(node.os)} / ${node.arch}`,
        node.pool_name,
      ].filter(Boolean).join(" · ")}
      navigation={
        <div
          className="node-detail-tabs"
          role="tablist"
          aria-label={t("nodes.detailTabs")}
        >
          {tabs.map((entry, index) => (
            <button
              key={entry.id}
              type="button"
              role="tab"
              id={`${id}-${entry.id}`}
              aria-controls={`${id}-panel`}
              aria-selected={tab === entry.id}
              tabIndex={tab === entry.id ? 0 : -1}
              ref={(element) => {
                tabRefs.current[index] = element;
              }}
              onClick={() => setTab(entry.id)}
              onKeyDown={(event) => {
                const next =
                  event.key === "ArrowRight"
                    ? (index + 1) % tabs.length
                    : event.key === "ArrowLeft"
                      ? (index + tabs.length - 1) % tabs.length
                      : event.key === "Home"
                        ? 0
                        : event.key === "End"
                          ? tabs.length - 1
                          : null;
                if (next === null) return;
                event.preventDefault();
                setTab(tabs[next].id);
                tabRefs.current[next]?.focus();
                tabRefs.current[next]?.scrollIntoView({
                  block: "nearest",
                  inline: "nearest",
                });
              }}
            >
              <entry.icon size={15} aria-hidden="true" />
              {entry.label}
            </button>
          ))}
        </div>
      }
    >
      <div
        ref={panelRef}
        role="tabpanel"
        id={`${id}-panel`}
        aria-labelledby={`${id}-${tab}`}
        tabIndex={0}
        className="node-detail-panel"
      >
        {tab !== "metadata" && tab !== "operations" && (
          <div className="mb-5 empty:hidden">
            <NodeDataError error={telemetry.error} retry={telemetry.reload} />
            {!telemetry.data && !telemetry.error && (
              <Loading variant="detail" />
            )}
          </div>
        )}
        {tab === "overview" && (
          <NodeOverview node={node} inventory={telemetry.data?.inventory} />
        )}
        {tab === "resources" && telemetry.data && (
          <NodeResources
            node={node}
            telemetry={telemetry.data}
            failed={!!telemetry.error}
          />
        )}
        {tab === "tools" && <NodeTools node={node} inventory={inventory} />}
        {tab === "metadata" && <NodeMetadata id={node.id} />}
        {tab === "operations" && (
          <NodeOperations node={node} user={user} pools={pools} reload={refresh} />
        )}
      </div>
    </Modal>
  );
}
