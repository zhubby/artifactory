/**
 * [INPUT]: 依赖标签激活状态、受权会话、增量消息与身份关联投影。
 * [OUTPUT]: 提供轮次概览、已加载动作筛选和证据阅读；切离暂停轮询并保留位置。
 * [POS]: 构建详情唯一的只读审计标签；不介入自主规划。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { ArrowDown, ArrowUp, Check, ChevronRight, CircleHelp, Clock3, FileText, Info, Layers3, Search, Sparkles, Wrench, X } from "lucide-react";
import { api, useRemote } from "@/lib/api";
import { asError, sameError } from "@/lib/errors";
import { auditEntries, auditSubject, type AuditEntry } from "@/lib/planning-audit";
import { useUI } from "@/components/providers";
import { ErrorBox, Loading, Status } from "@/components/common";
import { AuditInspector, auditTitle } from "@/components/planning-audit-content";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import type { PlanningConversation, ConversationMessage, ConversationPage } from "@/lib/types";

const identity = (session: PlanningConversation) => `${session.run_id}/${session.generation}`;

export function PlanningConversations({ buildId, terminal, active }: { buildId: string; terminal: boolean; active: boolean }) {
  return <AuditConversations key={buildId} buildId={buildId} terminal={terminal} active={active} />;
}

function AuditConversations({ buildId, terminal, active }: { buildId: string; terminal: boolean; active: boolean }) {
  const { t, date } = useUI();
  const [activated, setActivated] = useState(active);
  const [choice, setChoice] = useState<string>();
  const sessions = useRemote<{ conversations: PlanningConversation[] }>(active || activated ? `/builds/${buildId}/planning/conversations` : null, active && !terminal ? 3000 : false);
  const previous = useRef({ active, terminal });
  const finalRead = useRef(false);
  useEffect(() => {
    if (active) setActivated(true);
  }, [active]);
  useEffect(() => {
    if (activated && terminal && !previous.current.terminal) finalRead.current = true;
    if (active && activated && (finalRead.current || !previous.current.active)) {
      finalRead.current = false;
      void sessions.reload({ progress: "background" });
    }
    previous.current = { active, terminal };
  }, [terminal, active, activated, sessions.reload]);
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
  return <section className="planning-audit-shell" aria-label={t("audit.tab")}>
    <div className="planning-audit">
      <ErrorBox message={sessions.error} />
      {sessions.error && <Button variant="secondary" size="sm" onClick={() => void sessions.reload()}>{t("common.retry")}</Button>}
      {!sessions.data && !sessions.error && active && <Loading variant="detail" />}
      {sessions.data && <>
        <div className="audit-intro"><div className="audit-intro-icon" aria-hidden="true"><Sparkles size={21} /></div><div><h2>{t("audit.title")}</h2><p>{t("audit.hint")}</p></div><span>{t("audit.sessionCount", { count: list.length })}</span></div>
        {!list.length && <div className="audit-empty"><CircleHelp size={28} aria-hidden="true" /><p>{t("ai.noConversations")}</p><span>{t("audit.emptyDetail")}</span></div>}
        {!!list.length && <nav className="audit-rounds" aria-label={t("audit.roundNavigation")}>
          {list.map((session, index) => <button key={identity(session)} type="button" aria-pressed={selected === identity(session)} onClick={() => setChoice(identity(session))}>
            <span className="audit-round-index" aria-hidden="true">{String(index + 1).padStart(2, "0")}</span><span className="audit-round-copy"><span>{label(session, index)}</span><time dateTime={session.started_at}>{date(session.started_at)}</time></span><Status value={session.status} small />
          </button>)}
        </nav>}
        {list.map((session, index) => <div key={`${buildId}/${identity(session)}`} hidden={selected !== identity(session)}>
          <Conversation buildId={buildId} session={session} enabled={active && selected === identity(session)} title={label(session, index)} />
        </div>)}
      </>}
    </div>
  </section>;
}

function EntryIcon({ entry }: { entry: AuditEntry }) {
  if (entry.outcome === "failed" || entry.outcome === "rejected") return <X size={14} />;
  if (entry.outcome) return <Check size={14} />;
  return entry.kind === "tool" || entry.kind === "proposal" ? <Wrench size={14} /> : entry.kind === "system" || entry.kind === "runtime" ? <Info size={14} /> : <FileText size={14} />;
}

function Conversation({ buildId, session, enabled, title }: { buildId: string; session: PlanningConversation; enabled: boolean; title: string }) {
  const { t, date } = useUI();
  const [messages, setMessages] = useState<ConversationMessage[]>([]);
  const [next, setNext] = useState<number | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  const [choice, setChoice] = useState<string>();
  const [tab, setTab] = useState("result");
  const [hintsOpen, setHintsOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const feed = useRef<HTMLDivElement>(null);
  const inspector = useRef<HTMLDivElement>(null);
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
  const attention = (entry: AuditEntry) => entry.outcome === "failed" || entry.outcome === "rejected" || entry.kind === "partial" || ((entry.kind === "tool" || entry.kind === "proposal") && !entry.outcome && session.status !== "running");
  const needle = query.trim().toLocaleLowerCase();
  const matches = (entry: AuditEntry) => (!needle || `${auditTitle(entry, t)} ${auditSubject(entry)} ${entry.name || ""} ${entry.callId || ""}`.toLocaleLowerCase().includes(needle)) && (filter === "all" || filter === "attention" && attention(entry) || filter === "tools" && (entry.kind === "tool" || entry.kind === "proposal") || filter === "messages" && ["context", "assistant", "user", "unknown", "partial"].includes(entry.kind));
  const visible = normal.filter(matches);
  const visibleHints = hints.filter(matches);
  const attentionCount = normal.filter(attention).length;
  const toolsCount = normal.filter(entry => entry.kind === "tool" || entry.kind === "proposal").length;
  const reveal = (target: RefObject<HTMLDivElement | null>) => {
    target.current?.scrollIntoView({ block: "start", behavior: "instant" });
    target.current?.focus({ preventScroll: true });
  };
  const selected = entries.find(entry => entry.id === choice) || normal.find(entry => entry.outcome === "failed" || entry.outcome === "rejected") || normal[0] || entries[0];
  useEffect(() => { if (!choice && selected) setChoice(selected.id); }, [choice, selected]);
  const choose = (entry: AuditEntry) => { setChoice(entry.id); setTab("result"); };
  const row = (entry: AuditEntry) => <li key={entry.id}><button className={`audit-action outcome-${entry.outcome || "missing"}`} type="button" aria-pressed={selected?.id === entry.id} onClick={() => choose(entry)}>
    <span className="audit-action-icon" aria-hidden="true"><EntryIcon entry={entry} /></span><span className="audit-action-copy"><strong>{auditTitle(entry, t)}</strong><span>{auditSubject(entry) || entry.name || t(`audit.kinds.${entry.kind}`)}</span></span>
    <span className="audit-action-state">{entry.outcome ? t(`audit.outcomes.${entry.outcome}`) : entry.kind === "partial" ? t("audit.kinds.partial") : (entry.kind === "tool" || entry.kind === "proposal") && session.status !== "running" ? t("audit.missingResult") : ""}</span><ChevronRight size={12} aria-hidden="true" />
  </button></li>;
  return <section className="audit-session" aria-label={title}>
    <header className="audit-session-header"><div><span className="audit-session-eyebrow">{t("audit.roundNavigation")}</span><h3>{title}</h3><div className="audit-model"><Sparkles size={14} aria-hidden="true" /><span>{session.model || "—"}</span></div></div><div className="audit-session-time"><span><Clock3 size={14} aria-hidden="true" /><time dateTime={session.started_at}>{date(session.started_at)}</time></span>{session.ended_at && <span>{t("audit.endedAt")} <time dateTime={session.ended_at}>{date(session.ended_at)}</time></span>}</div></header>
    {session.status === "interrupted" && <p className="audit-interrupted" role="status">{t("audit.interrupted")}</p>}
    <ErrorBox message={error} />
    {enabled && !loaded && !error && <Loading variant="list" />}
    {loaded && !entries.length && <p className="audit-empty">{t("audit.noRecords")}</p>}
    {!!entries.length && <>
      <div className="audit-summary" aria-label={t("audit.loadedSummary")}><div><Layers3 size={17} aria-hidden="true" /><span>{t("audit.recordCount", { count: messages.length })}</span></div><div><Wrench size={17} aria-hidden="true" /><span>{t("audit.loadedTools", { count: toolsCount })}</span></div><div data-attention={attentionCount > 0 || undefined}><CircleHelp size={17} aria-hidden="true" /><span>{t("audit.loadedAttention", { count: attentionCount })}</span></div></div>
      <div className="audit-toolbar"><label className="audit-search"><Search size={17} aria-hidden="true" /><input type="search" value={query} onChange={event => setQuery(event.target.value)} aria-label={t("audit.search")} placeholder={t("audit.search")} /></label><Select aria-label={t("audit.filter")} value={filter} onChange={event => setFilter(event.target.value)}><option value="all">{t("audit.filters.all")}</option><option value="tools">{t("audit.filters.tools")}</option><option value="messages">{t("audit.filters.messages")}</option><option value="attention">{t("audit.filters.attention")}</option></Select></div>
      <p className="audit-filter-scope">{t("audit.loadedScope")}</p>
      <div className="audit-mobile-jump"><Button variant="secondary" size="sm" onClick={() => reveal(inspector)}><ArrowDown />{t("audit.viewSelected")}</Button></div>
      <div className="audit-workspace">
      <div className="audit-feed" ref={feed} tabIndex={-1} aria-label={t("audit.actions")}><div className="audit-feed-heading"><h4 className="audit-feed-title">{t("audit.actions")}</h4><span>{t("audit.visibleCount", { count: visible.length })}</span></div><ol className="audit-action-list">{visible.map(row)}</ol>
        {!visible.length && <p className="audit-filter-empty" role="status">{t("audit.noFilteredActions")}</p>}
        {!!hints.length && <details className="audit-hints" open={hintsOpen} onToggle={event => setHintsOpen(event.currentTarget.open)}><summary>{t("audit.platformHints", { count: visibleHints.length })}</summary><ol className="audit-action-list">{visibleHints.map(row)}</ol></details>}
        {next !== null && <Button variant="secondary" size="sm" disabled={busy} onClick={() => void load(next)}>{t("ai.moreMessages")}</Button>}
      </div>
      <div className="audit-inspector-anchor" ref={inspector} tabIndex={-1} aria-label={t("audit.detail")}><div className="audit-mobile-jump"><Button variant="ghost" size="sm" onClick={() => reveal(feed)}><ArrowUp />{t("audit.backToActions")}</Button></div>{selected && <AuditInspector key={selected.id} entry={selected} tab={tab} setTab={setTab} terminal={session.status !== "running"} />}</div>
    </div></>}
    {error && <Button variant="secondary" size="sm" disabled={busy} onClick={() => void load(latest.current)}>{t("common.retry")}</Button>}
    {loaded && <p className="audit-retention-note">{t("audit.retainedOnly")}</p>}
    <details className="audit-session-technical"><summary>{t("audit.sessionTechnical")}</summary><dl className="audit-fields"><div><dt>Run ID</dt><dd><code>{session.run_id}</code></dd></div><div><dt>Pod UID</dt><dd><code>{session.pod_uid}</code></dd></div>{session.source_digest && <div><dt>{t("audit.sourceDigest")}</dt><dd><code>{session.source_digest}</code></dd></div>}</dl></details>
  </section>;
}
