/**
 * [INPUT]: 依赖节点诊断、库存工具快照和节点详情展示组件。
 * [OUTPUT]: 提供 NodeTools 的能力、工具列表与签名授权信息。
 * [POS]: 节点详情的环境分区；库存字段覆盖同名诊断字段。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import {
  NodeDisclosure,
  NodeFacts,
  NodeSection,
} from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import type { Node, NodeInventory } from "@/lib/types";
import { cn } from "@/lib/utils";

function Tags({ values }: { values: string[] }) {
  const { t } = useUI();
  return values.length ? (
    <div className="flex flex-wrap gap-2">
      {values.map((value) => (
        <span className="tag max-w-full break-all" key={value}>
          {value}
        </span>
      ))}
    </div>
  ) : (
    <p className="node-empty">{t("nodes.notReported")}</p>
  );
}

export function NodeTools({
  node,
  inventory,
}: {
  node: Node;
  inventory?: NodeInventory;
}) {
  const { t } = useUI();
  const diagnostics = node.diagnostics.tools || {};
  const reported = inventory?.tools || {};
  const tools = [
    ...new Set([...Object.keys(diagnostics), ...Object.keys(reported)]),
  ].sort();
  return (
    <div className="node-section-stack">
      <NodeSection title={t("nodes.capabilities")}>
        <Tags
          values={[
            ...new Set([
              ...node.capabilities,
              ...(inventory?.capabilities || []),
            ]),
          ]}
        />
      </NodeSection>
      <NodeSection title={t("nodes.tools")}>
        {tools.length ? (
          <div className="node-tool-list">
            {tools.map((name) => {
              const tool = { ...diagnostics[name], ...reported[name] };
              return (
                <div key={name} className="node-tool">
                  <div className="node-tool-heading">
                    <h4>{name}</h4>
                    <span className="node-tool-version">
                      {tool.version || t("nodes.unknown")}
                    </span>
                    <span
                      className={cn(
                        "node-tool-availability",
                        tool.available === true
                          ? "text-success"
                          : "text-muted-foreground",
                      )}
                    >
                      {t(
                        tool.available === true
                          ? "nodes.toolAvailable"
                          : tool.available === false
                            ? "nodes.toolUnavailable"
                            : "nodes.unknown",
                      )}
                    </span>
                  </div>
                  {tool.path && <NodeDisclosure title={t("nodes.toolPath")}>
                    <p className="node-tool-path">
                      {tool.path || t("nodes.unknown")}
                    </p>
                  </NodeDisclosure>}
                </div>
              );
            })}
          </div>
        ) : (
          <p className="node-empty">{t("nodes.notReported")}</p>
        )}
      </NodeSection>
      <NodeSection title={t("nodes.signingAuthorization")}>
        <Tags values={node.signing_profiles} />
      </NodeSection>
      <NodeSection title={t("nodes.configuredSigning")}>
        <Tags
          values={
            node.diagnostics.configured_signing_profiles ||
            inventory?.signing_profile_refs ||
            []
          }
        />
      </NodeSection>
      {!!Object.keys(inventory?.collectors || {}).length && (
        <NodeSection title={t("nodes.collectors")}>
          <NodeFacts entries={Object.entries(inventory!.collectors!)} />
        </NodeSection>
      )}
    </div>
  );
}
