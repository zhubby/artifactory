/**
 * [INPUT]: 依赖审计阅读投影、已保存文本、剪贴板与统一通知。
 * [OUTPUT]: 安全呈现审计正文，提供类型层级与原始记录复制。
 * [POS]: 审计详情阅读层；不执行 HTML，不请求或改写源码。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { Fragment, useState, type ReactNode } from "react";
import { ClipboardList, Copy, FileText, GitPullRequest, Info, MessageSquare, Sparkles, Wrench } from "lucide-react";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Tabs } from "@/components/ui/tabs";
import { auditObject, type AuditEntry } from "@/lib/planning-audit";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { bytes } from "@/lib/utils";

function inline(text: string): ReactNode {
  return text.split(/(`[^`\n]+`|\*\*[^*\n]+\*\*)/g).map((part, index) => part.startsWith("`") && part.endsWith("`")
    ? <code key={index}>{part.slice(1, -1)}</code> : part.startsWith("**") && part.endsWith("**")
      ? <strong key={index}>{part.slice(2, -2)}</strong> : <Fragment key={index}>{part}</Fragment>);
}

export function AuditDocument({ text, code = false, offset = 0 }: { text: string; code?: boolean; offset?: number }) {
  const { t } = useUI();
  const [length, setLength] = useState(16000);
  const visible = text.slice(0, length);
  const lines = visible.split("\n");
  const nodes: ReactNode[] = [];
  if (code) nodes.push(<pre key="code" className="audit-source-code"><code>{lines.map((line, index) => <span className="audit-code-line" key={index}><span aria-hidden="true">{offset + index + 1}</span><span>{line || " "}</span></span>)}</code></pre>);
  else for (let index = 0; index < lines.length;) {
    const start = index, line = lines[index];
    if (!line.trim()) { index++; continue; }
    if (/^\s*```/.test(line)) {
      const content: string[] = []; index++;
      while (index < lines.length && !/^\s*```/.test(lines[index])) content.push(lines[index++]);
      index++;
      nodes.push(<pre key={start}><code>{content.join("\n")}</code></pre>);
    } else if (/^#{1,6}\s/.test(line)) {
      nodes.push(<h4 key={start}>{inline(line.replace(/^#{1,6}\s+/, ""))}</h4>); index++;
    } else if (/^\s*[-*]\s/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*[-*]\s/.test(lines[index])) items.push(lines[index++].replace(/^\s*[-*]\s+/, ""));
      nodes.push(<ul key={start}>{items.map((item, i) => <li key={i}>{inline(item)}</li>)}</ul>);
    } else {
      const paragraph: string[] = [line]; index++;
      while (index < lines.length && lines[index].trim() && !/^\s*(?:```|#{1,6}\s|[-*]\s)/.test(lines[index])) paragraph.push(lines[index++]);
      nodes.push(<p key={start}>{inline(paragraph.join("\n"))}</p>);
    }
  }
  return <div className="audit-document">{nodes}{text.length > length && <Button variant="ghost" size="sm" onClick={() => setLength(value => value + 16000)}>{t("audit.moreContent")}</Button>}</div>;
}

const fieldKeys = new Set(["goal", "ref", "commit", "path", "offset", "limit", "query", "target", "summary", "recipe", "targets", "steps", "name", "run", "os", "arch", "platforms", "dockerfile", "context", "pool", "error", "accepted", "plan_revision_id", "question", "reason", "evidence", "source_manifest", "target_requirements", "parameters"]);
function fieldName(key: string, t: ReturnType<typeof useUI>["t"]) { return fieldKeys.has(key) ? t(`audit.fields.${key}`) : key; }

export function AuditValue({ value, depth = 0 }: { value: unknown; depth?: number }) {
  const { t } = useUI();
  if (typeof value === "string") return <AuditDocument text={value} />;
  if (value === null || value === undefined) return <span className="audit-muted">{t("audit.emptyValue")}</span>;
  if (typeof value !== "object") return <span>{String(value)}</span>;
  if (depth >= 5) return <pre>{JSON.stringify(value, null, 2)}</pre>;
  if (Array.isArray(value)) return <div className="audit-values">{value.slice(0, 100).map((item, index) => <div key={index}><AuditValue value={item} depth={depth + 1} /></div>)}{value.length > 100 && <p className="audit-muted">{t("audit.largeValue")}</p>}{!value.length && <span className="audit-muted">{t("audit.emptyValue")}</span>}</div>;
  const object = auditObject(value)!;
  if (typeof object.path === "string" && typeof object.content === "string") {
    const markdown = /\.(?:md|markdown)$/i.test(object.path);
    return <div className="audit-file"><div className="audit-file-bar"><code>{object.path}</code>{typeof object.offset === "number" && <span>{t("audit.lineFrom", { line: object.offset + 1 })}</span>}</div><AuditDocument text={object.content} code={!markdown} offset={typeof object.offset === "number" ? object.offset : 0} />{object.truncated === true && <p className="audit-muted">{t("audit.sourceTruncated")}</p>}</div>;
  }
  if (Array.isArray(object.files)) return <><div className="audit-table-wrap"><table><caption>{t("audit.fileList")}</caption><thead><tr><th>{t("audit.fields.path")}</th><th>{t("audit.size")}</th></tr></thead><tbody>{object.files.slice(0, 200).map((file, index) => {
    const row = auditObject(file);
    return <tr key={index}><td><code>{String(row?.path ?? "")}</code></td><td>{typeof row?.size === "number" ? bytes(row.size) : "—"}</td></tr>;
  })}</tbody></table></div>{object.files.length > 200 && <p className="audit-muted">{t("audit.largeValue")}</p>}{object.next_offset != null && <p className="audit-muted">{t("audit.sourceTruncated")}</p>}</>;
  if (Array.isArray(object.matches)) return <><ul className="audit-search-results">{object.matches.slice(0, 200).map((match, index) => {
    const row = auditObject(match);
    return <li key={index}><code>{String(row?.path ?? "")}:{String(row?.line ?? "")}</code><pre>{String(row?.text ?? "")}</pre></li>;
  })}</ul>{!object.matches.length && <p>{t("audit.noMatches")}</p>}{object.matches.length > 200 && <p className="audit-muted">{t("audit.largeValue")}</p>}{object.truncated === true && <p className="audit-muted">{t("audit.searchTruncated")}</p>}</>;
  if (Array.isArray(object.logs)) return <div className="audit-values">{object.logs.slice(0, 100).map((log, index) => {
    const row = auditObject(log);
    return <div key={index}>{typeof row?.content === "string" ? <><div className="audit-file-bar"><code>{String(row.step ?? "")}</code></div><AuditDocument text={row.content} code /></> : <AuditValue value={log} depth={depth + 1} />}</div>;
  })}{object.logs.length > 100 && <p className="audit-muted">{t("audit.largeValue")}</p>}</div>;
  const fields = Object.entries(object);
  return <dl className="audit-fields">{fields.slice(0, 100).map(([key, item]) => <div key={key}><dt>{fieldName(key, t)}</dt><dd><AuditValue value={item} depth={depth + 1} /></dd></div>)}{fields.length > 100 && <div><dd>{t("audit.largeValue")}</dd></div>}</dl>;
}

function AuditContext({ value }: { value: unknown }) {
  const { t } = useUI();
  const context = auditObject(value);
  if (!context) return <AuditValue value={value} />;
  const primaryKeys = ["goal", "ref", "commit", "target_requirements"];
  const primary = Object.fromEntries(primaryKeys.filter(key => context[key] !== undefined).map(key => [key, context[key]]));
  const extra = Object.fromEntries(Object.entries(context).filter(([key]) => !primaryKeys.includes(key)));
  return <><div className="audit-context-primary"><AuditValue value={primary} /></div>{Object.keys(extra).length > 0 && <details className="audit-context-extra"><summary>{t("audit.moreContext")}</summary><AuditValue value={extra} /></details>}</>;
}

export function auditTitle(entry: AuditEntry, t: ReturnType<typeof useUI>["t"]) {
  const tools = new Set(["read_file", "list_tree", "search_source", "git_metadata", "read_logs", "node_capabilities", "query_nodes", "get_node", "submit_plan", "submit_placements", "request_input"]);
  return entry.name ? tools.has(entry.name) ? t(`audit.tools.${entry.name}`) : entry.name : t(`audit.kinds.${entry.kind}`);
}

export function AuditInspector({ entry, tab, setTab, terminal }: { entry: AuditEntry; tab: string; setTab: (value: string) => void; terminal: boolean }) {
  const { t, date } = useUI();
  const [copying, setCopying] = useState(false);
  const Icon = entry.kind === "tool" ? Wrench : entry.kind === "proposal" ? GitPullRequest
    : entry.kind === "context" ? ClipboardList : entry.kind === "assistant" ? Sparkles
      : entry.kind === "user" ? MessageSquare : entry.kind === "system" || entry.kind === "runtime" ? Info : FileText;
  const copy = async () => {
    setCopying(true);
    try {
      await navigator.clipboard.writeText(JSON.stringify(entry.records, null, 2));
      notify.success(t("common.copied"));
    } catch (error) {
      notify.error(asError(error));
    } finally {
      setCopying(false);
    }
  };
  return <section className="audit-inspector" role="region" aria-label={t("audit.detail")}>
    <header>
      <div className="audit-inspector-heading">
        <span className={`audit-inspector-icon kind-${entry.kind}`} aria-hidden="true"><Icon size={20} /></span>
        <div className="audit-inspector-title"><p className="audit-inspector-eyebrow">{t(`audit.kinds.${entry.kind}`)}</p><h3>{auditTitle(entry, t)}</h3>{entry.name && <code className="audit-tool-name">{entry.name}</code>}</div>
      </div>
      <div className="audit-inspector-actions">
        <span className={`audit-outcome outcome-${entry.outcome || "missing"}`}>{t(entry.outcome ? `audit.outcomes.${entry.outcome}` : entry.kind === "tool" || entry.kind === "proposal" ? terminal ? "audit.missingResult" : "audit.waitingResult" : `audit.kinds.${entry.kind}`)}</span>
        <Button type="button" variant="ghost" size="sm" disabled={copying} aria-busy={copying} title={`${t("common.copy")} · ${t("audit.raw")}`} onClick={() => void copy()}><Copy aria-hidden="true" />{t("common.copy")}</Button>
      </div>
    </header>
    <div className="audit-detail-meta"><time dateTime={entry.records[0].created_at}>{date(entry.records[0].created_at)}</time>{entry.durationMs !== undefined && <span>{entry.durationMs} ms</span>}</div>
    <Tabs value={tab} onChange={setTab} label={t("audit.detailTabs")} items={[
      { value: "result", label: t("audit.readable") }, { value: "arguments", label: t("audit.arguments") }, { value: "raw", label: t("audit.raw") },
    ]}>{value => <div className="audit-detail-body">
      {value === "raw" ? <pre className="audit-raw">{JSON.stringify(entry.records, null, 2)}</pre>
        : value === "arguments" ? entry.arguments !== undefined ? <AuditValue value={entry.arguments} /> : <p className="audit-muted">{t("audit.noArguments")}</p>
          : entry.kind === "partial" ? <p className="audit-muted">{t("audit.partialHint")}</p>
            : entry.result !== undefined ? <>{entry.outcome === "accepted" && <p className="audit-accepted">{t("audit.outcomes.accepted")}</p>}{entry.kind === "context" ? <AuditContext value={entry.result} /> : <AuditValue value={entry.result} />}{entry.kind === "proposal" && entry.arguments !== undefined && <AuditValue value={entry.arguments} />}</>
              : entry.text ? <AuditDocument text={entry.text} /> : <p className="audit-muted">{t(entry.kind === "assistant" ? "audit.noPublicText" : "audit.noResult")}</p>}
    </div>}</Tabs>
    <details className="audit-technical"><summary>{t("audit.technical")}</summary><dl><div><dt>{t("audit.recordSequences")}</dt><dd>{entry.records.map(record => record.sequence).join(", ")}</dd></div>{entry.callId && <div><dt>{t("audit.callId")}</dt><dd><code>{entry.callId}</code></dd></div>}</dl></details>
  </section>;
}
