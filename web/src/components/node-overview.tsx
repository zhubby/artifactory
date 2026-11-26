/**
 * [INPUT]: 依赖节点快照、可选硬件清单及共享字段和接口地址组件。
 * [OUTPUT]: 提供 NodeOverview 的硬件摘要、网络和按需技术信息。
 * [POS]: 详情概览先呈现设备配置，完整身份和系统字段保留在折叠区。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";
import { NodeDisclosure, NodeFacts, NodeRawData, NodeSection, NodeTimestamp } from "@/components/node-detail-parts";
import { NodeNetworkAddresses } from "@/components/node-network-addresses";
import { nodeHostname } from "@/lib/node-identity";
import type { Node, NodeTelemetry } from "@/lib/types";
import { bytes } from "@/lib/utils";

export function NodeOverview({ node, inventory }: { node: Node; inventory?: NodeTelemetry["inventory"] }) {
  const { t } = useUI();
  const machine = inventory?.inventory;
  const memory = node.identity?.memory_total_bytes ?? machine?.physical_memory_bytes;
  const summary = [
    [t("nodes.model"), node.identity?.device_model],
    [t("nodes.cpuModel"), node.identity?.cpu_model || machine?.cpu_model],
    [t("nodes.physicalMemory"), memory == null ? undefined : bytes(memory)],
    [t("nodes.capacity"), `${node.active_jobs} / ${node.slots}`],
  ];
  return <div className="node-section-stack">
    <NodeSection title={t("nodes.deviceSummary")}>
      <dl className="node-summary-grid">
        {summary.map(([label, value]) => <div key={label}>
          <dt>{label}</dt><dd>{value || t("nodes.unknown")}</dd>
        </div>)}
      </dl>
    </NodeSection>
    <NodeNetworkAddresses node={node} />
    {!!machine?.accelerators?.length && <NodeSection title={t("nodes.accelerators")}>
      <div className="node-accelerator-list">{machine.accelerators.map(device => <div key={device.id}>
        <div className="node-accelerator-heading">
          <strong>{device.model || device.id}</strong>
          {device.memory_total_bytes != null && <span>{bytes(device.memory_total_bytes)}</span>}
        </div>
        <NodeDisclosure title={t("nodes.deviceDetails")}><NodeFacts optional entries={[
          [t("nodes.deviceId"), device.id], [t("nodes.vendor"), device.vendor], [t("nodes.driver"), device.driver],
        ]} /></NodeDisclosure>
      </div>)}</div>
    </NodeSection>}
    {!!Object.keys(node.labels).length && <NodeSection title={t("nodes.detailLabels")}>
      <NodeFacts entries={Object.entries(node.labels)} />
    </NodeSection>}
    <NodeDisclosure title={t("nodes.moreHardware")}>
      <NodeFacts optional entries={[
        [t("nodes.vendor"), node.identity?.device_vendor],
        [t("nodes.osVersion"), machine?.os_version],
        [t("nodes.kernel"), machine?.kernel_version],
        [t("nodes.cpus"), machine?.logical_cpus ?? node.diagnostics.metrics?.logical_cpus],
        [t("nodes.physicalCores"), machine?.physical_cores],
        [t("nodes.cpuLimit"), machine?.effective_cpu_limit],
        [t("nodes.memoryLimit"), machine?.effective_memory_limit_bytes == null ? undefined : bytes(machine.effective_memory_limit_bytes)],
        [t("nodes.container"), machine?.container],
        [t("nodes.virtualization"), machine?.virtualization],
        [t("nodes.bootTime"), machine?.boot_time == null ? undefined : <NodeTimestamp key="boot" value={new Date(machine.boot_time * 1000).toISOString()} />],
      ]} />
      {inventory && <p className="node-section-note">{t("nodes.inventorySampledAt")} <NodeTimestamp value={inventory.sampled_at} /></p>}
      {machine && <NodeRawData value={machine} label={t("nodes.fullInventory")} />}
    </NodeDisclosure>
    <NodeDisclosure title={t("nodes.identity")}>
      <NodeFacts entries={[
        [t("nodes.nodeId"), node.id], [t("nodes.hostname"), nodeHostname(node)],
        [t("nodes.pool"), node.pool_name], [t("nodes.poolId"), node.pool_id],
        [t("nodes.createdAt"), <NodeTimestamp key="created" value={node.created_at} />],
        [t("nodes.lastSeenLabel"), <NodeTimestamp key="seen" value={node.last_seen} />],
        [t("nodes.certificateLabel"), <NodeTimestamp key="certificate" value={node.certificate_expires_at} />],
      ]} />
    </NodeDisclosure>
  </div>;
}
