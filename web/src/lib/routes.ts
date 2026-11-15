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

export function validTenantRoute(segments: string[]) {
  if (!segments.length) return true;
  if (segments.length === 1)
    return ["projects", "builds", "nodes", "artifacts", "releases", "settings"].includes(segments[0]);
  if (segments[0] === "projects" && isSlug(segments[1])) {
    if (segments.length === 2) return true;
    if (segments.length === 4 && segments[2] === "builds") {
      const number = Number(segments[3]);
      return /^[1-9][0-9]*$/.test(segments[3]) && Number.isSafeInteger(number);
    }
  }
  return segments.length === 2 && (
    (segments[0] === "builds" && segments[1] === "new") ||
    segments[0] === "releases"
  );
}
