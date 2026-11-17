/**
 * [INPUT]: 依赖镜像执行、日志与独立发布 API。
 * [OUTPUT]: 提供包含仓库主机的镜像引用、平台日志和发布操作。
 * [POS]: 构建输出的镜像视图；发布重试保留已有平台结果。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { useUI } from "@/components/providers";
import { CopyButton, ErrorBox, Field, Status } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { SubmitForm } from "@/components/forms";
import { api, mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { ImageExecution, ImageCredential } from "@/lib/types";

export function ImageBuilds({ executions, projectId, registry, editable, reload }: {
  executions: ImageExecution[]; projectId: string; registry?: string; editable: boolean; reload: () => Promise<unknown>;
}) {
  const { t } = useUI();
  const [publication, setPublication] = useState<ImageExecution>();
  const [error, setError] = useState<Error>();
  const credentials = useRemote<ImageCredential[]>(publication ? `/projects/${projectId}/image-credentials` : null);
  async function retry(execution: ImageExecution) {
    setError(undefined);
    try { await mutation(`/image-executions/${execution.parent_id || execution.id}/publish`, execution.destination_credential ? {credential_id:execution.destination_credential,repository:execution.destination_repository,tags:execution.tags} : {}); await reload(); }
    catch (error) { setError(asError(error)); }
  }
  return <div className="space-y-4 p-5">
    <ErrorBox message={error} />
    {executions.map(execution => {
      const origin = execution.destination_credential ? execution.destination_registry : registry;
      const ref = `${origin ? `${origin.replace(/^https?:\/\//, "").replace(/\/$/, "")}/` : ""}${execution.destination_repository}${execution.root_digest ? `@${execution.root_digest}` : ""}`;
      return <article key={execution.id} className="space-y-3 rounded-md border border-border p-4">
        <div className="flex flex-wrap items-center justify-between gap-2"><strong>{execution.spec.name} · {execution.operation === "publish" ? t("images.publication") : t("images.build")}</strong><Status value={execution.status} /></div>
        <p className="break-all font-mono text-xs">{ref} {origin && <CopyButton value={ref} />}</p>
        <p className="text-xs text-muted-foreground">{execution.spec.platforms.join(" · ")}</p>
        {execution.error && <p role="alert" className="text-sm text-destructive">{execution.error}</p>}
        <div className="flex flex-wrap gap-2">
          {editable && execution.status === "publish_failed" && <Button size="sm" onClick={() => void retry(execution)}>{t("images.retry")}</Button>}
          {editable && execution.operation === "build" && execution.status === "succeeded" && <Button size="sm" variant="secondary" onClick={() => setPublication(execution)}>{t("images.publishExternal")}</Button>}
        </div>
        <ImageLogs execution={execution} />
      </article>;
    })}
    <Modal open={!!publication} onOpenChange={open => { if (!open) setPublication(undefined); }} title={t("images.publishExternal")} description={t("images.publishDescription")}>
      <ErrorBox message={credentials.error} />
      <SubmitForm label={t("images.publishExternal")} onSubmit={async form => {
        await mutation(`/image-executions/${publication!.id}/publish`, { credential_id: form.get("credential"), repository: form.get("repository"), tags: String(form.get("tags") || "").split(/[,\s]+/).filter(Boolean) });
        setPublication(undefined); await reload();
      }}>
        <Field label={t("images.credential")}><select name="credential" required className="input">{credentials.data?.filter(c => c.kind === "registry" && c.enabled).map(c => <option key={c.id} value={c.id}>{c.name}</option>)}</select></Field>
        <Field label={t("images.repository")}><Input name="repository" required placeholder="team/application" /></Field>
        <Field label={t("images.tags")}><Input name="tags" placeholder="v1.0.0" /></Field>
      </SubmitForm>
    </Modal>
  </div>;
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
