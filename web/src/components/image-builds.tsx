/**
 * [INPUT]: 依赖镜像执行快照和日志 API。
 * [OUTPUT]: 提供镜像候选摘要、平台日志和分发状态。
 * [POS]: 构建输出只读视图；正式镜像分发从已发布 Release 发起。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { useUI } from "@/components/providers";
import { CopyButton, ErrorBox, Status } from "@/components/common";
import { api } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { ImageExecution } from "@/lib/types";

export function ImageBuilds({ executions, registry }: { executions: ImageExecution[]; registry?: string }) {
  const { t } = useUI();
  return <div className="space-y-4 p-5">{executions.map(execution => {
    const origin = execution.destination_credential ? execution.destination_registry : registry;
    const ref = `${origin ? `${origin.replace(/^https?:\/\//, "").replace(/\/$/, "")}/` : ""}${execution.destination_repository}${execution.root_digest ? `@${execution.root_digest}` : ""}`;
    return <article key={execution.id} className="space-y-3 rounded-md border border-border p-4">
      <div className="flex flex-wrap items-center justify-between gap-2"><strong>{execution.spec.name} · {execution.operation === "publish" ? t("images.publication") : t("images.build")}</strong><Status value={execution.status} /></div>
      <p className="break-all font-mono text-xs">{ref} {origin && <CopyButton value={ref} />}</p>
      {!!execution.tags.length && <p className="font-mono text-xs">{execution.tags.join(" · ")}</p>}
      <p className="text-xs text-muted-foreground">{execution.spec.platforms.join(" · ")}</p>
      {execution.error && <p role="alert" className="text-sm text-destructive">{execution.error}</p>}
      <ImageLogs execution={execution} />
    </article>;
  })}</div>;
}
function ImageLogs({ execution }: { execution: ImageExecution }) {
  const { t } = useUI();
  const [open, setOpen] = useState(false);
  const [logs, setLogs] = useState<{ id: number; platform: string; text: string }[]>([]);
  const [error, setError] = useState<Error>();
  useEffect(() => {
    if (!open) return;
    const controller = new AbortController(); let after = 0; let timer: ReturnType<typeof setTimeout> | undefined;
    async function load() {
      try {
        const page = await api<{ id: number; platform: string; text: string }[]>(`/image-executions/${execution.id}/logs?after=${after}`, { signal: controller.signal });
        if (controller.signal.aborted) return;
        if (page.length) { after = page.at(-1)!.id; setLogs(previous => [...previous, ...page].slice(-2000)); }
        if (page.length === 100 || ["preparing", "running"].includes(execution.status)) timer = setTimeout(() => void load(), page.length === 100 ? 50 : 3000);
      } catch (error) { if (!controller.signal.aborted) setError(asError(error)); }
    }
    setLogs([]); void load();
    return () => { controller.abort(); if (timer) clearTimeout(timer); };
  }, [open, execution.id, execution.status]);
  return <details onToggle={event => setOpen(event.currentTarget.open)}><summary className="cursor-pointer text-sm">{t("images.logs")}</summary><ErrorBox message={error} />{open && <pre className="log-console max-h-96 overflow-auto">{logs.map(log => `[${log.platform}] ${log.text}`).join("")}</pre>}</details>;
}
