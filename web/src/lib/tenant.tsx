"use client";

import NextLink from "next/link";
import { usePathname, useRouter as useNextRouter } from "next/navigation";
import type { ComponentProps } from "react";

export function tenantFromPath(path: string) {
  return /^\/t\/([0-9a-f-]{36})(?:\/|$)/i.exec(path)?.[1];
}
export function currentTenant() {
  return typeof window === "undefined"
    ? undefined
    : tenantFromPath(window.location.pathname);
}
export function tenantPath(path: string, tenant = currentTenant()) {
  if (
    !tenant ||
    !path.startsWith("/") ||
    /^\/(?:t|tenants|join|login|register|api|docs)(?:\/|\?|$)/.test(path)
  )
    return path;
  return `/t/${tenant}${path === "/" ? "" : path}`;
}
export function tenantHeaders(): Record<string, string> {
  const tenant = currentTenant();
  return tenant ? { "X-Artifactory-Tenant": tenant } : {};
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
