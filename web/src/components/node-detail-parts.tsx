/**
 * [INPUT]: 依赖节点事实、全局错误通知与格式化词条。
 * [OUTPUT]: 提供分区、折叠区、对齐字段、时间、重试和原始数据视图。
 * [POS]: 设备详情的展示基础；字段保留零值，折叠状态与轮询隔离。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import type { ReactNode } from "react";
import { ChevronRight, RefreshCw } from "lucide-react";
import { ErrorBox } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";

export function NodeSection({ title, children }: { title: string; children: ReactNode }) {
  return <section className="node-detail-section"><h3>{title}</h3>{children}</section>;
}

export function NodeDisclosure({ title, children }: { title: string; children: ReactNode }) {
  return <details className="node-disclosure">
    <summary><ChevronRight size={14} aria-hidden="true" /><h3>{title}</h3></summary>
    <div className="node-disclosure-content">{children}</div>
  </details>;
}

export function NodeFacts({ entries, optional = false }: {
  entries: [string, ReactNode][];
  optional?: boolean;
}) {
  const { t } = useUI();
  const present = (value: ReactNode) => value !== null && value !== undefined && value !== "";
  return (
    <dl className="node-facts">
      {entries.filter(([, value]) => !optional || present(value)).map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>
            {!present(value) ? t("nodes.unknown") : typeof value === "boolean" ? String(value) : value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function NodeTimestamp({ value }: { value?: string | null }) {
  const { t, locale } = useUI();
  if (!value || !Number.isFinite(Date.parse(value)))
    return <>{t("nodes.unknown")}</>;
  return (
    <time dateTime={value} title={value}>
      {new Intl.DateTimeFormat(locale, {
        dateStyle: "medium",
        timeStyle: "medium",
      }).format(new Date(value))}
    </time>
  );
}

export function NodeDataError({
  error,
  retry,
}: {
  error?: Error;
  retry: () => Promise<unknown>;
}) {
  const { t } = useUI();
  return (
    <>
      <ErrorBox message={error} />
      {error && <div className="space-y-3">
      <p className="text-sm text-muted-foreground">
        {t("nodes.dataUnavailable")}
      </p>
      <Button size="sm" variant="secondary" onClick={() => void retry()}>
        <RefreshCw size={14} />
        {t("nodes.retryData")}
      </Button>
      </div>}
    </>
  );
}

export function NodeRawData({
  value,
  label,
}: {
  value: unknown;
  label?: string;
}) {
  const { t } = useUI();
  return (
    <NodeDisclosure title={label || t("nodes.rawData")}>
      <div className="node-raw-data">
      <pre>{JSON.stringify(value, null, 2)}</pre>
      </div>
    </NodeDisclosure>
  );
}
