/**
 * [INPUT]: 依赖租户、项目 slug 和页面段。
 * [OUTPUT]: 提供含设备终端和项目目录的业务路由校验及邮箱返回地址。
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

export function releasePath(projectSlug: string, releaseId: string) {
  return `${projectPath(projectSlug)}/releases/${encodeURIComponent(releaseId)}`;
}

export function buildPath(projectSlug: string, number: number, releaseId?: string) {
  return `${releaseId ? releasePath(projectSlug, releaseId) : projectPath(projectSlug)}/builds/${number}`;
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
  if (/^\/tenants\/[^/]+\/projects\/[^/]+\/releases\//.test(path)) return `${base}/releases`;
  return column ? `${base}/${column}` : base;
}

export function validTenantRoute(segments: string[]) {
  if (!segments.length) return true;
  if (segments.length === 1) return ["projects", "nodes", "settings"].includes(segments[0]);
  if (segments.length === 3 && segments[0] === "nodes" && segments[2] === "terminal")
    return /^[A-Za-z0-9_-]{1,100}$/.test(segments[1]);
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
      "integrations",
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
        "integrations",
      ].includes(segments[3]);
    if (segments.length === 4 && segments[2] === "releases")
      return segments[3] === "new" || /^[0-9a-f-]{36}$/.test(segments[3]);
    if (segments.length === 6 && segments[2] === "releases" &&
      /^[0-9a-f-]{36}$/.test(segments[3]) && segments[4] === "builds")
      return /^[1-9][0-9]*$/.test(segments[5]) && Number.isSafeInteger(Number(segments[5]));
    if (segments.length === 4 && segments[2] === "builds") {
      const number = Number(segments[3]);
      return /^[1-9][0-9]*$/.test(segments[3]) && Number.isSafeInteger(number);
    }
  }
  return false;
}


/** 邮箱验证只接受本站的两条确认路由和单个不透明令牌。 */
export function notificationReturnTo(value:string|null|undefined):string|undefined {
 if(!value || value.length>4096 || !value.startsWith("/") || value.startsWith("//") || /[\\\u0000-\u0020\u007f]/.test(value))return undefined;
 try{
  const url=new URL(value,"https://artifactory.invalid");
  if(url.origin!=="https://artifactory.invalid" || url.hash || !["/notifications/verify","/notifications/email/verify"].includes(url.pathname))return undefined;
  if([...url.searchParams.keys()].some(key=>key!=="token") || url.searchParams.getAll("token").length!==1)return undefined;
  const token=url.searchParams.get("token");
  if(!token || !/^[A-Za-z0-9_-]{1,512}$/.test(token))return undefined;
  return `${url.pathname}?token=${encodeURIComponent(token)}`;
 }catch{return undefined;}
}
