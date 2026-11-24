/**
 * [INPUT]: 依赖受权会话、增量审计消息与身份关联阅读投影。
 * [OUTPUT]: 展示规划轮次、动作和可读详情，保留分页、选择与阅读位置。
 * [POS]: 构建计划与过程共用的只读审计入口；不介入自主规划。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronRight, FileText, Info, Wrench, X } from "lucide-react";
import { api, useRemote } from "@/lib/api";
import { asError, sameError } from "@/lib/errors";
import { auditEntries, auditSubject, type AuditEntry } from "@/lib/planning-audit";
import { useUI } from "@/components/providers";
import { ErrorBox, Loading, Status } from "@/components/common";
import { AuditInspector, auditTitle } from "@/components/planning-audit-content";
import { Button } from "@/components/ui/button";
import type { PlanningConversation, ConversationMessage, ConversationPage } from "@/lib/types";

const identity = (session: PlanningConversation) => `${session.run_id}/${session.generation}`;

export function PlanningConversations({ buildId, terminal }: { buildId: string; terminal: boolean }) {
  return <AuditConversations key={buildId} buildId={buildId} terminal={terminal} />;
}

function AuditConversations({ buildId, terminal }: { buildId: string; terminal: boolean }) {
  const { t, date } = useUI();
  const [open, setOpen] = useState(false);
  const [activated, setActivated] = useState(false);
  const [choice, setChoice] = useState<string>();
  const sessions = useRemote<{ conversations: PlanningConversation[] }>(activated ? `/builds/${buildId}/planning/conversations` : null, open && !terminal ? 3000 : false);
  const wasTerminal = useRef(terminal);
  const finalRead = useRef(false);
  useEffect(() => {
    if (terminal && !wasTerminal.current) finalRead.current = true;
    if (finalRead.current && open) { finalRead.current = false; void sessions.reload({ progress: "background" }); }
    wasTerminal.current = terminal;
  }, [terminal, open, sessions.reload]);
  const list = sessions.data?.conversations || [];
  const fallback = list.findLast(session => session.status === "running") || list[0];
  const selected = choice && list.some(session => identity(session) === choice) ? choice : fallback ? identity(fallback) : undefined;
  useEffect(() => { if (!choice && fallback) setChoice(identity(fallback)); }, [choice, fallback]);
  const revisions = [...new Set(list.filter(session => session.purpose === "revision").map(session => session.run_id))];
  const label = (session: PlanningConversation, index: number) => {
    const purpose = session.purpose;
    const title = purpose === "revision" ? t("audit.roundRevision", { count: revisions.indexOf(session.run_id) + 1 })
      : ["initial", "clarification", "placement"].includes(purpose || "") ? t(`audit.rounds.${purpose}`) : t("audit.roundUnknown", { count: index + 1 });
    return session.generation > 1 ? `${title} · ${t("ai.generation", { generation: session.generation })}` : title;
  };
  return <details className="planning-audit-shell" onToggle={event => { const value = event.currentTarget.open; setOpen(value); if (value) setActivated(true); }}>
    <summary>{t("ai.conversations")}</summary>
    <div className="planning-audit" hidden={!open}>
      <ErrorBox message={sessions.error} />
      {sessions.error && <Button variant="secondary" size="sm" onClick={() => void sessions.reload()}>{t("common.retry")}</Button>}
      {!sessions.data && !sessions.error && open && <Loading variant="detail" />}
      {sessions.data && <>
        <div className="audit-intro"><p>{t("audit.hint")}</p><span>{t("audit.sessionCount", { count: list.length })}</span></div>
        {!list.length && <p className="audit-empty">{t("ai.noConversations")}</p>}
        {!!list.length && <nav className="audit-rounds" aria-label={t("audit.roundNavigation")}>
          {list.map((session, index) => <button key={identity(session)} type="button" aria-pressed={selected === identity(session)} onClick={() => setChoice(identity(session))}>
            <span>{label(session, index)}</span><Status value={session.status} small /><time dateTime={session.started_at}>{date(session.started_at)}</time>
          </button>)}
        </nav>}
        {list.map((session, index) => <div key={`${buildId}/${identity(session)}`} hidden={selected !== identity(session)}>
          <Conversation buildId={buildId} session={session} enabled={open && selected === identity(session)} title={label(session, index)} />
        </div>)}
      </>}
    </div>
  </details>;
}

function EntryIcon({ entry }: { entry: AuditEntry }) {
  if (entry.outcome === "failed" || entry.outcome === "rejected") return <X size={14} />;
  if (entry.outcome) return <Check size={14} />;
  return entry.kind === "tool" || entry.kind === "proposal" ? <Wrench size={14} /> : entry.kind === "system" || entry.kind === "runtime" ? <Info size={14} /> : <FileText size={14} />;
}

function Conversation({ buildId, session, enabled, title }: { buildId: string; session: PlanningConversation; enabled: boolean; title: string }) {
  const { t } = useUI();
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  const [choice, setChoice] = useState<string>();
  const [tab, setTab] = useState("result");
  const [hintsOpen, setHintsOpen] = useState(false);
  const latest = useRef(0);
  const pending = useRef<AbortController | null>(null);
  const priorStatus = useRef(session.status);
  const finalPending = useRef(false);
  const path = `/builds/${buildId}/planning/conversations/${session.run_id}/${session.generation}/messages`;
  const load = useCallback(async (after: number, background = false) => {
    if (pending.current) return;
    const controller = new AbortController(); pending.current = controller; setBusy(true);
    try {
      const page = await api<ConversationPage>(`${path}?after=${after}&limit=50`, { signal: controller.signal, progress: background ? "background" : "foreground" });
      if (controller.signal.aborted) return;
      setMessages(previous => {
        const merged = new Map(previous.map(message => [message.sequence, message]));
        for (const message of page.messages) merged.set(message.sequence, message);
        return [...merged.values()].sort((a, b) => a.sequence - b.sequence);
      });
      latest.current = Math.max(latest.current, ...page.messages.map(message => message.sequence));
      setNext(page.next_cursor); setLoaded(true); setError(undefined);
    } catch (error) {
      if (!controller.signal.aborted) setError(previous => { const failure = asError(error); return sameError(previous, failure) ? previous : failure; });
    } finally {
      if (pending.current === controller) pending.current = null;
      if (!controller.signal.aborted) setBusy(false);
    }
  }, [path]);
  useEffect(() => { if (enabled && !loaded && !error) void load(0); }, [enabled, loaded, error, load]);
  useEffect(() => {
    if (priorStatus.current === "running" && session.status !== "running") finalPending.current = true;
    priorStatus.current = session.status;
    if (enabled && loaded && !busy && finalPending.current) { finalPending.current = false; void load(latest.current, true); }
  }, [enabled, loaded, busy, session.status, load]);
  useEffect(() => {
    if (!enabled || !loaded || next !== null || session.status !== "running") return;
    const timer = setInterval(() => void load(latest.current, true), 3000);
    return () => clearInterval(timer);
  }, [enabled, loaded, next, session.status, load]);
  useEffect(() => () => { pending.current?.abort(); pending.current = null; }, []);
  const entries = useMemo(() => auditEntries(messages), [messages]);
  const normal = entries.filter(entry => entry.kind !== "runtime" && entry.kind !== "system");
  const hints = entries.filter(entry => entry.kind === "runtime" || entry.kind === "system");
  const selected = entries.find(entry => entry.id === choice) || normal.find(entry => entry.outcome === "failed" || entry.outcome === "rejected") || normal[0] || entries[0];
  useEffect(() => { if (!choice && selected) setChoice(selected.id); }, [choice, selected]);
  const choose = (entry: AuditEntry) => { setChoice(entry.id); setTab("result"); };
  const row = (entry: AuditEntry) => <li key={entry.id}><button className={`audit-action outcome-${entry.outcome || "missing"}`} type="button" aria-pressed={selected?.id === entry.id} onClick={() => choose(entry)}>
    <span className="audit-action-icon" aria-hidden="true"><EntryIcon entry={entry} /></span><span className="audit-action-copy"><strong>{auditTitle(entry, t)}</strong><span>{auditSubject(entry) || entry.name || t(`audit.kinds.${entry.kind}`)}</span></span>
    {entry.outcome && <span className="audit-action-state">{t(`audit.outcomes.${entry.outcome}`)}</span>}<ChevronRight size={12} aria-hidden="true" />
  </button></li>;
  return <section className="audit-session" aria-label={title}>
    <header className="audit-session-header"><div><h3>{title}</h3><span>{session.model}</span></div><span>{t("audit.recordCount", { count: messages.length })}</span></header>
    {session.status === "interrupted" && <p className="audit-interrupted" role="status">{t("audit.interrupted")}</p>}
    <ErrorBox message={error} />
    {enabled && !loaded && !error && <Loading variant="list" />}
    {loaded && !entries.length && <p className="audit-empty">{t("audit.noRecords")}</p>}
    {!!entries.length && <div className="audit-workspace">
      <div className="audit-feed"><h4 className="audit-feed-title">{t("audit.actions")}</h4><ol className="audit-action-list">{normal.map(row)}</ol>
        {!!hints.length && <details className="audit-hints" open={hintsOpen} onToggle={event => setHintsOpen(event.currentTarget.open)}><summary>{t("audit.platformHints", { count: hints.length })}</summary><ol className="audit-action-list">{hints.map(row)}</ol></details>}
        {next !== null && <Button variant="secondary" size="sm" disabled={busy} onClick={() => void load(next)}>{t("ai.moreMessages")}</Button>}
      </div>
      {selected && <AuditInspector key={selected.id} entry={selected} tab={tab} setTab={setTab} terminal={session.status !== "running"} />}
    </div>}
    {error && <Button variant="secondary" size="sm" disabled={busy} onClick={() => void load(latest.current)}>{t("common.retry")}</Button>}
    {loaded && <p className="audit-retention-note">{t("audit.retainedOnly")}</p>}
    <details className="audit-session-technical"><summary>{t("audit.sessionTechnical")}</summary><dl className="audit-fields"><div><dt>Run ID</dt><dd><code>{session.run_id}</code></dd></div><div><dt>Pod UID</dt><dd><code>{session.pod_uid}</code></dd></div>{session.source_digest && <div><dt>{t("audit.sourceDigest")}</dt><dd><code>{session.source_digest}</code></dd></div>}</dl></details>
  </section>;
}
