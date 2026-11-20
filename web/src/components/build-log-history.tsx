/**
 * [INPUT]: 依赖鉴权日志分页 API、内容骨架和目标步骤筛选。
 * [OUTPUT]: 提供无轮询的历史日志与首次读取骨架。
 * [POS]: 终态日志阅读器；向前加载时保留阅读锚点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useUI } from "@/components/providers";
import { CopyButton, ErrorBox, Loading, SectionTitle } from "@/components/common";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { BuildLog } from "@/lib/build-state";

interface LogPage { logs: BuildLog[]; next_before: number | null }

export function BuildLogHistory({ buildId, attemptId, step }: {
  buildId: string; attemptId?: string; step?: string;
}) {
  const { t } = useUI();
  const [logs, setLogs] = useState<BuildLog[]>([]);
  const [before, setBefore] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error>();
  const terminal = useRef<HTMLDivElement>(null);
  const request = useRef<AbortController | null>(null);
  const anchor = useRef<{ top: number; height: number; viewport: number } | null>(null);
  const query = new URLSearchParams();
  if (attemptId) query.set("attempt_id", attemptId);
  if (step) query.set("step", step);
  const path = `/builds/${buildId}/logs?${query}`;

  const load = useCallback(async (cursor: number | null, controller: AbortController) => {
    setLoading(true);
    setError(undefined);
    try {
      const page = await api<LogPage>(path + (cursor === null ? "" : `&before=${cursor}`), { signal: controller.signal });
      if (controller.signal.aborted) return;
      const node = terminal.current;
      anchor.current = cursor !== null && node ? { top: node.scrollTop, height: node.scrollHeight, viewport: node.getBoundingClientRect().top } : null;
      setLogs(previous => cursor === null ? page.logs : [...page.logs, ...previous]);
      setBefore(page.next_before);
    } catch (error) {
      if (!controller.signal.aborted) setError(asError(error));
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }, [path]);
  useEffect(() => {
    const controller = new AbortController();
    request.current = controller;
    setLogs([]);
    setBefore(null);
    void load(null, controller);
    return () => controller.abort();
  }, [load]);
  useLayoutEffect(() => {
    const node = terminal.current;
    if (!node) return;
    node.scrollTop = anchor.current
      ? anchor.current.top + node.scrollHeight - anchor.current.height + node.getBoundingClientRect().top - anchor.current.viewport
      : node.scrollHeight;
    anchor.current = null;
  }, [logs]);
  return <section className="panel min-w-0">
    <SectionTitle live title={step ? t("builds.filteredLogs", { step }) : t("builds.logs")} detail={t("builds.history")}>
      <CopyButton value={logs.map(log => log.text).join("")} />
    </SectionTitle>
    <ErrorBox message={error} />
    {(before !== null || error) && <div className="border-b border-border px-5 py-2">
      <Button size="sm" variant="secondary" disabled={loading} onClick={() => { if (request.current) void load(before, request.current); }}>
        {error ? t("common.retry") : t("builds.olderLogs")}
      </Button>
    </div>}
    <div className="log-console" ref={terminal} role="log" aria-label={t("builds.logLabel")} aria-live="off" aria-busy={loading}>
      {logs.length ? logs.map(log => <span key={log.id} data-log-id={log.id}>
        {!step && <span className="mr-3 select-none text-log-muted">[{log.step}]</span>}{log.text}
      </span>) : loading ? <Loading variant="log" /> : !error && <span className="text-muted-foreground">{t("builds.noLogs")}</span>}
    </div>
  </section>;
}
