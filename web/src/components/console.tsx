/**
 * [INPUT]: 依赖工作空间路由、项目目录、节点池、设备终端和共享顶栏。
 * [OUTPUT]: 提供保留作用域身份的控制台和交付目录工具层。
 * [POS]: 由租户布局持续挂载，按租户与项目隔离状态，非法路径让位于 404。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { Select } from "@/components/ui/select";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import {
  ArrowRight,
  Menu,
  Plus,
  X,
  GitBranch,
} from "lucide-react";
import { ProjectNotificationFollow } from "@/components/notification-settings";
import { HeaderTools } from "@/components/header-tools";
import { NotificationBell } from "@/components/notification-center";
import { useUI } from "@/components/providers";
import { useAccount } from "@/components/account";
import { WorkspaceSidebar, WorkspaceBreadcrumb } from "@/components/workspace-navigation";
import { CommandPalette } from "@/components/command-palette";
import { ListToolbar } from "@/components/ui/list-toolbar";
import { WorkspaceManagement } from "@/components/workspace-management";
import { NodesWorkspace } from "@/components/nodes-workspace";
import { NodePoolsWorkspace } from "@/components/node-pools-workspace";
import { TerminalWorkspace } from "@/components/terminal-workspace";
import { BuildTable } from "@/components/build-table";
import { BuildDetail } from "@/components/build-detail";
import { ReleaseCreate } from "@/components/release-form";
import { ReleaseTable } from "@/components/release-table";
import { ReleaseDetail } from "@/components/release-detail";
import { ImageCatalog } from "@/components/image-catalog";
import { Dashboard } from "@/components/dashboard";
import { ProjectsWorkspace } from "@/components/projects-workspace";
import { Button } from "@/components/ui/button";
import {
  ArtifactTable,
  Empty,
  ErrorBox,
  Loading,
  PageTitle,
  Status,
} from "@/components/common";
import { useRemote, cancelBusinessRequests } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { projectFromPath, tenantFromPath, projectPath, validTenantRoute } from "@/lib/routes";
import type {
  TenantContext,
  User,
  Project,
  Build,
  Artifact,
  Release,
} from "@/lib/types";
import { compareReleaseVersions } from "@/lib/releases";

export function Console() {
  const pathname = usePathname();
  const tenant = tenantFromPath(pathname);
  let path: string[];
  try {
    path = pathname.split("/").slice(3).filter(Boolean).map(decodeURIComponent);
  } catch {
    return null;
  }
  if (!tenant || !validTenantRoute(path)) return null;
  const project = projectFromPath(pathname);
  return (
    <Workspace
      key={`${tenant}/${project || ""}`}
      tenant={tenant}
      projectSlug={project}
    />
  );
}
function Workspace({
  tenant,
  projectSlug,
}: {
  tenant: string;
  projectSlug?: string;
}) {
  const { t } = useUI();
  const account = useAccount();
  const pathname = usePathname();
  const relative = pathname.replace(/^\/tenants\/[^/]+/, "") || "/";
  const pieces = relative.split("/").filter(Boolean);
  const context = useRemote<TenantContext>(
    account.data ? "/tenant" : null,
    5000,
  );
  const projects = useRemote<Project[]>(
    context.data ? "/projects" : null,
    10000,
  );
  const detail = useRemote<Project>(
    context.data && projectSlug ? `/projects/by-slug/${projectSlug}` : null,
    5000,
  );
  const [mobile, setMobile] = useState(false);
  const navigationRef = useRef<HTMLElement>(null);
  const space = `${tenant}/${projectSlug || ""}`;
  useEffect(() => () => cancelBusinessRequests(space), [space]);
  useEffect(() => {
    setMobile(false);
  }, [pathname]);
  useLayoutEffect(() => {
    if (!mobile) return;
    const previous = document.activeElement as HTMLElement | null;
    const regions = Array.from(
      document.querySelectorAll<HTMLElement>(".workspace,.account-anchor"),
    );
    const overflow = document.body.style.overflow;
    regions.forEach((element) => (element.inert = true));
    document.body.style.overflow = "hidden";
    navigationRef.current?.querySelector<HTMLElement>("a,button")?.focus();
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        setMobile(false);
      }
      if (event.key !== "Tab") return;
      const items = Array.from(
        navigationRef.current?.querySelectorAll<HTMLElement>(
          'a[href],button:not(:disabled),summary,input:not(:disabled),select:not(:disabled),textarea:not(:disabled),[tabindex="0"]',
        ) || [],
      ).filter((element) => element.getClientRects().length);
      const first = items[0],
        last = items.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };
    const wide = window.matchMedia("(min-width: 1024px)");
    const resize = () => {
      if (wide.matches) setMobile(false);
    };
    window.addEventListener("keydown", key);
    wide.addEventListener("change", resize);
    return () => {
      regions.forEach((element) => (element.inert = false));
      document.body.style.overflow = overflow;
      window.removeEventListener("keydown", key);
      wide.removeEventListener("change", resize);
      previous?.focus();
    };
  }, [mobile]);
  const user: User | undefined =
    account.data && context.data
      ? {
          ...account.data,
          tenant_role: context.data.tenant.role,
          project_roles: context.data.project_roles,
          role: ["owner", "admin"].includes(context.data.tenant.role)
            ? "admin"
            : Object.values(context.data.project_roles).some(
                  (role) => role !== "viewer",
                )
              ? "operator"
              : "viewer",
        }
      : undefined;
  const project = detail.data;
  const base = projectSlug ? projectPath(projectSlug) : "";
  const readOnly = context.data?.tenant.status !== "active";
  const workspaceError = account.error || context.error || projects.error || detail.error;
  const retryWorkspace = () => {
    void account.reload();
    void context.reload();
    void projects.reload();
    if (projectSlug) void detail.reload();
  };
  return (
    <div className="app-shell workspace-shell">
      {mobile && (
        <button
          className="workspace-nav-dismiss"
          tabIndex={-1}
          aria-hidden="true"
          aria-label={t("navigation.close")}
          onClick={() => setMobile(false)}
        />
      )}
      <WorkspaceSidebar
        navigationRef={navigationRef}
        tenant={context.data?.tenant}
        project={project}
        tenantAdmin={user?.role === "admin"}
        relativePath={relative}
        projects={projects.data || []}
        ready={!!user && (!projectSlug || !!project)}
        error={account.error || context.error || detail.error}
        onRetry={retryWorkspace}
        mobile={mobile}
        onClose={() => setMobile(false)}
      />
      <div className="workspace">
        <header className="topbar workspace-topbar">
          <Button
            variant="ghost"
            size="icon"
            className="mobile-menu"
            aria-label={t("navigation.open")}
            aria-controls="workspace-navigation"
            aria-expanded={mobile}
            onClick={() => setMobile(!mobile)}
          >
            {mobile ? <X /> : <Menu />}
          </Button>
          <WorkspaceBreadcrumb
            tenant={context.data?.tenant}
            project={project}
            tenantAdmin={user?.role === "admin"}
            relativePath={relative}
          />
          <HeaderTools workspace notification={account.data && <NotificationBell />}>
            {user && <CommandPalette tenantName={context.data?.tenant.name} projects={projects.data || []} project={project} user={user} readOnly={readOnly || (!!project && project.status !== "active")} />}
          </HeaderTools>
        </header>
        <main className="main" id="workspace-content">
          <ErrorBox message={workspaceError} />
          {workspaceError && <div className="mb-5">
            <Button variant="secondary" onClick={retryWorkspace}>{t("common.retry")}</Button>
          </div>}
          {!user || !context.data || !projects.data || (projectSlug && !project) ? (
            workspaceError ? null : <Loading variant="detail" />
          ) : (
            <>
              {(readOnly || (project && project.status !== "active")) && (
                <div className="workspace-state" role="status">
                  <Status
                    value={
                      readOnly ? context.data.tenant.status : project!.status
                    }
                  />
                  <p>{t("workspace.readOnly")}</p>
                  <Link
                    href={
                      project
                        ? `${base}/settings/lifecycle`
                        : "/settings/lifecycle"
                    }
                  >
                    {t("workspace.section_lifecycle")}
                    <ArrowRight size={14} />
                  </Link>
                </div>
              )}
              {project ? (
                <ProjectWorkspace
                  project={project}
                  user={user}
                  context={context.data}
                  projects={projects.data || []}
                  pieces={pieces.slice(2)}
                  readOnly={readOnly || project.status !== "active"}
                  reload={() => {
                    void detail.reload();
                    void context.reload();
                    void projects.reload();
                  }}
                />
              ) : pieces[0] === "nodes" ? (
                pieces[2] === "terminal" ? (
                  <TerminalWorkspace key={pieces[1]} nodeId={pieces[1]} user={user} readOnly={readOnly} />
                ) : <NodesWorkspace user={user} readOnly={readOnly} />
              ) : pieces[0] === "pools" ? (
                <NodePoolsWorkspace user={user} readOnly={readOnly} />
              ) : pieces[0] === "settings" ? (
                <WorkspaceManagement
                  context={context.data}
                  user={user}
                  projects={projects.data || []}
                  section={pieces[1]}
                  reload={() => {
                    void context.reload();
                    void projects.reload();
                  }}
                />
              ) : pieces[0] === "projects" ? (
                <ProjectsWorkspace
                  context={context.data}
                  user={user}
                  projects={projects.data}
                  reload={projects.reload}
                />
              ) : (
                <>
                  <PageTitle eyebrow={t("workspace.tenantWorkbench")}
                    title={context.data.tenant.name} description={t("dashboard.tenantDescription")} />
                  <Dashboard />
                </>
              )}
            </>
          )}
        </main>
      </div>
    </div>
  );
}


function ProjectWorkspace({
  project,
  user,
  context,
  projects,
  pieces,
  readOnly,
  reload,
}: {
  project: Project;
  user: User;
  context: TenantContext;
  projects: Project[];
  pieces: string[];
  readOnly: boolean;
  reload: () => void;
}) {
  const { t } = useUI();
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("");
  const [order, setOrder] = useState("newest");
  const column = pieces[0] || "overview";
  const base = projectPath(project.slug);
  const builds = useRemote<Build[]>(
    column === "builds" && !pieces[1]
      ? `/builds?project_id=${project.id}`
      : null,
    5000,
  );
  const artifacts = useRemote<Artifact[]>(
    column === "artifacts" ? `/artifacts?project_id=${project.id}` : null,
  );
  const releases = useRemote<Release[]>(
    column === "releases" && !pieces[1]
      ? `/releases?project_id=${project.id}`
      : null,
  );
  useEffect(() => { setQuery(""); setStatus(""); }, [column]);
  if (column === "settings")
    return (
      <WorkspaceManagement
        project={project}
        user={user}
        context={context}
        projects={projects}
        section={pieces[1]}
        reload={reload}
      />
    );
  if (column === "releases" && pieces[1] === "new")
    return readOnly || project.effective_role === "viewer" ? <p className="notice">{t(readOnly ? "workspace.readOnly" : "releaseFlow.noPermission")}</p> : <ReleaseCreate project={project} />;
  if (column === "builds" && pieces[1])
    return <BuildDetail key={pieces[1]} projectSlug={project.slug} number={Number(pieces[1])} user={user} readOnly={readOnly} />;
  if (column === "releases" && pieces[1] && pieces[2] === "builds")
    return <BuildDetail key={pieces[3]} projectSlug={project.slug} number={Number(pieces[3])} releaseId={pieces[1]} user={user} readOnly={readOnly} />;
  if (column === "releases" && pieces[1])
    return <ReleaseDetail key={pieces[1]} id={pieces[1]} project={project} readOnly={readOnly} />;
  const matches = (...values: (string | null | undefined)[]) =>
    values.some((value) => value?.toLowerCase().includes(query.toLowerCase()));
  const chronological = (a: { created_at: string }, b: { created_at: string }) =>
    (Date.parse(b.created_at) - Date.parse(a.created_at)) * (order === "newest" ? 1 : -1);
  const visibleBuilds = builds.data?.filter(build => (!status || build.status === status) &&
    matches(String(build.number), build.requested_ref, build.release_version, build.status)).sort(chronological);
  const visibleArtifacts = artifacts.data?.filter(artifact => matches(artifact.delivery_filename,artifact.path,artifact.target,artifact.release_version)).sort(chronological);
  const visibleReleases = releases.data?.filter(release => (!status || release.status === status) && matches(release.version, release.notes)).sort((a,b) => compareReleaseVersions(b.version,a.version) * (order === "newest" ? 1 : -1));
  const listRequest = column === "builds" ? builds : column === "artifacts" ? artifacts : releases;
  return (
    <>
      <PageTitle
        eyebrow={t("workspace.projectWorkbench")}
        title={column === "overview" ? project.name : t(`navigation.${column}`)}
        titleAccessory={column === "overview" ? <ProjectNotificationFollow key={project.id} projectId={project.id} /> : undefined}
        description={
          column === "overview" ? project.repository_url : project.name
        }
      >
        {!readOnly &&
          project.effective_role !== "viewer" &&
          ["overview", "releases"].includes(column) && (
            <Button asChild>
              <Link href={`${base}/releases/new`}>
                <Plus />
                {t("releaseFlow.create")}
              </Link>
            </Button>
          )}
      </PageTitle>
      {column === "overview" ? (
        <>
          <div className="project-identity-strip">
            <span>
              <GitBranch size={15} />
              {project.slug}
            </span>
            <span>
              {t("projects.signingCount", {
                count: project.signing_profiles.length,
              })}
            </span>
            <Status small value={project.status} />
          </div>
          <Dashboard project={project} />
        </>
      ) : (
        <>
          <div className="prism-delivery-toolbar"><ListToolbar query={query} onQueryChange={setQuery}
            count={column === "builds" ? visibleBuilds?.length : column === "artifacts" ? visibleArtifacts?.length : visibleReleases?.length}>
            {["builds","releases"].includes(column) && <Select className="input" aria-label={t("redesign.filterStatus")} value={status} onChange={event => setStatus(event.target.value)}>
              <option value="">{t("redesign.allStatus")}</option>
              {Array.from(new Set((column === "builds" ? builds.data : releases.data)?.map(item => item.status))).map(value => <option key={value} value={value}>{t("status." + value)}</option>)}
            </Select>}
            <Select className="input" aria-label={t("redesign.order")} value={order} onChange={event => setOrder(event.target.value)}>
              <option value="newest">{t(column === "releases" ? "releaseFlow.versionDescending" : "redesign.newest")}</option><option value="oldest">{t(column === "releases" ? "releaseFlow.versionAscending" : "redesign.oldest")}</option>
            </Select>
          </ListToolbar></div>
          <ErrorBox
            message={builds.error || artifacts.error || releases.error}
          />
          <section className="panel">
            {listRequest.error && !listRequest.data ? <Button variant="secondary" className="m-4" onClick={() => void listRequest.reload()}>{t("common.retry")}</Button> : column === "builds" ? (
              builds.data ? (
                <BuildTable
                  builds={visibleBuilds || []}
                />
              ) : (
                <Loading variant="table" />
              )
            ) : column === "artifacts" ? (
              artifacts.data ? (
                <ArtifactTable
                  artifacts={visibleArtifacts || []}
                />
              ) : (
                <Loading variant="table" />
              )
            ) : releases.data ? <ReleaseTable releases={visibleReleases || []} /> : <Loading variant="table" />}
          </section>
          {column === "artifacts" && <ImageCatalog projectId={project.id} query={query} order={order} />}
        </>
      )}
    </>
  );
}
