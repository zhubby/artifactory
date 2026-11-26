/**
 * [INPUT]: 依赖租户节点池 API、共享表单、弹窗和通知。
 * [OUTPUT]: 提供 NodePoolsWorkspace 列表、创建、重命名和删除确认。
 * [POS]: 租户资源入口；设备数量由服务端按当前授权聚合。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useState } from "react";
import { ArrowUpRight, Layers3, Pencil, Plus, Trash2 } from "lucide-react";
import { Empty, ErrorBox, Field, Loading, PageTitle } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ListToolbar } from "@/components/ui/list-toolbar";
import { mutation, useRemote } from "@/lib/api";
import { LocalizedError } from "@/lib/errors";
import { Link } from "@/lib/tenant";
import type { Pool, User } from "@/lib/types";

type PoolAction = { kind: "create" } | { kind: "rename" | "delete"; pool: Pool };

export function NodePoolsWorkspace({ user, readOnly }: { user: User; readOnly: boolean }) {
  const { t } = useUI();
  const pools = useRemote<Pool[]>("/pools");
  const [query, setQuery] = useState("");
  const [action, setAction] = useState<PoolAction>();
  const canManage = user.role === "admin" && !readOnly;
  const visible = pools.data?.filter(pool => `${pool.name} ${pool.id}`.toLowerCase().includes(query.trim().toLowerCase()));
  return <>
    <PageTitle eyebrow={t("workspace.tenantWorkbench")} title={t("nodePools.title")} description={t("nodePools.description")}>
      {canManage && <Button onClick={() => setAction({ kind: "create" })}><Plus />{t("nodePools.create")}</Button>}
    </PageTitle>
    <ListToolbar query={query} onQueryChange={setQuery} count={visible?.length} placeholder={t("nodePools.search")} />
    <ErrorBox message={pools.error} />
    {pools.error && <Button variant="secondary" onClick={() => void pools.reload()}>{t("common.retry")}</Button>}
    {!pools.data ? !pools.error && <Loading variant="cards" /> : !visible?.length ? (
      <section className="panel"><Empty title={t("nodePools.empty")} detail={t(query ? "nodePools.noMatches" : "nodePools.emptyHint")} /></section>
    ) : <div className="grid gap-4 lg:grid-cols-2">
      {visible.map(pool => <article key={pool.id} aria-label={pool.name} className="panel min-w-0 p-5">
        <div className="flex min-w-0 items-start gap-3">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-border bg-secondary"><Layers3 size={18} aria-hidden="true" /></span>
          <div className="min-w-0 flex-1">
            <h2 className="break-words text-sm font-medium">{pool.name}</h2>
            <p className="mt-1 break-all font-mono text-[11px] text-muted-foreground">{pool.id}</p>
          </div>
          {pool.is_default && <span className="shrink-0 rounded border border-border px-2 py-1 text-xs text-muted-foreground">{t("nodePools.default")}</span>}
        </div>
        <p className="mt-5 text-sm text-muted-foreground">{pool.node_count === undefined || pool.online_count === undefined
          ? t("nodePools.countUnavailable") : t("nodePools.devices", { total: pool.node_count, online: pool.online_count })}</p>
        <div className="mt-5 flex flex-wrap items-center gap-2">
          <Button asChild variant="secondary" size="sm"><Link href={`/nodes?pool=${encodeURIComponent(pool.id)}`}>{t("nodePools.viewDevices")}<ArrowUpRight aria-hidden="true" /></Link></Button>
          {canManage && <>
            <Button variant="ghost" size="sm" onClick={() => setAction({ kind: "rename", pool })}><Pencil aria-hidden="true" />{t("nodePools.rename")}</Button>
            <Button variant="ghost" size="sm" disabled={pool.is_default || (pool.node_count ?? 0) > 0}
              title={pool.is_default ? t("nodePools.defaultProtected") : (pool.node_count ?? 0) > 0 ? t("nodePools.moveDevicesFirst") : undefined}
              onClick={() => setAction({ kind: "delete", pool })}><Trash2 aria-hidden="true" />{t("nodePools.delete")}</Button>
          </>}
        </div>
      </article>)}
    </div>}
    {canManage && action && <PoolDialog key={action.kind === "create" ? "create" : `${action.kind}/${action.pool.id}`}
      action={action} close={() => setAction(undefined)} reload={pools.reload} />}
  </>;
}

function PoolDialog({ action, close, reload }: { action: PoolAction; close: () => void; reload: () => Promise<void> }) {
  const { t } = useUI();
  const [busy, setBusy] = useState(false);
  const deleting = action.kind === "delete";
  return <Modal open onOpenChange={open => { if (!open) close(); }} dismissible={!busy}
    title={t(deleting ? "nodePools.deleteTitle" : action.kind === "create" ? "nodePools.create" : "nodePools.rename")}
    description={deleting ? t("nodePools.deleteHint", { name: action.pool.name }) : t("nodePools.nameHint")}>
    <SubmitForm label={t(deleting ? "nodePools.deleteTitle" : action.kind === "create" ? "nodePools.create" : "common.save")}
      onCancel={busy ? undefined : close} successMessage={t(deleting ? "nodePools.deleted" : action.kind === "create" ? "nodePools.created" : "nodePools.renamed")}
      onSubmit={async (form, markSaved) => {
        const name = String(form.get("name") || "").trim();
        if (!deleting && (!name || new TextEncoder().encode(name).length > 100 || /[\u0000-\u001f\u007f-\u009f]/.test(name)))
          throw new LocalizedError("nodePools.invalidName");
        setBusy(true);
        try {
          await mutation(action.kind === "create" ? "/pools" : `/pools/${action.pool.id}`, deleting ? {} : { name },
            deleting ? "DELETE" : action.kind === "create" ? "POST" : "PATCH");
          markSaved();
          close();
          await reload();
        } finally { setBusy(false); }
      }}>
      {!deleting && <Field label={t("nodePools.name")}><Input name="name" required maxLength={100} disabled={busy}
        defaultValue={action.kind === "rename" ? action.pool.name : ""} /></Field>}
    </SubmitForm>
  </Modal>;
}
