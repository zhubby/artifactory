/**
 * [INPUT]: 依赖账户通知 API、单行筛选、共享下拉和失效上下文。
 * [OUTPUT]: 提供顶栏铃铛及保留阅读窗口的实时列表与记录。
 * [POS]: 账户通知页面复用共享顶栏；铃铛共用抽屉与授权地址。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import {
  Bell,
  Check,
  ChevronLeft,
  ExternalLink,
  Inbox,
  Settings2,
  X,
} from "lucide-react";
import { useUI } from "@/components/providers";
import { useAccount } from "@/components/account";
import { HeaderTools } from "@/components/header-tools";
import { useNotificationCenter } from "@/components/notification-provider";
import { NotificationSettings } from "@/components/notification-settings";
import { ErrorBox, Loading } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { api, mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { Link } from "@/lib/tenant";
import {
  notificationCategories,
  type NotificationItem,
  type NotificationList,
  type NotificationDetail,
  type NotificationScopes,
} from "@/lib/notification-types";
import { useNotificationCopy } from "@/lib/notification-copy";
import { isNotificationTarget } from "@/lib/notify";
import { NotificationFocusBranch } from "@/components/notifications";

function safeURL(url: string | null) {
  return url?.startsWith("/") && !url.startsWith("//") && !/[\\\r\n]/.test(url)
    ? url
    : null;
}
function comparePosition(a: NotificationItem, b: NotificationItem) {
  const time = Date.parse(a.updated_at) - Date.parse(b.updated_at);
  if (time) return time;
  if (a.updated_at !== b.updated_at)
    return a.updated_at > b.updated_at ? 1 : -1;
  return a.id === b.id ? 0 : a.id > b.id ? 1 : -1;
}
export function NotificationBell({ variant = "icon" }: { variant?: "icon" | "sidebar" }) {
  const c = useNotificationCopy();
  const { summary } = useNotificationCenter();
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger asChild>
        <button
          ref={trigger}
          className={variant === "sidebar" ? "nav-item inbox-nav" : "inbox-bell"}
          aria-label={`${c("title")}${summary.unread_count ? ` · ${summary.unread_count} ${c("unread")}` : ""}`}
        >
          <Bell size={18} aria-hidden="true" />
          {variant === "sidebar" && <span>{c("title")}</span>}
          {summary.unread_count > 0 && (
            <span className="inbox-badge">
              {summary.unread_count > 99 ? "99+" : summary.unread_count}
            </span>
          )}
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="inbox-overlay" />
        <Dialog.Content
          className="inbox-drawer"
          onInteractOutside={(e) => {
            if (isNotificationTarget(e.target)) e.preventDefault();
          }}
          onCloseAutoFocus={(e) => {
            e.preventDefault();
            trigger.current?.focus();
          }}
        >
          <NotificationFocusBranch />
          <header className="inbox-heading">
            <div>
              <Dialog.Title>{c("title")}</Dialog.Title>
              <Dialog.Description>{c("personal")}</Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <Button variant="ghost" size="icon" aria-label={c("close")}>
                <X size={18} />
              </Button>
            </Dialog.Close>
          </header>
          <div className="inbox-drawer-scroll">
            <NotificationInbox />
          </div>
          <footer className="inbox-footer">
            <Link href="/notifications" onClick={() => setOpen(false)}>
              {c("full")}
              <ExternalLink size={14} />
            </Link>
          </footer>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
export function NotificationInbox({ full = false }: { full?: boolean }) {
  const c = useNotificationCopy();
  const { locale, date } = useUI();
  const { version, refresh } = useNotificationCenter();
  const [mode, setMode] = useState("all");
  const [category, setCategory] = useState("");
  const [tenant, setTenant] = useState("");
  const [project, setProject] = useState("");
  const [result, setResult] = useState<NotificationList>();
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [detail, setDetail] = useState<string>();
  const scopes = useRemote<NotificationScopes>(
    full ? "/notifications/scopes" : null,
    false,
  );
  const request = useRef(0);
  const loaded = useRef<NotificationList | undefined>(undefined);
  const currentQuery = useRef("");
  const query = new URLSearchParams({ limit: full ? "30" : "15" });
  if (category) query.set("category", category);
  if (tenant) query.set("tenant_id", tenant);
  if (project) query.set("project_id", project);
  if (mode === "unread") query.set("unread", "true");
  if (mode === "action") query.set("needs_action", "true");
  const key = query.toString();
  currentQuery.current = key;
  const load = useCallback(
    async (cursor?: string, background = false) => {
      const n = ++request.current;
      setBusy(true);
      if (!background) setError(undefined);
      try {
        let value = await api<NotificationList>(
          `/notifications?${key}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ""}`,
          { progress: background ? "background" : "foreground" },
        );
        if (n !== request.current || currentQuery.current !== key) return;
        const head = value;
        const boundary = background ? loaded.current?.items.at(-1) : undefined;
        const collected = [...value.items];
        // 后台从最新页重验到原阅读边界，撤权与筛选变化不会保留旧正文。
        while (
          boundary && value.next_cursor && value.items.length &&
          comparePosition(value.items.at(-1)!, boundary) > 0
        ) {
          value = await api<NotificationList>(
            `/notifications?${key}&cursor=${encodeURIComponent(value.next_cursor)}`,
            { progress: "background" },
          );
          if (n !== request.current || currentQuery.current !== key) return;
          collected.push(...value.items);
        }
        const visibleWindow = boundary
          ? collected.filter((item) => comparePosition(item, boundary) >= 0)
          : collected;
        const candidates = cursor
          ? [...(loaded.current?.items || []), ...visibleWindow]
          : visibleWindow.length ? visibleWindow : head.items;
        const items = [...new Map(candidates.map((item) => [item.id, item])).values()];
        const last = items.at(-1);
        const nextCursor = boundary && visibleWindow.length && last
          ? (value.next_cursor || visibleWindow.length < collected.length
            ? `${last.updated_at}|${last.id}` : null)
          : value.next_cursor;
        const next = { ...head, items, next_cursor: nextCursor };
        loaded.current = next;
        setError(undefined);
        setResult(next);
      } catch (e) {
        if (n === request.current) setError(asError(e));
      } finally {
        if (n === request.current) setBusy(false);
      }
    },
    [key],
  );
  useEffect(() => {
    loaded.current = undefined;
    setResult(undefined);
    void load();
    return () => {
      request.current++;
    };
  }, [load]);
  const lastVersion = useRef(version);
  useEffect(() => {
    if (lastVersion.current !== version) {
      lastVersion.current = version;
      void load(undefined, true);
      if (full) void scopes.reload({ progress: "background" });
    }
  }, [version, load]);
  useEffect(() => {
    if (!scopes.data) return;
    if (tenant && !scopes.data.tenants.some((value) => value.id === tenant)) {
      setTenant("");
      setProject("");
    } else if (
      project &&
      !scopes.data.projects.some(
        (value) =>
          value.id === project && (!tenant || value.tenant_id === tenant),
      )
    )
      setProject("");
  }, [scopes.data, tenant, project]);
  async function mark(item?: NotificationItem) {
    setError(undefined);
    try {
      if (item)
        await mutation(`/notifications/${item.id}/read`, {
          attention_revision: item.attention_revision,
        });
      else if (result)
        await mutation("/notifications/read-all", {
          watermark: result.watermark,
          ...(category ? { category } : {}),
          ...(tenant ? { tenant_id: tenant } : {}),
          ...(project ? { project_id: project } : {}),
        });
      await refresh();
    } catch (e) {
      setError(asError(e));
    }
  }
  if (detail)
    return (
      <NotificationTimeline id={detail} back={() => setDetail(undefined)} />
    );
  const tenants = (scopes.data?.tenants || []).map(
    (value) => [value.id, value.name] as const,
  );
  const projects = (scopes.data?.projects || [])
    .filter((value) => !tenant || value.tenant_id === tenant)
    .map((value) => [value.id, value.name] as const);
  return (
    <section className="inbox-content" aria-label={c("title")}>
      <nav className="inbox-tabs" aria-label={c("title")}>
        {["all", "unread", "action"].map((m) => (
          <button key={m} aria-pressed={mode === m} onClick={() => setMode(m)}>
            {c(m as "all" | "unread" | "action")}
          </button>
        ))}
      </nav>
      <div className="inbox-filters filter-controls">
        <Select
          aria-label={c("category")}
          value={category}
          onChange={(e) => setCategory(e.target.value)}
        >
          <option value="">{c("allCategories")}</option>
          {notificationCategories.map((v) => (
            <option key={v} value={v}>
              {c(v)}
            </option>
          ))}
        </Select>
        {full && (
          <>
            <Select
              aria-label={c("tenant")}
              value={tenant}
              onChange={(e) => {
                setTenant(e.target.value);
                setProject("");
              }}
            >
              <option value="">{c("allTenants")}</option>
              {tenants.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </Select>
            <Select
              aria-label={c("project")}
              value={project}
              onChange={(e) => setProject(e.target.value)}
            >
              <option value="">{c("allProjects")}</option>
              {projects.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </Select>
          </>
        )}
      </div>
      <div className="inbox-list-tools">
        <small>
          {result
            ? `${result.unread_count} ${c("unread")} · ${result.needs_action_count} ${c("action")}`
            : ""}
        </small>
        <Button
          size="sm"
          variant="ghost"
          disabled={!result || busy}
          onClick={() => void mark()}
        >
          <Check size={14} />
          {c("readAll")}
        </Button>
      </div>
      <ErrorBox message={error || scopes.error} />
      {error && (
        <Button variant="secondary" onClick={() => void load()}>
          {c("retry")}
        </Button>
      )}
      {!result && !error ? (
        <Loading variant="list" />
      ) : result?.items.length === 0 ? (
        <div className="inbox-empty">
          <Inbox size={28} />
          <h3>{c("empty")}</h3>
          <p>{c("emptyHint")}</p>
        </div>
      ) : (
        <ul className="inbox-list">
          {result?.items.map((item) => (
            <li
              key={item.id}
              className={
                item.attention_revision > item.read_revision ? "is-unread" : ""
              }
            >
              <button
                className="inbox-item"
                aria-label={item.title[locale === "en" ? "en" : "zh"]}
                onClick={() => setDetail(item.id)}
              >
                <span className="inbox-dot" />
                <span>
                  <strong>{item.title[locale === "en" ? "en" : "zh"]}</strong>
                  <p>{item.summary[locale === "en" ? "en" : "zh"]}</p>
                  <small>
                    {[item.tenant_name, item.project_name]
                      .filter(Boolean)
                      .join(" / ")}{" "}
                    · {date(item.updated_at)}
                    {item.needs_action && <em>{c("action")}</em>}
                  </small>
                </span>
              </button>
              {item.attention_revision > item.read_revision && (
                <Button
                  className="inbox-read"
                  variant="ghost"
                  size="icon"
                  aria-label={c("read")}
                  onClick={() => void mark(item)}
                >
                  <Check size={15} />
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      {result?.next_cursor && (
        <Button
          variant="secondary"
          disabled={busy}
          className="inbox-more"
          onClick={() => void load(result.next_cursor!)}
        >
          {c("more")}
        </Button>
      )}
    </section>
  );
}
function NotificationTimeline({ id, back }: { id: string; back: () => void }) {
  const c = useNotificationCopy();
  const { locale, date } = useUI();
  const detail = useRemote<NotificationDetail>(`/notifications/${id}`, false);
  const { refresh, version } = useNotificationCenter();
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  const [older, setOlder] = useState<NotificationDetail["events"]>([]);
  const [cursor, setCursor] = useState<string | null | undefined>();
  const item = detail.error ? undefined : detail.data?.item;
  const events = [
    ...new Map(
      [...(detail.data?.events || []), ...older].map((event) => [
        event.id,
        event,
      ]),
    ).values(),
  ].sort(
    (a, b) =>
      a.occurred_at.localeCompare(b.occurred_at) || a.id.localeCompare(b.id),
  );
  const snapshot = useRef({
    events,
    cursor: cursor === undefined ? detail.data?.next_cursor : cursor,
  });
  snapshot.current = {
    events,
    cursor: cursor === undefined ? detail.data?.next_cursor : cursor,
  };
  const lastVersion = useRef(version);
  const reloadDetail = detail.reload;
  useEffect(() => {
    if (lastVersion.current === version) return;
    lastVersion.current = version;
    const current = snapshot.current;
    const controller = new AbortController();
    void (async () => {
      await reloadDetail({ progress: "background" });
      // 已读到末尾时从最后事件续读；未读完时保留现有分页边界。
      const last = current.events.at(-1);
      if (current.cursor === null && last && !controller.signal.aborted) {
        try {
          const next = await api<NotificationDetail>(
            `/notifications/${id}?cursor=${encodeURIComponent(`${last.occurred_at}|${last.id}`)}`,
            { progress: "background", signal: controller.signal },
          );
          if (!controller.signal.aborted) {
            setOlder((previous) => [
              ...new Map(
                [...previous, ...next.events].map((event) => [event.id, event]),
              ).values(),
            ]);
            setCursor(next.next_cursor);
            setError(undefined);
          }
        } catch (failure) {
          if (!controller.signal.aborted) setError(asError(failure));
        }
      }
    })();
    return () => controller.abort();
  }, [version, id, reloadDetail]);
  async function read() {
    if (!item) return;
    setBusy(true);
    try {
      await mutation(`/notifications/${id}/read`, {
        attention_revision: item.attention_revision,
      });
      await refresh();
      await detail.reload({ progress: "background" });
    } catch (e) {
      setError(asError(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="inbox-timeline">
      <Button variant="ghost" onClick={back}>
        <ChevronLeft size={15} />
        {c("back")}
      </Button>
      <ErrorBox message={detail.error || error} />
      {!item && !detail.error ? (
        <Loading variant="list" />
      ) : (
        item && (
          <>
            <h2>{item.title[locale === "en" ? "en" : "zh"]}</h2>
            <p>{item.summary[locale === "en" ? "en" : "zh"]}</p>
            <div className="inbox-timeline-actions">
              {safeURL(item.url) && (
                <Button asChild>
                  <Link href={safeURL(item.url)!}>
                    {c("open")}
                    <ExternalLink size={14} />
                  </Link>
                </Button>
              )}
              {item.attention_revision > item.read_revision && (
                <Button
                  variant="secondary"
                  disabled={busy}
                  onClick={() => void read()}
                >
                  {c("read")}
                </Button>
              )}
            </div>
            <h3>{c("history")}</h3>
            <ol>
              {events.map((event) => (
                <li key={event.id}>
                  <strong>{event.title[locale === "en" ? "en" : "zh"]}</strong>
                  <p>{event.summary[locale === "en" ? "en" : "zh"]}</p>
                  <time>{date(event.occurred_at)}</time>
                </li>
              ))}
            </ol>
            {(cursor === undefined ? detail.data?.next_cursor : cursor) && (
              <Button
                variant="secondary"
                disabled={busy}
                onClick={async () => {
                  setBusy(true);
                  try {
                    const next = await api<NotificationDetail>(
                      `/notifications/${id}?cursor=${encodeURIComponent((cursor === undefined ? detail.data?.next_cursor : cursor)!)}`,
                    );
                    setOlder((v) => [...v, ...next.events]);
                    setCursor(next.next_cursor);
                  } catch (e) {
                    setError(asError(e));
                  } finally {
                    setBusy(false);
                  }
                }}
              >
                {c("more")}
              </Button>
            )}
          </>
        )
      )}
    </section>
  );
}
export function NotificationPage() {
  const c = useNotificationCopy();
  const { t } = useUI();
  const account = useAccount();
  const [settings, setSettings] = useState(false);
  return (
    <div className="notification-page">
      <header className="notification-page-header">
        <Link href="/tenants">Artifactory</Link>
        <HeaderTools notification={account.data && <NotificationBell />} />
      </header>
      <main>
        <div className="notification-page-title">
          <div>
            <p className="eyebrow">ARTIFACTORY ACCOUNT</p>
            <h1>{settings ? c("settings") : c("title")}</h1>
            <p>{c("personal")}</p>
          </div>
          <Button
            variant="secondary"
            onClick={() => {
              if (
                !document.querySelector(
                  '.notification-settings form[data-dirty="true"]',
                ) ||
                window.confirm(c("unsaved"))
              )
                setSettings((v) => !v);
            }}
          >
            <Settings2 size={15} />
            {settings ? c("back") : c("settings")}
          </Button>
        </div>
        <ErrorBox message={account.error} />
        {!account.data ? (
          account.error ? <Button variant="secondary" onClick={() => void account.reload()}>{t("common.retry")}</Button> : <Loading variant="list" />
        ) : settings ? (
          <NotificationSettings />
        ) : (
          <NotificationInbox full />
        )}
      </main>
    </div>
  );
}
