/**
 * [INPUT]: 依赖节点身份、主机遥测快照和节点详情展示组件。
 * [OUTPUT]: 提供资源指标、成对内存负载字段、设备计数与采样诊断。
 * [POS]: 节点详情的资源分区；保留采样时效判断和原始数据入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Activity, Cpu, HardDrive } from "lucide-react";
import {
  NodeDisclosure,
  NodeFacts,
  NodeRawData,
  NodeSection,
  NodeTimestamp,
} from "@/components/node-detail-parts";
import { useUI } from "@/components/providers";
import type { Node, NodeTelemetry } from "@/lib/types";
import { bytes, cn } from "@/lib/utils";

export function NodeResources({
  node,
  telemetry,
  failed,
}: {
  node: Node;
  telemetry?: NodeTelemetry;
  failed: boolean;
}) {
  const { t, number } = useUI();
  const host = telemetry?.host;
  if (!host) return <p className="node-empty">{t("nodes.noResourceSample")}</p>;
  const sample = host.sample;
  const formatBytes = (value?: number | null) =>
    value == null ? t("nodes.unknown") : bytes(value);
  const numeric = (value?: number | null) =>
    value == null ? t("nodes.unknown") : number(value);
  const percent = (value?: number | null) =>
    value == null
      ? t("nodes.unknown")
      : `${number(Math.round(value * 10) / 10)}%`;
  const recent = (value?: string | null) => {
    const age = value ? Date.now() - Date.parse(value) : NaN;
    return Number.isFinite(age) && age < 90000 && age >= -30000;
  };
  const fresh =
    !failed &&
    node.online &&
    host.fresh === true &&
    recent(host.sampled_at) &&
    recent(host.received_at);
  const metrics = [
    {
      label: `CPU ${t("nodes.cpuUsage")}`,
      value: percent(sample.cpu_percent),
      icon: Cpu,
    },
    {
      label: t("nodes.availableMemory"),
      value: formatBytes(sample.available_memory_bytes),
      icon: Activity,
    },
    {
      label: t("nodes.disk"),
      value: formatBytes(sample.build_disk_available_bytes),
      icon: HardDrive,
    },
  ];
  return (
    <div className="node-section-stack">
      <div
        className={cn("node-resource-freshness", fresh && "is-fresh")}
        role="status"
      >
        <span className="font-medium">
          {t(fresh ? "nodes.freshSample" : "nodes.staleSample")}
        </span>
        <span>
          {t("nodes.sampledAt")} <NodeTimestamp value={host.sampled_at} />
        </span>
      </div>
      <div className="node-resource-grid">
        {metrics.map(({ label, value, icon: Icon }) => (
          <div className="node-resource-metric" key={label}>
            <span>
              <Icon size={15} aria-hidden="true" />
              {label}
            </span>
            <strong>{value}</strong>
          </div>
        ))}
      </div>
      <NodeSection title={t("nodes.memoryAndLoad")}>
        <div className="node-resource-facts">
          <NodeFacts
            entries={[
              [t("nodes.memoryUsed"), formatBytes(sample.memory_used_bytes)],
              [t("nodes.memoryTotal"), formatBytes(sample.memory_total_bytes)],
              [t("nodes.swapUsed"), formatBytes(sample.swap_used_bytes)],
              [t("nodes.swapTotal"), formatBytes(sample.swap_total_bytes)],
              [t("nodes.loadOne"), numeric(sample.load_one)],
              [t("nodes.activeAttempts"), numeric(sample.active_attempts)],
            ]}
          />
        </div>
      </NodeSection>
      <NodeSection title={t("nodes.disks")}>
        {sample.disks?.length ? (
          <div className="node-detail-table">
            <table>
              <thead>
                <tr>
                  {[
                    t("nodes.device"),
                    t("nodes.mount"),
                    t("nodes.total"),
                    t("nodes.available"),
                    t("nodes.readBytes"),
                    t("nodes.writtenBytes"),
                  ].map((label) => (
                    <th key={label}>{label}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {sample.disks.map((disk, index) => (
                  <tr key={`${disk.device}:${disk.mount}:${index}`}>
                    <td>{disk.device}</td>
                    <td>{disk.mount}</td>
                    <td>{formatBytes(disk.total_bytes)}</td>
                    <td>{formatBytes(disk.available_bytes)}</td>
                    <td>{formatBytes(disk.read_bytes)}</td>
                    <td>{formatBytes(disk.written_bytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="node-empty">{t("nodes.notReported")}</p>
        )}
      </NodeSection>
      <NodeSection title={t("nodes.networks")}>
        <p className="mb-3 text-xs text-muted-foreground">
          {t("nodes.cumulativeCounters")}
        </p>
        {sample.networks?.length ? (
          <div className="node-detail-table">
            <table>
              <thead>
                <tr>
                  {[
                    t("nodes.interface"),
                    t("nodes.receivedBytes"),
                    t("nodes.transmittedBytes"),
                    t("nodes.receiveErrors"),
                    t("nodes.transmitErrors"),
                  ].map((label) => (
                    <th key={label}>{label}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {sample.networks.map((network) => (
                  <tr key={network.interface}>
                    <td>{network.interface}</td>
                    <td>{formatBytes(network.received_bytes)}</td>
                    <td>{formatBytes(network.transmitted_bytes)}</td>
                    <td>{numeric(network.receive_errors)}</td>
                    <td>{numeric(network.transmit_errors)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="node-empty">{t("nodes.notReported")}</p>
        )}
      </NodeSection>
      <NodeSection title={t("nodes.accelerators")}>
        {sample.accelerators?.length ? (
          <div className="node-detail-table">
            <table>
              <thead>
                <tr>
                  {[
                    t("nodes.device"),
                    t("nodes.cpuUsage"),
                    t("nodes.memoryUsed"),
                  ].map((label) => (
                    <th key={label}>{label}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {sample.accelerators.map((accelerator) => (
                  <tr key={accelerator.id}>
                    <td>{accelerator.id}</td>
                    <td>{percent(accelerator.utilization_percent)}</td>
                    <td>{formatBytes(accelerator.memory_used_bytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="node-empty">{t("nodes.notReported")}</p>
        )}
      </NodeSection>
      <NodeDisclosure title={t("nodes.sampleDiagnostics")}>
        <NodeFacts
          entries={[
            [t("nodes.sessionId"), host.session_id],
            [
              t("nodes.receivedAt"),
              <NodeTimestamp key="received" value={host.received_at} />,
            ],
            [t("nodes.droppedBatches"), numeric(host.dropped_batches)],
          ]}
        />
      </NodeDisclosure>
      <NodeRawData value={host} />
    </div>
  );
}
