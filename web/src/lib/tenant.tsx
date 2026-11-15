"use client";

import NextLink from "next/link";
import { usePathname, useRouter as useNextRouter } from "next/navigation";
import type { ComponentProps } from "react";
import { LocalizedError } from "@/lib/errors";
import { tenantFromPath } from "@/lib/routes";

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
    /^\/(?:tenants|join|login|register|api|docs)(?:\/|\?|$)/.test(path)
  )
    return path;
  return `/tenants/${tenant}${path === "/" ? "" : path}`;
}

export function assertTenant(tenant: string | undefined, signal?: AbortSignal | null) {
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
          credentials: "same-origin", cache: "no-store", headers: {Accept: "application/json"},
        });
      } catch {
        throw new LocalizedError("common.networkFailed");
      }
      const body = await response.json().catch(() => undefined);
      assertTenant(slug);
      if (!response.ok) {
        if (response.status === 401) window.location.replace("/login");
        if (typeof body?.error === "string") throw new Error(body.error);
        throw new LocalizedError("common.httpFailed", {status: response.status});
      }
      if (typeof body?.id !== "string") throw new LocalizedError("common.requestFailed");
      return body.id;
    })();
    tenantIds.set(slug, pending);
    void pending.catch(() => { if (tenantIds.get(slug) === pending) tenantIds.delete(slug); });
  }
  return pending;
}

export async function tenantHeaders(
  tenant = currentTenant(), signal?: AbortSignal | null,
): Promise<Record<string, string>> {
  assertTenant(tenant, signal);
  if (!tenant) return {};
  const id = await resolveTenant(tenant);
  assertTenant(tenant, signal);
  return { "X-Artifactory-Tenant": id };
}
export function Link({ href, ...props }: ComponentProps<typeof NextLink>) {
  const tenant = tenantFromPath(usePathname());
  return (
    <NextLink
      {...props}
      href={typeof href === "string" ? tenantPath(href, tenant) : href}
    />
  );
}
export function useRouter() {
  const router = useNextRouter();
  const tenant = tenantFromPath(usePathname());
  return {
    ...router,
    push: (path: string) => router.push(tenantPath(path, tenant)),
    replace: (path: string) => router.replace(tenantPath(path, tenant)),
  };
}
