"use client";

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type SetStateAction,
} from "react";
import { assertTenant, currentTenant, tenantHeaders } from "@/lib/tenant";
import { asError, LocalizedError } from "@/lib/errors";

export async function api<Type>(
  path: string,
  init: RequestInit = {},
): Promise<Type> {
  const tenant = currentTenant();
  const headers = /^\/(?:auth|tenants|invitations)(?:\/|$)/.test(path)
    ? {} : await tenantHeaders(tenant, init.signal);
  assertTenant(tenant, init.signal);
  let response: Response;
  try {
    response = await fetch(`/api/v1${path}`, {
      ...init,
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
    assertTenant(tenant, init.signal);
    throw new LocalizedError("common.networkFailed");
  }
  assertTenant(tenant, init.signal);
  const body = await response.json().catch(() => undefined);
  assertTenant(tenant, init.signal);
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
    throw new LocalizedError("common.httpFailed", { status: response.status });
  }
  return body as Type;
}

export function mutation<Type>(path: string, body: unknown, method = "POST") {
  return api<Type>(path, { method, body: JSON.stringify(body) });
}

export function useRemote<Type>(path: string | null, interval = 10000) {
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
    const timer = path ? setInterval(() => void reload(), interval) : undefined;
    const pending = requests.current;
    return () => {
      clearInterval(timer);
      for (const controller of pending) controller.abort();
      pending.clear();
    };
  }, [reload, path, interval]);
  return {
    data: snapshot.path === path ? snapshot.data : undefined,
    error: snapshot.path === path ? snapshot.error : undefined,
    reload,
    setData,
  };
}
