/**
 * [INPUT]: 依赖账户身份、通知摘要 API 和账户级 SSE 失效信号。
 * [OUTPUT]: 提供单连接通知上下文、摘要刷新与内容失效版本。
 * [POS]: 根账户内部的持久通知状态；不连接临时操作提示。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useAccount } from "@/components/account";
import { api } from "@/lib/api";
import type { NotificationSummary } from "@/lib/notification-types";
const empty: NotificationSummary = {
  unread_count: 0,
  needs_action_count: 0,
  watermark: 0,
};
const Context = createContext<{
  summary: NotificationSummary;
  version: number;
  refresh: () => Promise<void>;
}>({ summary: empty, version: 0, refresh: async () => {} });
export const useNotificationCenter = () => useContext(Context);
export function NotificationProvider({ children }: { children: ReactNode }) {
  const account = useAccount();
  const identity = account.data?.id;
  const [summary, setSummary] = useState(empty);
  const [version, setVersion] = useState(0);
  const serial = useRef(0);
  const refresh = useCallback(async () => {
    if (!identity) return;
    const n = ++serial.current;
    try {
      const value = await api<NotificationSummary>("/notifications/summary", {
        progress: "background",
      });
      if (n === serial.current) {
        setSummary(value);
        setVersion((v) => v + 1);
      }
    } catch {
      /* 后台连接失败由定时补读恢复。 */
    }
  }, [identity]);
  useEffect(() => {
    setSummary(empty);
    setVersion(0);
    if (!identity) return;
    let disposed = false;
    let connection: EventSource | undefined;
    let retry: ReturnType<typeof setTimeout>;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const invalidate = () => {
      if (timer) return;
      timer = setTimeout(() => {
        timer = undefined;
        if (!disposed) void refresh();
      }, 200);
    };
    const connect = () => {
      if (disposed) return;
      connection = new EventSource("/api/v1/notifications/events", {
        withCredentials: true,
      });
      connection.onmessage = invalidate;
      connection.addEventListener("invalidate", invalidate);
      connection.onerror = () => {
        if (disposed) return;
        connection?.close();
        retry = setTimeout(connect, 15000);
      };
    };
    void refresh();
    connect();
    const fallback = setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, 60000);
    const visible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", visible);
    window.addEventListener("online", visible);
    return () => {
      disposed = true;
      serial.current++;
      connection?.close();
      clearTimeout(retry);
      clearTimeout(timer);
      clearInterval(fallback);
      document.removeEventListener("visibilitychange", visible);
      window.removeEventListener("online", visible);
    };
  }, [identity, refresh]);
  return (
    <Context.Provider value={{ summary, version, refresh }}>
      {children}
    </Context.Provider>
  );
}
