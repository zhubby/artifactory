/**
 * [INPUT]: 依赖当前租户权限、设备终端 API、会话传输和终端渲染器。
 * [OUTPUT]: 提供显式创建、结束、原页面重连与粘贴确认的设备终端工作区。
 * [POS]: 独立于设备列表和详情挂载；离开页面即结束会话。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, Copy, LogOut, Square, Terminal } from "lucide-react";
import { ErrorBox, Field, Loading, PageTitle } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Modal } from "@/components/ui/dialog";
import { TerminalView, type TerminalViewHandle } from "@/components/terminal-view";
import { api, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { createIdempotencyKey } from "@/lib/idempotency";
import { notify } from "@/lib/notify";
import { TerminalConnection, TerminalConnectionError } from "@/lib/terminal";
import { Link } from "@/lib/tenant";
import type { Node, User } from "@/lib/types";
import type { TerminalCapabilities, TerminalSession, TerminalStatus } from "@/lib/terminal-types";

const MAX_PASTE_BYTES = 64 * 1024;

export function TerminalWorkspace({ nodeId, user, readOnly }: { nodeId: string; user: User; readOnly: boolean }) {
  const { t } = useUI();
  const permitted = user.role === "admin" && !readOnly;
  const nodes = useRemote<Node[]>(permitted ? `/nodes?limit=100&offset=0&q=${encodeURIComponent(nodeId)}` : null, false);
  const capabilities = useRemote<TerminalCapabilities>(permitted ? `/nodes/${encodeURIComponent(nodeId)}/terminal-capabilities` : null, 5000);
  const node = nodes.data?.find(value => value.id === nodeId);
  const view = useRef<TerminalViewHandle>(null);
  const release = useRef<HTMLButtonElement>(null);
  const connection = useRef<TerminalConnection | undefined>(undefined);
  const mounted = useRef(true);
  const creating = useRef<AbortController | undefined>(undefined);
  const renderer = useRef("");
  const idempotency = useRef("");
  const [ready, setReady] = useState(false);
  const [profile, setProfile] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<TerminalStatus>({ phase: "idle" });
  const [error, setError] = useState<Error>();
  const [paste, setPaste] = useState<string>();
  const active = ["connecting", "running", "reconnecting"].includes(status.phase);
  const enabled = permitted && capabilities.data?.enabled && capabilities.data.online && !node?.revoked && !node?.draining && !node?.ota_draining;
  const selectedProfile = capabilities.data?.profiles.find(value => value.id === profile)?.id || capabilities.data?.profiles[0]?.id || "";

  function report(cause: Error) {
    if (!mounted.current) return;
    if (cause instanceof TerminalConnectionError) {
      notify.error(t(`terminal.error_${cause.code}`));
      setStatus({ phase: "error", reason: t(`terminal.error_${cause.code}`) });
    } else setError(cause);
  }

  useEffect(() => {
    mounted.current = true;
    const logout = () => { view.current?.clear(); setPaste(undefined); setStatus({ phase: "closed" }); };
    window.addEventListener("artifactory:session-ended", logout);
    return () => {
      mounted.current = false;
      creating.current?.abort();
      connection.current?.close();
      view.current?.clear();
      window.removeEventListener("artifactory:session-ended", logout);
    };
  }, [nodeId]);

  useEffect(() => {
    if (!permitted || (capabilities.data && (!capabilities.data.enabled || !capabilities.data.online))) {
      creating.current?.abort();
      connection.current?.close();
      view.current?.clear();
      setPaste(undefined);
      if (!permitted) setReady(false);
    }
  }, [permitted, capabilities.data]);

  const onReady = useCallback(() => setReady(true), []);
  const input = useCallback((bytes: Uint8Array) => connection.current?.input(bytes), []);
  const resize = useCallback((columns: number, rows: number) => connection.current?.resize(columns, rows), []);

  async function open() {
    if (!enabled || !ready || active || busy || !selectedProfile) return;
    setError(undefined);
    setBusy(true);
    if (!idempotency.current) {
      renderer.current = createIdempotencyKey();
      idempotency.current = createIdempotencyKey();
    }
    const controller = new AbortController();
    creating.current = controller;
    try {
      const size = view.current!.size();
      const session = await api<TerminalSession>(`/nodes/${encodeURIComponent(nodeId)}/terminal-sessions`, {
        method: "POST", signal: controller.signal,
        body: JSON.stringify({ idempotency_key: idempotency.current, renderer_id: renderer.current, profile: selectedProfile, ...size, term: "xterm-256color" }),
      });
      if (!mounted.current || controller.signal.aborted) return;
      view.current?.clear();
      setStatus({ phase: "connecting" });
      const client = new TerminalConnection(session.id, renderer.current, {
        status: next => {
          if (!mounted.current) return;
          setStatus(previous => ({ ...previous, ...next }));
          if (next.phase === "running") {
            const currentSize = view.current?.size();
            if (currentSize) connection.current?.resize(currentSize.columns, currentSize.rows);
          }
        },
        output: bytes => view.current?.write(bytes) || Promise.resolve(),
        clear: () => view.current?.clear(),
        failure: report,
      });
      connection.current?.close();
      connection.current = client;
      idempotency.current = "";
      client.start();
      view.current?.focus();
    } catch (cause) {
      if (mounted.current && !controller.signal.aborted) report(asError(cause));
    } finally { if (mounted.current) setBusy(false); }
  }

  function receivePaste(value: string) {
    if (status.phase !== "running") return;
    if (new TextEncoder().encode(value).length > MAX_PASTE_BYTES) {
      notify.error(t("terminal.pasteTooLarge"));
      return;
    }
    if (/[\r\n]/.test(value)) setPaste(value);
    else view.current?.paste(value);
  }

  async function copy() {
    const selection = view.current?.selection();
    if (!selection) { notify.info(t("terminal.selectToCopy")); return; }
    try { await navigator.clipboard.writeText(selection); notify.success(t("terminal.copied")); }
    catch { notify.error(t("terminal.copyFailed")); }
  }

  const unavailable = !permitted ? t("terminal.permissionRequired")
    : !capabilities.data ? undefined
    : !capabilities.data.online ? t("terminal.offline")
    : !capabilities.data.enabled ? t("terminal.disabled")
    : !capabilities.data.profiles.length ? t("terminal.noProfiles")
    : node?.revoked || node?.draining || node?.ota_draining ? t("terminal.unavailable") : undefined;

  return <section className="terminal-workspace" data-testid="terminal-workspace">
    <PageTitle eyebrow={t("workspace.sharedDevices")} title={t("terminal.title")} description={node ? `${node.name} · ${node.os}/${node.arch}` : nodeId}>
      <Button variant="ghost" asChild><Link href="/nodes"><ArrowLeft size={15} />{t("terminal.back")}</Link></Button>
    </PageTitle>
    <ErrorBox message={error || capabilities.error || nodes.error} />
    {!permitted ? <p className="terminal-unavailable" role="status">{unavailable}</p> : !capabilities.data ? (
      capabilities.error ? <Button variant="secondary" onClick={() => void capabilities.reload()}>{t("common.retry")}</Button> : <Loading variant="detail" />
    ) : <>
      <div className="terminal-controls">
        <Field label={t("terminal.profile")}>
          <Select aria-label={t("terminal.profile")} value={selectedProfile} disabled={active || busy || !enabled}
            onChange={event => { setProfile(event.target.value); idempotency.current = ""; }}>
            {capabilities.data.profiles.map(value => <option value={value.id} key={value.id}>{value.label}</option>)}
          </Select>
        </Field>
        <Button disabled={!enabled || !ready || active || busy || !selectedProfile} onClick={() => void open()}>
          <Terminal size={15} />{t(busy ? "terminal.creating" : "terminal.open")}
        </Button>
        <Button variant="secondary" disabled={!active} onClick={() => { connection.current?.close(); setPaste(undefined); }}>
          <Square size={13} />{t("terminal.end")}
        </Button>
      </div>
      {unavailable && <p className="terminal-unavailable" role="status">{unavailable}</p>}
      <div className="terminal-surface">
        <div className="terminal-toolbar">
          <div className="terminal-target">
            <span className={`terminal-state terminal-state-${status.phase}`} role="status" data-testid="terminal-state">{t(`terminal.state_${status.phase}`)}</span>
            <span title={status.account || capabilities.data.account}>{status.account || capabilities.data.account || t("nodes.unknown")}</span>
            {status.cwd && <span title={status.cwd}>{status.cwd}</span>}
            {status.shell && <span title={status.shell}>{status.shell}</span>}
          </div>
          <div className="terminal-toolbar-actions">
            <Button variant="ghost" size="sm" onClick={() => void copy()}><Copy size={14} />{t("terminal.copy")}</Button>
            <Button ref={release} variant="ghost" size="sm" onClick={() => release.current?.focus()} aria-label={t("terminal.leaveFocus")}>
              <LogOut size={14} />{t("terminal.leaveFocus")}
            </Button>
          </div>
        </div>
        <TerminalView ref={view} active={status.phase === "running"} label={t("terminal.input")}
          onReady={onReady} onInput={input} onResize={resize} onPaste={receivePaste}
          onReleaseFocus={() => {
            release.current?.focus({ preventScroll: true });
            release.current?.scrollIntoView({ block: "center" });
          }} onError={report} />
        <div className="terminal-footer">
          <span>{status.reason || t("terminal.focusHint")}</span>
          {status.code != null && <span>{t("terminal.exitCode", { code: status.code })}</span>}
          {status.signal && <span>{t("terminal.signal", { signal: status.signal })}</span>}
          <Button variant="ghost" size="sm" onClick={() => view.current?.focus()}>{t("terminal.focus")}</Button>
        </div>
      </div>
      <p className="terminal-account-hint">{t("terminal.accountHint")}</p>
      <p className="terminal-account-hint">{t("terminal.refreshHint")}</p>
    </>}
    <Modal open={paste !== undefined} onOpenChange={value => { if (!value) setPaste(undefined); }}
      title={t("terminal.pasteTitle")} description={t("terminal.pasteDescription")}>
      <pre className="terminal-paste-preview">{paste?.slice(0, 2000)}{paste && paste.length > 2000 ? "…" : ""}</pre>
      <div className="form-footer">
        <Button variant="secondary" onClick={() => setPaste(undefined)}>{t("common.cancel")}</Button>
        <Button disabled={status.phase !== "running"} onClick={() => { if (paste !== undefined) view.current?.paste(paste); setPaste(undefined); }}>{t("terminal.confirmPaste")}</Button>
      </div>
    </Modal>
  </section>;
}
