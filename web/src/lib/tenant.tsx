/**
 * [INPUT]: 依赖工作区 URL、Next 导航状态、未保存保护和前台进度计数。
 * [OUTPUT]: 提供租户请求头、账户与租户链接与带加载反馈的路由操作。
 * [POS]: 连接工作区地址、授权请求与导航生命周期。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import NextLink, { useLinkStatus } from "next/link";
import { confirmLeave } from "@/lib/unsaved";
import { usePathname, useRouter as useNextRouter } from "next/navigation";
import { useEffect, useTransition, type ComponentProps } from "react";
import { LocalizedError } from "@/lib/errors";
import { tenantFromPath } from "@/lib/routes";
import { beginProgress } from "@/lib/progress";

export { tenantFromPath } from "@/lib/routes";
export function currentTenant() {
  return typeof window === "undefined"
    ? undefined
    : tenantFromPath(window.location.pathname);
}
export function tenantPath(path: string, tenant = currentTenant()) {
  if (
    !tenant ||
    !path.startsWith("/") ||
    /^\/(?:tenants|join|login|register|api|docs|notifications)(?:\/|\?|$)/.test(path)
  )
    return path;
  return `/tenants/${tenant}${path === "/" ? "" : path}`;
}

export function assertTenant(
  tenant: string | undefined,
  signal?: AbortSignal | null,
) {
  if (signal?.aborted || tenant !== currentTenant())
    throw new DOMException("Tenant changed or request aborted", "AbortError");
}

// Cache only the immutable slug-to-ID mapping. Every business request rechecks authority.
const tenantIds = new Map<string, Promise<string>>();
async function resolveTenant(slug: string): Promise<string> {
  let pending = tenantIds.get(slug);
  if (!pending) {
    pending = (async () => {
      let response: Response;
      try {
        response = await fetch(`/api/v1/tenants/${encodeURIComponent(slug)}`, {
          credentials: "same-origin",
          cache: "no-store",
          headers: { Accept: "application/json" },
        });
      } catch {
        throw new LocalizedError("common.networkFailed");
      }
      const body = await response.json().catch(() => undefined);
      assertTenant(slug);
      if (!response.ok) {
        if (response.status === 401) window.location.replace("/login");
        if (typeof body?.error === "string") throw new Error(body.error);
        throw new LocalizedError("common.httpFailed", {
          status: response.status,
        });
      }
      if (typeof body?.id !== "string")
        throw new LocalizedError("common.requestFailed");
      return body.id;
    })();
    tenantIds.set(slug, pending);
    void pending.catch(() => {
      if (tenantIds.get(slug) === pending) tenantIds.delete(slug);
    });
  }
  return pending;
}

export async function tenantHeaders(
  tenant = currentTenant(),
  signal?: AbortSignal | null,
): Promise<Record<string, string>> {
  assertTenant(tenant, signal);
  if (!tenant) return {};
  const lookup = resolveTenant(tenant);
  // 共享解析仍供其他请求复用，单个调用方取消后立即结束自己的等待。
  const id = await (signal ? new Promise<string>((resolve, reject) => {
    const abort = () => reject(signal.reason);
    signal.addEventListener("abort", abort, { once: true });
    lookup.then(value => {
      signal.removeEventListener("abort", abort);
      resolve(value);
    }, error => {
      signal.removeEventListener("abort", abort);
      reject(error);
    });
  }) : lookup);
  assertTenant(tenant, signal);
  return { "X-Artifactory-Tenant": id };
}
function useNavigationProgress(pending: boolean) {
  useEffect(() => pending ? beginProgress() : undefined, [pending]);
}
function LinkProgress() {
  useNavigationProgress(useLinkStatus().pending);
  return null;
}
export function Link({ href, children, ...props }: ComponentProps<typeof NextLink>) {
  const tenant = tenantFromPath(usePathname());
  return (
    <NextLink
      {...props}
      href={typeof href === "string" ? tenantPath(href, tenant) : href}
    >
      {children}
      <LinkProgress />
    </NextLink>
  );
}
export function useRouter() {
  const router = useNextRouter();
  const tenant = tenantFromPath(usePathname());
  const [pending, startTransition] = useTransition();
  useNavigationProgress(pending);
  return {
    ...router,
    push: (path: string, options?: Parameters<typeof router.push>[1]) => {
      if (confirmLeave()) startTransition(() => router.push(tenantPath(path, tenant), options));
    },
    replace: (path: string, options?: Parameters<typeof router.replace>[1]) => {
      if (confirmLeave()) startTransition(() => router.replace(tenantPath(path, tenant), options));
    },
  };
}
