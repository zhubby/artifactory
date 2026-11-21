/**
 * [INPUT]: 依赖节点诊断接口快照、时间组件和 UI 词条。
 * [OUTPUT]: 提供按接口对齐的地址行、采样时间和截断状态。
 * [POS]: 概览在设备摘要后显示最后地址快照；地址不承担身份识别。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";
import { NodeSection, NodeTimestamp } from "@/components/node-detail-parts";
import type { Node } from "@/lib/types";

export function NodeNetworkAddresses({ node }: { node: Node }) {
  const { t } = useUI();
  const snapshot = node.diagnostics.network_interfaces;
  if (!snapshot) return null;
  return <NodeSection title={t("nodes.networkAddresses")}>
    {snapshot.interfaces.length ? <dl className="node-facts node-network-list">
      {snapshot.interfaces.map(network => <div key={network.name}>
        <dt>{network.name}</dt>
        <dd>{network.addresses.length ? <ul className="node-addresses">
          {network.addresses.map(address => <li key={address}>
            <span className="node-address-family">{address.includes(":") ? "IPv6" : "IPv4"}</span>
            <span>{address}</span>
          </li>)}
        </ul> : t("nodes.notReported")}</dd>
      </div>)}
    </dl> : <p className="node-empty">{t("nodes.notReported")}</p>}
    <p className="node-section-note">{t("nodes.addressSnapshotHint")}</p>
    <p className="node-section-note">{t("nodes.sampledAt")}{" "}
      <NodeTimestamp value={snapshot.sampled_at} />
    </p>
    {snapshot.truncated && <p className="node-section-note">{t("nodes.networkTruncated")}</p>}
  </NodeSection>;
}
