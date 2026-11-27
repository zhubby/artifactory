/**
 * [INPUT]: 依赖工作空间授权项目快照、共享表单、下拉和反馈。
 * [OUTPUT]: 提供可搜索、排序、筛选和切换视图的项目目录。
 * [POS]: Console 的工作空间入口；只展示真实快照，不为装饰追加请求。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useState } from "react";
import { ArrowRight, ArrowUpRight, FolderGit2, GitBranch, LayoutGrid, List, Plus, Search, Settings2, ShieldCheck } from "lucide-react";
import { useUI } from "@/components/providers";
import { ProjectForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Select } from "@/components/ui/select";
import { Modal } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, PageTitle, Status } from "@/components/common";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { projectPath } from "@/lib/routes";
import type { Credential, Pool, Project, TenantContext, User } from "@/lib/types";

export function ProjectsWorkspace({ context, user, projects, reload }: {
  context: TenantContext; user: User; projects?: Project[]; reload: () => Promise<void>;
}) {
  const { t, date, number } = useUI();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [sort, setSort] = useState("recent");
  const [view, setView] = useState<"grid" | "list">("grid");
  const [creating, setCreating] = useState(false);
  const pools = useRemote<Pool[]>(creating ? "/pools" : null);
  const credentials = useRemote<Credential[]>(creating ? "/credentials" : null);
  const canCreate = user.role === "admin" && context.tenant.status === "active";
  const needle = query.trim().toLowerCase();
  const filtered = projects?.filter(project =>
    `${project.name} ${project.slug} ${project.repository_url}`.toLowerCase().includes(needle) &&
    (filter === "all" || project.status === filter),
  ).toSorted((a, b) => sort === "name" ? a.name.localeCompare(b.name) :
    (Date.parse(b.last_build?.created_at || "") || 0) - (Date.parse(a.last_build?.created_at || "") || 0));
  const recent = projects?.filter(project => project.last_build).toSorted((a, b) =>
    Date.parse(b.last_build!.created_at) - Date.parse(a.last_build!.created_at))[0];
  return <div className="prism-projects">
    <PageTitle eyebrow={context.tenant.name} title={t("navigation.projects")} description={t("prismProjects.description")}>
      {canCreate && <Button onClick={() => setCreating(true)}><Plus aria-hidden="true" />{t("projects.add")}</Button>}
    </PageTitle>
    <div className="prism-project-summary">
      <span><strong>{projects ? number(projects.length) : "—"}</strong>{t("prismProjects.projectCount")}</span>
      <span><strong>{projects ? number(projects.reduce((sum, project) => sum + project.build_count, 0)) : "—"}</strong>{t("prismProjects.buildCount")}</span>
      <p>{t("prismProjects.summaryHint")}</p>
    </div>
    {recent && <section className="prism-project-feature panel" aria-label={t("prismProjects.latestActivity")}>
      <div className="prism-project-feature-copy"><p className="eyebrow">{t("prismProjects.latestActivity")}</p>
        <h2><Link href={projectPath(recent.slug)}>{recent.name}</Link></h2>
        <p className="prism-feature-repo"><GitBranch size={16} aria-hidden="true" /><span>{recent.repository_url}</span></p>
        <div className="prism-feature-facts"><Status value={recent.last_build!.status} /><span>#{recent.last_build!.number}</span><time>{date(recent.last_build!.created_at)}</time></div>
        <Link className="prism-project-continue" href={projectPath(recent.slug)}>{t("prismProjects.continue")}<ArrowRight size={16} aria-hidden="true" /></Link>
      </div>
      <div className="prism-project-object" aria-hidden="true"><span className="prism-project-orbit" /><span className="prism-project-tile"><FolderGit2 size={44} /></span><span className="prism-project-node"><GitBranch size={21} /></span><span className="prism-project-caption">SOURCE TO RELEASE</span></div>
    </section>}
    <div className="workspace-section-heading prism-project-heading"><div><h2>{t("workspace.allProjects")}</h2><span>{projects ? number(projects.length) : "—"}</span></div><p>{t("prismProjects.collectionHint")}</p></div>
    <div className="prism-project-toolbar">
      <label className="workspace-filter"><Search size={18} aria-hidden="true" /><input value={query} onChange={event => setQuery(event.target.value)} aria-label={t("workspace.searchProject")} placeholder={t("workspace.searchProject")} /></label>
      <Select value={filter} onChange={event => setFilter(event.target.value)} aria-label={t("prismProjects.filter")}><option value="all">{t("prismProjects.all")}</option><option value="active">{t("lifecycle.active")}</option><option value="archived">{t("lifecycle.archived")}</option><option value="deleting">{t("lifecycle.deleting")}</option></Select>
      <Select value={sort} onChange={event => setSort(event.target.value)} aria-label={t("prismProjects.sort")}><option value="recent">{t("prismProjects.recent")}</option><option value="name">{t("prismProjects.name")}</option></Select>
      <div className="prism-project-view" role="group" aria-label={t("prismProjects.view")}><Button variant="ghost" size="icon" aria-label={t("prismProjects.grid")} aria-pressed={view === "grid"} onClick={() => setView("grid")}><LayoutGrid /></Button><Button variant="ghost" size="icon" aria-label={t("prismProjects.list")} aria-pressed={view === "list"} onClick={() => setView("list")}><List /></Button></div>
    </div>
    <p className="prism-project-count" role="status">{t("prismProjects.results", { count: filtered?.length || 0 })}</p>
    {!filtered ? <Loading variant="cards" /> : !filtered.length ? <section className="panel"><Empty title={projects?.length ? t("workspace.noResults") : t(user.role === "admin" ? "projects.first" : "tenants.noProjects")} detail={projects?.length ? t("workspace.trySearch") : t(user.role === "admin" ? "projects.firstDetail" : "tenants.requestAccess")} />{(needle || filter !== "all") && <div className="prism-project-empty-action"><Button variant="secondary" onClick={() => { setQuery(""); setFilter("all"); }}>{t("prismProjects.clear")}</Button></div>}</section> :
      <div className={`project-grid prism-project-grid ${view === "list" ? "is-list" : ""}`}>{filtered.map(project => <article className="project-card prism-project-card" key={project.id}>
        <div className="project-card-identity"><span className="project-glyph"><FolderGit2 size={24} aria-hidden="true" /></span><div className="min-w-0"><span className="prism-project-slug">{project.slug}</span><h2><Link className="project-card-title" href={projectPath(project.slug)}>{project.name}<ArrowUpRight aria-hidden="true" /></Link></h2></div></div>
        <p className="project-card-repository" title={project.repository_url}><GitBranch size={15} aria-hidden="true" /><span>{project.repository_url}</span></p>
        <div className="project-card-latest"><Status value={project.last_build?.status || project.status} /><span>{project.last_build ? `#${project.last_build.number} · ${date(project.last_build.created_at)}` : t("projects.noBuilds")}</span></div>
        <footer><span className="project-card-count"><span>{t("prismProjects.buildCount")}</span><strong>{number(project.build_count)}</strong></span>
          <div>{project.effective_role === "admin" && <Link href={projectPath(project.slug) + "/settings"} className="prism-project-settings" aria-label={t("projects.settingsNamed", { name: project.name })}><Settings2 size={18} aria-hidden="true" /></Link>}<Link href={projectPath(project.slug)} className="prism-project-enter" aria-label={t("prismProjects.enter", { name: project.name })}><ArrowRight size={19} aria-hidden="true" /></Link></div></footer>
      </article>)}</div>}
    <div className="prism-project-foot"><span><ShieldCheck size={16} aria-hidden="true" />{t("prismProjects.permission")}</span><Link href="/tenants">{t("workspace.switchTenant")}<ArrowRight size={16} aria-hidden="true" /></Link></div>
    <Modal open={creating} onOpenChange={setCreating} title={t("projects.dialogTitle")} description={t("projects.dialogDetail")}><ErrorBox message={pools.error || credentials.error} />
      {!pools.data || !credentials.data ? pools.error || credentials.error ? <Button variant="secondary" onClick={() => { void pools.reload(); void credentials.reload(); }}>{t("common.retry")}</Button> : <Loading variant="form" /> : <ProjectForm pools={pools.data} credentials={credentials.data} done={() => { setCreating(false); void reload(); }} />}
    </Modal>
  </div>;
}
