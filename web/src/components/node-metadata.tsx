/**
 * [INPUT]: 依赖节点元数据 API、详情字段和内容骨架。
 * [OUTPUT]: 展示元数据字段、按需复杂值、折叠快照信息和失败重试。
 * [POS]: 详情的按需分区，区分普通值与原始 JSON，刷新保留快照。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useRemote } from "@/lib/api";
import type { NodeMetadataSnapshot } from "@/lib/types";
import { useUI } from "@/components/providers";
import { Loading } from "@/components/common";
import {
  NodeDataError,
  NodeDisclosure,
  NodeFacts,
  NodeTimestamp,
} from "@/components/node-detail-parts";

export function NodeMetadata({ id }: { id: string }) {
  const { t } = useUI();
  const snapshot = useRemote<NodeMetadataSnapshot>(
    `/nodes/${id}/metadata`,
    30000,
  );
  return (
    <div className="node-section-stack">
      <NodeDataError error={snapshot.error} retry={snapshot.reload} />
      {!snapshot.data && !snapshot.error && <Loading variant="detail" />}
      {snapshot.data && (
        <>
          {Object.keys(snapshot.data.metadata).length === 0 ? (
            <p className="node-empty">{t("nodes.notReported")}</p>
          ) : (
            <dl className="node-metadata-list">
              {Object.entries(snapshot.data.metadata)
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([key, value]) => (
                  <div key={key} className={value !== null && typeof value === "object" ? "node-metadata-complex" : undefined}>
                    <dt className={value !== null && typeof value === "object" ? "sr-only" : undefined}>{key}</dt>
                    <dd>
                      {value !== null && typeof value === "object" ? (
                        <NodeDisclosure title={key}><pre>{JSON.stringify(value, null, 2)}</pre></NodeDisclosure>
                      ) : <span>{String(value)}</span>}
                    </dd>
                  </div>
                ))}
            </dl>
          )}
          <NodeDisclosure title={t("nodes.snapshotInfo")}><NodeFacts
            entries={[
              [t("nodes.startupId"), snapshot.data.startup_id],
              [t("nodes.metadataDigest"), snapshot.data.digest],
              [
                t("nodes.receivedAt"),
                <NodeTimestamp
                  key="received"
                  value={snapshot.data.received_at}
                />,
              ],
            ]}
          /></NodeDisclosure>
        </>
      )}
    </div>
  );
}
