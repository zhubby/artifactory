/**
 * [INPUT]: 依赖租户作用域、CSRF 边界与 React 生命周期。
 * [OUTPUT]: 提供请求、变更和可停止轮询的 useRemote。
 * [POS]: 控制台远端数据的请求与取消边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type SetStateAction,
} from "react";
import { assertTenant, currentTenant, tenantHeaders } from "@/lib/tenant";
import { projectFromPath } from "@/lib/routes";
import { asError, LocalizedError } from "@/lib/errors";

const scopedRequests = new Map<AbortController, string>();
function currentScope() {
  return `${currentTenant() || ""}/${typeof window !== "undefined" ? projectFromPath(location.pathname) || "" : ""}`;
}
export function cancelBusinessRequests(scope: string) {
  for (const [controller, key] of scopedRequests) {
    if (key === scope) {
      controller.abort();
      scopedRequests.delete(controller);
    }
  }
}
export async function api<Type>(
  path: string,
  init: RequestInit = {},
): Promise<Type> {
  const tenant = currentTenant();
  const independent = /^\/(?:auth|tenants|invitations|operations)(?:\/|$)/.test(
    path,
  );
  const scope = currentScope();
  const controller = new AbortController();
  const signal = init.signal
    ? AbortSignal.any([init.signal, controller.signal])
    : controller.signal;
  const check = () => {
    if (signal.aborted || (!independent && scope !== currentScope()))
      throw new DOMException(
        "Workspace changed or request aborted",
        "AbortError",
      );
  };
  const headers = independent ? {} : await tenantHeaders(tenant, signal);
  check();
  if (!independent) scopedRequests.set(controller, scope);
  try {
    let response: Response;
    try {
      response = await fetch(`/api/v1${path}`, {
        ...init,
        signal,
        credentials: "same-origin",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
          ...headers,
          "x-artifactory-csrf": "1",
          ...init.headers,
        },
        cache: "no-store",
      });
    } catch {
      check();
      throw new LocalizedError("common.networkFailed");
    }
    check();
    const body = await response.json().catch(() => undefined);
    check();
    if (!response.ok) {
      if (
        response.status === 401 &&
        !["/auth/login", "/auth/register", "/auth/config"].includes(path) &&
        typeof window !== "undefined"
      ) {
        if (!window.location.pathname.startsWith("/join/"))
          window.location.replace("/login");
      }
      if (body?.code === "tenant_required" && typeof window !== "undefined")
        window.location.replace("/tenants");
      if (path === "/auth/config")
        throw new LocalizedError("register.configFailed");
      if (path === "/auth/register") {
        if (response.status === 400) {
          if (body?.error === "username must contain 1..100 bytes")
            throw new LocalizedError("register.invalidUsername");
          if (body?.error === "password must contain 12..256 bytes")
            throw new LocalizedError("register.invalidPassword");
          throw new LocalizedError("register.invalidFields");
        }
        if (response.status === 403)
          throw new LocalizedError("register.forbidden");
        if (response.status === 409)
          throw new LocalizedError("register.usernameTaken");
        if (response.status === 429)
          throw new LocalizedError("register.rateLimited");
        throw new LocalizedError("register.failed");
      }
      if (typeof body?.error === "string" && body.error)
        throw new Error(body.error);
      throw new LocalizedError("common.httpFailed", {
        status: response.status,
      });
    }
    return body as Type;
  } finally {
    scopedRequests.delete(controller);
  }
}

export async function mutation<Type>(
  path: string,
  body: unknown,
  method = "POST",
) {
  return api<Type>(path, { method, body: JSON.stringify(body) });
}

export function useRemote<Type>(
  path: string | null,
  interval: number | false | ((data?: Type) => number | false) = 10000,
) {
  const [snapshot, setSnapshot] = useState<{
    path: string | null;
    data?: Type;
    error?: Error;
  }>({ path });
  const mounted = useRef(true);
  const requests = useRef(new Set<AbortController>());
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const controller of requests.current) controller.abort();
      requests.current.clear();
    };
  }, []);
  const currentPath = useRef(path);
  currentPath.current = path;
  const setData = useCallback(
    (value: SetStateAction<Type | undefined>) => {
      setSnapshot((previous) => ({
        path,
        data:
          typeof value === "function"
            ? (value as (previous: Type | undefined) => Type | undefined)(
                previous.path === path ? previous.data : undefined,
              )
            : value,
      }));
    },
    [path],
  );
  const reload = useCallback(async () => {
    if (!path) return;
    const controller = new AbortController();
    requests.current.add(controller);
    try {
      const data = await api<Type>(path, { signal: controller.signal });
      if (
        mounted.current &&
        !controller.signal.aborted &&
        currentPath.current === path
      )
        setSnapshot({ path, data });
    } catch (error) {
      if (
        mounted.current &&
        !controller.signal.aborted &&
        currentPath.current === path
      )
        setSnapshot((previous) => ({
          path,
          data: previous.path === path ? previous.data : undefined,
          error: asError(error, "common.requestFailed"),
        }));
    } finally {
      requests.current.delete(controller);
    }
  }, [path]);
  useEffect(() => {
    void reload();
    const pending = requests.current;
    return () => {
      for (const controller of pending) controller.abort();
      pending.clear();
    };
  }, [reload]);
  const data = snapshot.path === path ? snapshot.data : undefined;
  const delay = typeof interval === "function" ? interval(data) : interval;
  useEffect(() => {
    if (!path || delay === false || delay <= 0) return;
    const timer = setInterval(() => void reload(), delay);
    return () => clearInterval(timer);
  }, [reload, path, delay]);
  return {
    data,
    error: snapshot.path === path ? snapshot.error : undefined,
    reload,
    setData,
  };
}
