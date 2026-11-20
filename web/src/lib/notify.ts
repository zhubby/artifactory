/**
 * [INPUT]: 依赖浏览器会话存储和调用方提供的操作反馈。
 * [OUTPUT]: 提供按来源隔离的 notify、订阅和跳转成功提示的单次恢复。
 * [POS]: 全局通知状态；不发送请求，不保存错误详情或认证材料。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { sameError } from "@/lib/errors";

export type NotificationKind = "success" | "error" | "warning" | "info";
export type NotificationMessage = string | Error;
type Options = { id?: string; persist?: boolean };
export type Notification = {
  id: string;
  explicitId: boolean;
  revision: number;
  kind: NotificationKind;
  message: NotificationMessage;
};

const storageKey = "artifactory:redirect-notification";
const empty: readonly Notification[] = [];
let notifications = empty;
let sequence = 0;
const listeners = new Set<() => void>();

function publish(next: readonly Notification[]) {
  notifications = next;
  for (const listener of listeners) listener();
}

function send(kind: NotificationKind, message: NotificationMessage, options: Options = {}) {
  if (typeof window === "undefined") return "";
  const id = options.id ?? `notification-${++sequence}`;
  // 仅显式跨整页跳转的成功文案进入存储，二十秒后失效。
  if (options.persist && kind === "success" && typeof message === "string") {
    try {
      sessionStorage.setItem(storageKey, JSON.stringify({ message, expires: Date.now() + 20000 }));
    } catch { /* 存储受限时仍显示当前页面通知。 */ }
  }
  const explicitId = options.id !== undefined;
  const duplicate = !explicitId && notifications.find(item => !item.explicitId && item.kind === kind && (
    item.message === message || (item.message instanceof Error && message instanceof Error && sameError(item.message, message))
  ));
  const activeId = duplicate ? duplicate.id : id;
  publish([{ id: activeId, explicitId, revision: ++sequence, kind, message }, ...notifications.filter(item => item.id !== activeId && item.id !== id)].slice(0, 3));
  return activeId;
}

export const notify = {
  success: (message: NotificationMessage, options?: Options) => send("success", message, options),
  error: (message: NotificationMessage, options?: Options) => send("error", message, options),
  warning: (message: NotificationMessage, options?: Options) => send("warning", message, options),
  info: (message: NotificationMessage, options?: Options) => send("info", message, options),
  dismiss: (id: string) => {
    if (notifications.some(item => item.id === id)) publish(notifications.filter(item => item.id !== id));
  },
};

export function subscribeNotifications(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export const getNotifications = () => notifications;
export const getServerNotifications = () => empty;

export function restoreNotification() {
  try {
    const saved = sessionStorage.getItem(storageKey);
    sessionStorage.removeItem(storageKey);
    if (!saved) return;
    const value = JSON.parse(saved);
    if (typeof value.message === "string" && typeof value.expires === "number" && value.expires > Date.now())
      notify.success(value.message);
  } catch { /* 损坏或受限的会话存储不影响页面。 */ }
}

export function isNotificationTarget(target: EventTarget | null) {
  return target instanceof Element && Boolean(target.closest("[data-notify]"));
}
