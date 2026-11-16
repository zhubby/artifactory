/**
 * [INPUT]: 依赖租户、项目 slug 和页面段。
 * [OUTPUT]: 提供工作区路由生成和合法性校验。
 * [POS]: 签名设置与业务页面的公开地址边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
/** Public page identifiers are stored slugs and project-local build numbers. */
export function isSlug(value: string) {
  return value.length <= 63 && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(value);
}

export function tenantFromPath(path: string) {
  const segment = /^\/tenants\/([^/]+)(?:\/|$)/.exec(path)?.[1];
  if (!segment) return undefined;
  try {
    const slug = decodeURIComponent(segment);
    return isSlug(slug) ? slug : undefined;
  } catch {
    return undefined;
  }
}

export function projectPath(slug: string) {
  return `/projects/${encodeURIComponent(slug)}`;
}

export function buildPath(projectSlug: string, number: number) {
  return `${projectPath(projectSlug)}/builds/${number}`;
}

export function projectFromPath(path: string) {
  const value = /^\/tenants\/[^/]+\/projects\/([^/]+)(?:\/|$)/.exec(path)?.[1];
  return value && isSlug(value) ? value : undefined;
}
export function projectSwitchPath(path: string, slug: string) {
  const base = projectPath(slug);
  const column =
    /^\/tenants\/[^/]+\/projects\/[^/]+\/(builds|artifacts|releases)\/?$/.exec(
      path,
    )?.[1];
  return column ? `${base}/${column}` : base;
}

export function validTenantRoute(segments: string[]) {
  if (!segments.length) return true;
  if (segments.length === 1) return ["nodes", "settings"].includes(segments[0]);
  if (segments[0] === "settings" && segments.length === 2)
    return [
      "general",
      "members",
      "resources",
      "security",
      "tokens",
      "credentials",
      "signing",
      "emissary",
      "ai",
      "audit",
      "lifecycle",
    ].includes(segments[1]);
  if (segments[0] === "projects" && isSlug(segments[1])) {
    if (segments.length === 2) return true;
    if (segments.length === 3)
      return ["builds", "artifacts", "releases", "settings"].includes(
        segments[2],
      );
    if (segments.length === 4 && segments[2] === "settings")
      return [
        "general",
        "members",
        "resources",
        "signing",
        "lifecycle",
      ].includes(segments[3]);
    if (segments.length === 4 && segments[2] === "releases")
      return /^[0-9a-f-]{36}$/.test(segments[3]);
    if (segments.length === 4 && segments[2] === "builds") {
      if (segments[3] === "new") return true;
      const number = Number(segments[3]);
      return /^[1-9][0-9]*$/.test(segments[3]) && Number.isSafeInteger(number);
    }
  }
  return false;
}
