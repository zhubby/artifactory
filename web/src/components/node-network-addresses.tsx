/**
 * [INPUT]: 依赖节点诊断接口快照、时间组件和 UI 词条。
 * [OUTPUT]: 提供 NodeNetworkAddresses 的接口地址与采样状态。
 * [POS]: 概览显示最后上报的 IPv4/IPv6；不据地址推导设备身份。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";
import { NodeTimestamp } from "@/components/node-detail-parts";
import type { Node } from "@/lib/types";

export function NodeNetworkAddresses({ node }: { node: Node }) {
  const { t } = useUI();
  const snapshot = node.diagnostics.network_interfaces;
  if (!snapshot) return null;
  return <section className="node-detail-section">
    <h3>{t("nodes.networkAddresses")}</h3>
    <p className="mb-3 text-xs leading-5 text-muted-foreground">{t("nodes.addressSnapshotHint")}</p>
    {snapshot.interfaces.length ? <dl className="node-facts">
      {snapshot.interfaces.map(network => <div key={network.name}>
        <dt>{network.name}</dt>
        <dd>{network.addresses.length ? <ul className="space-y-1">
          {network.addresses.map(address => <li key={address}>
            <span className="mr-2 text-muted-foreground">{address.includes(":") ? "IPv6" : "IPv4"}</span>
            <span className="font-mono">{address}</span>
          </li>)}
        </ul> : t("nodes.notReported")}</dd>
      </div>)}
    </dl> : <p className="node-empty">{t("nodes.notReported")}</p>}
    <p className="mt-3 text-xs leading-5 text-muted-foreground">{t("nodes.sampledAt")}{" "}
      <NodeTimestamp value={snapshot.sampled_at} />
    </p>
    {snapshot.truncated && <p className="mt-2 text-xs text-muted-foreground">{t("nodes.networkTruncated")}</p>}
  </section>;
}
