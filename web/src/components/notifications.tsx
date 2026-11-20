/**
 * [INPUT]: 依赖 notify、弹窗焦点与导航边界、字段校验和语言上下文。
 * [OUTPUT]: 提供顶部通知、无布局错误桥和弹窗通知焦点分支。
 * [POS]: 根 Provider 的唯一通知出口；弹窗与页面共享，不改变内容尺寸。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { useTranslations } from "next-intl";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";
import { useFocusScopeBranch } from "@radix-ui/react-focus-scope";
import { LocalizedError } from "@/lib/errors";
import {
  getNotifications, getServerNotifications, notify, restoreNotification,
  subscribeNotifications, type Notification, type NotificationMessage,
} from "@/lib/notify";

export function ErrorNotification({ message }: { message?: NotificationMessage | null }) {
  const id = useId();
  useEffect(() => {
    if (message) notify.error(message, { id });
    else notify.dismiss(id);
  }, [message, id]);
  return null;
}

export function NotificationFocusBranch() {
  const [viewport, setViewport] = useState<HTMLElement | null>(null);
  useEffect(() => {
    const find = () => {
      const element = document.getElementById("app-notifications");
      if (element) setViewport(element);
      return Boolean(element);
    };
    if (find()) return;
    const observer = new MutationObserver(() => { if (find()) observer.disconnect(); });
    observer.observe(document.body, { childList: true });
    return () => observer.disconnect();
  }, []);
  useFocusScopeBranch(viewport);
  return null;
}

function NotificationCard({ notice }: { notice: Notification }) {
  const t = useTranslations();
  const [paused, setPaused] = useState(false);
  const remaining = useRef(notice.kind === "error" ? 10000 : 5000);
  const previousFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (paused) return;
    const started = Date.now();
    const timer = window.setTimeout(() => notify.dismiss(notice.id), remaining.current);
    return () => {
      window.clearTimeout(timer);
      remaining.current = Math.max(0, remaining.current - (Date.now() - started));
    };
  }, [notice.id, notice.message, paused]);
  const Icon = { success: CheckCircle2, error: XCircle, warning: AlertTriangle, info: Info }[notice.kind];
  const message = notice.message instanceof LocalizedError
    ? t(notice.message.key, notice.message.values)
    : notice.message instanceof Error ? notice.message.message : notice.message;
  return (
    <div
      data-notify=""
      data-kind={notice.kind}
      role={notice.kind === "error" ? "alert" : "status"}
      aria-atomic="true"
      className="notification"
      onPointerEnter={() => setPaused(true)}
      onPointerLeave={(event) => setPaused(event.currentTarget.contains(document.activeElement))}
      onFocusCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget) && event.relatedTarget instanceof HTMLElement)
          previousFocus.current = event.relatedTarget;
        setPaused(true);
      }}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setPaused(event.currentTarget.matches(":hover"));
      }}
    >
      <Icon className="notification-icon" size={18} aria-hidden="true" />
      <div className="notification-content">
        {notice.kind === "error" && <strong>{t("common.requestFailed")}</strong>}
        <p>{message}</p>
      </div>
      <button
        type="button"
        className="notification-close"
        aria-label={t("common.dismissNotification")}
        onPointerDown={(event) => event.preventDefault()}
        onClick={(event) => {
          const restore = event.currentTarget === document.activeElement;
          notify.dismiss(notice.id);
          if (restore) previousFocus.current?.focus({ preventScroll: true });
        }}
      ><X size={16} aria-hidden="true" /></button>
    </div>
  );
}

export function Notifications() {
  const t = useTranslations();
  const notices = useSyncExternalStore(subscribeNotifications, getNotifications, getServerNotifications);
  const [mounted, setMounted] = useState(false);
  const hasNotices = notices.length > 0;
  useEffect(() => {
    if (!mounted || !hasNotices) return;
    const viewport = document.getElementById("app-notifications");
    if (!viewport) return;
    let frame = 0;
    let boundaries: HTMLElement[] = [];
    const schedule = () => { if (!frame) frame = requestAnimationFrame(update); };
    const resize = new ResizeObserver(schedule);
    function update() {
      frame = 0;
      const next = Array.from(document.querySelectorAll<HTMLElement>("[data-notification-clearance], [data-notification-heading]"));
      if (next.length !== boundaries.length || next.some((element, index) => element !== boundaries[index])) {
        resize.disconnect();
        boundaries = next;
        boundaries.forEach(element => resize.observe(element));
      }
      const bottom = Math.max(0, ...boundaries.map(element => {
        const box = element.getBoundingClientRect();
        return box.width && box.height ? box.bottom : 0;
      }));
      viewport!.style.setProperty("--notification-clearance", `${bottom ? Math.ceil(bottom + 8) : 0}px`);
    }
    // 只测量抽屉导航边界；浮层位置变化不改变页面尺寸。
    const observer = new MutationObserver(schedule);
    observer.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", schedule);
    update();
    return () => {
      observer.disconnect();
      resize.disconnect();
      window.removeEventListener("resize", schedule);
      cancelAnimationFrame(frame);
      viewport.style.removeProperty("--notification-clearance");
    };
  }, [mounted, hasNotices]);
  useEffect(() => {
    setMounted(true);
    restoreNotification();
    // 原生校验仍阻止提交；只替换浏览器提示气泡，聚焦首个无效字段。
    let frame = 0;
    let firstInvalid: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement | null = null;
    const invalid = (event: Event) => {
      const field = event.target;
      if (!(field instanceof HTMLInputElement || field instanceof HTMLSelectElement || field instanceof HTMLTextAreaElement)) return;
      event.preventDefault();
      if (firstInvalid) return;
      firstInvalid = field;
      notify.error(field.validationMessage, { id: "field-validation" });
      frame = requestAnimationFrame(() => {
        firstInvalid?.focus({ preventScroll: true });
        firstInvalid = null;
      });
    };
    const clearValidation = () => notify.dismiss("field-validation");
    document.addEventListener("invalid", invalid, true);
    document.addEventListener("submit", clearValidation, true);
    const focusNotification = (event: KeyboardEvent) => {
      if (event.key !== "F8") return;
      const close = document.querySelector<HTMLButtonElement>("#app-notifications .notification-close");
      if (!close) return;
      event.preventDefault();
      close.focus({ preventScroll: true });
    };
    document.addEventListener("keydown", focusNotification);
    return () => {
      document.removeEventListener("invalid", invalid, true);
      document.removeEventListener("submit", clearValidation, true);
      document.removeEventListener("keydown", focusNotification);
      cancelAnimationFrame(frame);
    };
  }, []);
  if (!mounted) return null;
  return createPortal(
    <section id="app-notifications" className="notification-viewport" aria-label={t("common.notifications")} aria-live="polite" aria-relevant="additions text">
      {notices.map(notice => <NotificationCard key={`${notice.id}-${notice.revision}`} notice={notice} />)}
    </section>,
    document.body,
  );
}
