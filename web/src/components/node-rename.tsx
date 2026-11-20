/**
 * [INPUT]: 依赖节点快照、名称 API、共享弹窗和全局通知。
 * [OUTPUT]: 提供 NodeRename 表单，按服务端 Unicode 空白与字节规则校验。
 * [POS]: 仅提交节点名称；编辑草稿与轮询快照隔离。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { Pencil } from "lucide-react";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError } from "@/lib/errors";
import { nodeHostname } from "@/lib/node-identity";
import { notify } from "@/lib/notify";
import type { Node } from "@/lib/types";

export function NodeRename({ node, reload }: { node: Node; reload: () => Promise<unknown> }) {
  const { t } = useUI();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [original, setOriginal] = useState("");
  const [busy, setBusy] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const form = useRef<HTMLFormElement>(null);
  const restoreInput = useRef(false);
  const id = useId();
  const hostname = nodeHostname(node);
  useEffect(() => {
    if (!busy && restoreInput.current) {
      restoreInput.current = false;
      input.current?.focus();
    }
  }, [busy]);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) return;
    const name = draft.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
    const reason = !name ? "nodes.nameRequired"
      : /\p{Cc}/u.test(name) ? "nodes.nameControl"
      : new TextEncoder().encode(name).length > 100 ? "nodes.nameBytes" : undefined;
    setInvalid(!!reason);
    if (reason) {
      notify.error(t(reason));
      input.current?.focus();
      return;
    }
    setBusy(true);
    try {
      await mutation(`/nodes/${node.id}/name`, { name }, "PATCH");
      if (form.current) form.current.dataset.dirty = "false";
      setOpen(false);
      notify.success(t("nodes.nameUpdated"));
      await reload();
    } catch (error) {
      notify.error(asError(error));
      restoreInput.current = true;
    } finally {
      setBusy(false);
    }
  }

  return <>
    <Button type="button" size="sm" variant="ghost" onClick={() => {
      setDraft(node.name);
      setOriginal(node.name);
      setInvalid(false);
      setOpen(true);
    }}><Pencil size={14} aria-hidden="true" />{t("nodes.rename")}</Button>
    <Modal open={open} onOpenChange={(value) => { if (!busy) setOpen(value); }}
      title={t("nodes.rename")} description={t("nodes.renameHint")} initialFocus={input}>
      <form ref={form} onSubmit={save} className="form-stack" data-dirty={draft !== original}>
        <div className="grid gap-2">
          <label htmlFor={id} className="text-sm font-medium">{t("nodes.name")}</label>
          <Input id={id} ref={input} value={draft} disabled={busy} aria-invalid={invalid}
            aria-describedby={`${id}-hint`} autoComplete="off" onChange={event => {
              setDraft(event.target.value);
              setInvalid(false);
            }} />
          <p id={`${id}-hint`} className="text-xs leading-5 text-muted-foreground">{t("nodes.nameHint")}</p>
        </div>
        <div className="grid justify-items-start gap-2 min-w-0">
          <Button type="button" variant="secondary" disabled={!hostname || busy}
            aria-describedby={!hostname ? `${id}-hostname` : undefined}
            onClick={() => { setDraft(hostname!); setInvalid(false); input.current?.focus(); }}>
            {t("nodes.useHostname")}
          </Button>
          <p id={`${id}-hostname`} className="max-w-full break-all text-xs leading-5 text-muted-foreground">
            {hostname || t("nodes.noHostname")}
          </p>
        </div>
        <div className="form-footer">
          <Button type="button" variant="secondary" disabled={busy} onClick={() => {
            if (draft !== original && !window.confirm(t("workspace.unsaved"))) return;
            setOpen(false);
          }}>{t("common.cancel")}</Button>
          <Button type="submit" disabled={busy}>{t(busy ? "common.processing" : "common.save")}</Button>
        </div>
      </form>
    </Modal>
  </>;
}
