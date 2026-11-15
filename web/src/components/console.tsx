"use client";
import { Suspense, useEffect, useRef, useState } from "react";
import { usePathname } from "next/navigation";
import {
  Activity,
  ArrowLeft,
  ArrowRight,
  ArrowUpRight,
  BookOpen,
  Boxes,
  FolderGit2,
  LayoutDashboard,
  Menu,
  Package,
  Play,
  Plus,
  Search,
  Server,
  Settings2,
  X,
  GitBranch,
} from "lucide-react";
import { useUI } from "@/components/providers";
import { useAccount } from "@/components/account";
import { BrandMark } from "@/components/brand-mark";
import { SpaceSwitcher } from "@/components/space-switcher";
import { WorkspaceManagement } from "@/components/workspace-management";
import { NodesWorkspace } from "@/components/nodes-workspace";
import { OperationHistory } from "@/components/lifecycle";
import { BuildTable } from "@/components/build-table";
import { BuildDetail } from "@/components/build-detail";
import { BuildWizard } from "@/components/build-wizard";
import { ReleaseDetail } from "@/components/release-detail";
import { ProjectForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Modal } from "@/components/ui/dialog";
import {
  ArtifactTable,
  Empty,
  ErrorBox,
  Loading,
  PageTitle,
  SectionTitle,
  Status,
} from "@/components/common";
import { useRemote, cancelBusinessRequests } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { projectFromPath, tenantFromPath, projectPath } from "@/lib/routes";
import type {
  TenantContext,
  User,
  Project,
  Build,
  Artifact,
  Release,
  Credential,
  Pool,
} from "@/lib/types";
import { cn, short } from "@/lib/utils";

export function Console() {
  const pathname = usePathname();
  const tenant = tenantFromPath(pathname)!;
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
  useEffect(() => {
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
      if (event.key === "Escape") {
        event.preventDefault();
        setMobile(false);
      }
      if (event.key !== "Tab") return;
      const items = Array.from(
        navigationRef.current?.querySelectorAll<HTMLElement>(
          "a[href],button",
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
  const nav = projectSlug
    ? [
        { path: base, label: t("navigation.overview"), icon: LayoutDashboard },
        {
          path: `${base}/builds`,
          label: t("navigation.builds"),
          icon: Activity,
        },
        {
          path: `${base}/artifacts`,
          label: t("navigation.artifacts"),
          icon: Boxes,
        },
        {
          path: `${base}/releases`,
          label: t("navigation.releases"),
          icon: Package,
        },
        ...(project?.effective_role === "admin"
          ? [
              {
                path: `${base}/settings`,
                label: t("workspace.manageProject"),
                icon: Settings2,
              },
            ]
          : []),
      ]
    : [
        { path: "/", label: t("workspace.allProjects"), icon: FolderGit2 },
        { path: "/nodes", label: t("workspace.sharedDevices"), icon: Server },
        ...(user?.role === "admin"
          ? [
              {
                path: "/settings",
                label: t("workspace.manageTenant"),
                icon: Settings2,
              },
            ]
          : [
              {
                path: "/settings/security",
                label: t("security.tokens"),
                icon: Settings2,
              },
            ]),
      ];
  const readOnly = context.data?.tenant.status !== "active";
  return (
    <div className="app-shell workspace-shell">
      {mobile && (
        <button
          className="workspace-nav-dismiss"
          aria-label={t("navigation.close")}
          onClick={() => setMobile(false)}
        />
      )}
      <aside
        ref={navigationRef}
        id="workspace-navigation"
        role={mobile ? "dialog" : undefined}
        aria-modal={mobile ? true : undefined}
        className={cn("sidebar", mobile && "open")}
        aria-label={t("navigation.main")}
      >
        <Button
          variant="ghost"
          size="icon"
          className="mobile-menu sidebar-close"
          aria-label={t("navigation.close")}
          onClick={() => setMobile(false)}
        >
          <X />
        </Button>
        <Link href="/tenants" className="brand">
          <span className="brand-mark">
            <BrandMark />
          </span>
          Artifactory
        </Link>
        {projectSlug && (
          <Link href="/" className="sidebar-back">
            <ArrowLeft size={14} />
            {t("workspace.allProjects")}
          </Link>
        )}
        <div className="sidebar-space">
          <small>
            {t(projectSlug ? "workspace.project" : "workspace.tenant")}
          </small>
          <strong>{project?.name || context.data?.tenant.name || "…"}</strong>
          <span>{projectSlug || tenant}</span>
        </div>
        <nav>
          {nav.map((entry) => (
            <Link
              key={entry.path}
              href={entry.path}
              className={cn(
                "nav-item",
                (entry.path === "/"
                  ? relative === "/"
                  : relative === entry.path ||
                    (entry.path !== base &&
                      relative.startsWith(`${entry.path}/`))) && "active",
              )}
              aria-current={
                (
                  entry.path === "/"
                    ? relative === "/"
                    : relative === entry.path ||
                      (entry.path !== base &&
                        relative.startsWith(`${entry.path}/`))
                )
                  ? "page"
                  : undefined
              }
            >
              <entry.icon />
              {entry.label}
            </Link>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <Link href="/tenants" className="nav-item">
            <Boxes />
            {t("tenants.choose")}
          </Link>
          <a className="nav-item" href="/docs">
            <BookOpen />
            {t("navigation.docs")}
          </a>
        </div>
      </aside>
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
          {context.data ? (
            <SpaceSwitcher
              tenant={context.data.tenant}
              project={project}
              projects={projects.data || []}
            />
          ) : (
            <span className="text-sm text-muted-foreground">Artifactory</span>
          )}
        </header>
        <main className="main space-enter" id="workspace-content">
          <ErrorBox
            message={
              account.error || context.error || projects.error || detail.error
            }
          />
          {!user || !context.data || (projectSlug && !project) ? (
            <Loading />
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
                    {t("lifecycle.operations")}
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
                <NodesWorkspace user={user} readOnly={readOnly} />
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
              ) : (
                <TenantHome
                  context={context.data}
                  user={user}
                  projects={projects.data}
                  reload={projects.reload}
                />
              )}
            </>
          )}
        </main>
        <footer className="workspace-footer">
          <span>Artifactory</span>
          <span>
            {context.data?.tenant.name}
            {project && ` / ${project.name}`}
          </span>
        </footer>
      </div>
    </div>
  );
}

function TenantHome({
  context,
  user,
  projects,
  reload,
}: {
  context: TenantContext;
  user: User;
  projects?: Project[];
  reload: () => Promise<void>;
}) {
  const { t, date, number } = useUI();
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const pools = useRemote<Pool[]>(creating ? "/pools" : null);
  const credentials = useRemote<Credential[]>(creating ? "/credentials" : null);
  const activity = useRemote<Build[]>("/builds", 10000);
  const filtered = projects?.filter((project) =>
    `${project.name} ${project.slug} ${project.repository_url}`
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  return (
    <>
      <PageTitle
        eyebrow={t("workspace.tenantWorkbench")}
        title={context.tenant.name}
        description={t("workspace.projectsDescription")}
      >
        {user.role === "admin" && context.tenant.status === "active" && (
          <Button onClick={() => setCreating(true)}>
            <Plus />
            {t("projects.add")}
          </Button>
        )}
      </PageTitle>
      <div className="workspace-section-heading">
        <div>
          <h2>{t("workspace.allProjects")}</h2>
          <span>{projects ? number(projects.length) : "—"}</span>
        </div>
        <label className="workspace-filter">
          <Search size={16} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={t("workspace.searchProject")}
            placeholder={t("workspace.searchProject")}
          />
        </label>
      </div>
      {!filtered ? (
        <Loading />
      ) : !filtered.length ? (
        <section className="panel">
          <Empty
            title={
              projects?.length
                ? t("workspace.noResults")
                : t(
                    user.role === "admin"
                      ? "projects.first"
                      : "tenants.noProjects",
                  )
            }
            detail={
              projects?.length
                ? t("workspace.trySearch")
                : user.role === "admin"
                  ? t("projects.firstDetail")
                  : t("tenants.requestAccess")
            }
          />
        </section>
      ) : (
        <div className="project-grid">
          {filtered.map((project) => (
            <article className="project-card" key={project.id}>
              <div className="project-card-top">
                <span className="project-glyph">
                  <FolderGit2 size={21} />
                </span>
                <Status small value={project.status} />
              </div>
              <Link
                className="project-card-title"
                href={projectPath(project.slug)}
              >
                {project.name}
                <ArrowUpRight size={18} />
              </Link>
              <p
                className="project-card-repository"
                title={project.repository_url}
              >
                {project.repository_url}
              </p>
              <div className="project-card-metrics">
                <span>
                  <strong>{number(project.build_count)}</strong>
                  {t("navigation.builds")}
                </span>
                <span>
                  {project.last_build ? (
                    <>
                      <Status small value={project.last_build.status} />
                      <small>{date(project.last_build.created_at)}</small>
                    </>
                  ) : (
                    t("projects.noBuilds")
                  )}
                </span>
              </div>
              <div className="project-card-footer">
                <code>{project.slug}</code>
                {project.effective_role === "admin" && (
                  <Link
                    href={`${projectPath(project.slug)}/settings`}
                    aria-label={t("projects.settingsNamed", {
                      name: project.name,
                    })}
                  >
                    <Settings2 size={16} />
                  </Link>
                )}
              </div>
            </article>
          ))}
        </div>
      )}
      <section className="panel tenant-activity">
        <SectionTitle title={t("workspace.recentActivity")} />
        {activity.data ? (
          <BuildTable builds={activity.data.slice(0, 5)} />
        ) : (
          <Loading />
        )}
      </section>
      <OperationHistory tenant={context.tenant.id} />
      <Modal
        open={creating}
        onOpenChange={setCreating}
        title={t("projects.dialogTitle")}
        description={t("projects.dialogDetail")}
      >
        <ErrorBox message={pools.error || credentials.error} />
        {!pools.data || !credentials.data ? (
          <Loading />
        ) : (
          <ProjectForm
            pools={pools.data}
            credentials={credentials.data}
            done={() => {
              setCreating(false);
              void reload();
            }}
          />
        )}
      </Modal>
    </>
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
  const { t, date } = useUI();
  const [query, setQuery] = useState("");
  const column = pieces[0] || "overview";
  const base = projectPath(project.slug);
  const builds = useRemote<Build[]>(
    ["overview", "builds"].includes(column) && !pieces[1]
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
  useEffect(() => setQuery(""), [column]);
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
  if (column === "builds" && pieces[1] === "new")
    return readOnly ? (
      <p className="notice">{t("workspace.readOnly")}</p>
    ) : (
      <Suspense fallback={<Loading />}>
        <BuildWizard projects={[project]} user={user} />
      </Suspense>
    );
  if (column === "builds" && pieces[1])
    return (
      <BuildDetail
        key={pieces[1]}
        projectSlug={project.slug}
        number={Number(pieces[1])}
        user={user}
        readOnly={readOnly}
      />
    );
  if (column === "releases" && pieces[1])
    return <ReleaseDetail id={pieces[1]} projectId={project.id} />;
  const matches = (...values: (string | null | undefined)[]) =>
    values.some((value) => value?.toLowerCase().includes(query.toLowerCase()));
  return (
    <>
      <PageTitle
        eyebrow={t("workspace.projectWorkbench")}
        title={column === "overview" ? project.name : t(`navigation.${column}`)}
        description={
          column === "overview" ? project.repository_url : project.name
        }
      >
        {!readOnly &&
          project.effective_role !== "viewer" &&
          ["overview", "builds"].includes(column) && (
            <Button asChild>
              <Link href={`${base}/builds/new`}>
                <Play />
                {t("builds.start")}
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
          <section className="panel">
            <SectionTitle title={t("projects.builds")}>
              <Link href={`${base}/builds`} className="text-xs text-primary">
                {t("builds.all")} ↗
              </Link>
            </SectionTitle>
            <ErrorBox message={builds.error} />
            {builds.data ? (
              <BuildTable builds={builds.data.slice(0, 10)} />
            ) : (
              <Loading />
            )}
          </section>
          <div className="project-shortcuts">
            <Link href={`${base}/artifacts`}>
              <Boxes />
              <div>
                <strong>{t("navigation.artifacts")}</strong>
                <span>{t("workspace.artifactShortcut")}</span>
              </div>
              <ArrowRight />
            </Link>
            <Link href={`${base}/releases`}>
              <Package />
              <div>
                <strong>{t("navigation.releases")}</strong>
                <span>{t("workspace.releaseShortcut")}</span>
              </div>
              <ArrowRight />
            </Link>
          </div>
        </>
      ) : (
        <>
          <label className="workspace-filter mb-5">
            <Search size={16} />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("search.placeholder")}
              aria-label={t("search.label")}
            />
          </label>
          <ErrorBox
            message={builds.error || artifacts.error || releases.error}
          />
          <section className="panel">
            {column === "builds" ? (
              builds.data ? (
                <BuildTable
                  builds={builds.data.filter((build) =>
                    matches(
                      String(build.number),
                      build.requested_ref,
                      build.release_version,
                      build.status,
                    ),
                  )}
                />
              ) : (
                <Loading />
              )
            ) : column === "artifacts" ? (
              artifacts.data ? (
                <ArtifactTable
                  artifacts={artifacts.data.filter((artifact) =>
                    matches(artifact.path, artifact.target),
                  )}
                />
              ) : (
                <Loading />
              )
            ) : releases.data ? (
              releases.data.length ? (
                <div className="table-scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>{t("releases.versionProject")}</th>
                        <th>{t("releases.commit")}</th>
                        <th>{t("releases.artifacts")}</th>
                        <th>{t("releases.publishedAt")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {releases.data
                        .filter((release) =>
                          matches(release.version, release.commit_sha),
                        )
                        .map((release) => (
                          <tr key={release.id}>
                            <td>
                              <Link
                                className="text-primary"
                                href={`${base}/releases/${release.id}`}
                              >
                                {release.version}
                              </Link>
                            </td>
                            <td className="font-mono">
                              {short(release.commit_sha)}
                            </td>
                            <td>{release.artifact_count}</td>
                            <td>{date(release.created_at)}</td>
                          </tr>
                        ))}
                    </tbody>
                  </table>
                </div>
              ) : (
                <Empty
                  title={t("releases.empty")}
                  detail={t("releases.emptyDetail")}
                />
              )
            ) : (
              <Loading />
            )}
          </section>
        </>
      )}
    </>
  );
}
