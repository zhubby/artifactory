/**
 * [INPUT]: 依赖当前租户、项目、全局错误通知和统一导航定义。
 * [OUTPUT]: 提供工作区侧栏与顶栏面包屑。
 * [POS]: 同一侧栏在业务和设置之间切换；跨租户选择留在顶栏，项目选择留在侧栏。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import type { Ref } from "react";
import { ArrowLeft, BookOpen, ChevronRight, X } from "lucide-react";
import { BrandMark } from "@/components/brand-mark";
import { ProjectSwitcher, TenantSwitcher } from "@/components/space-switcher";
import { Button } from "@/components/ui/button";
import { useUI } from "@/components/providers";
import { Link } from "@/lib/tenant";
import { projectPath } from "@/lib/routes";
import { useRemote } from "@/lib/api";
import type { Project, Release, Tenant } from "@/lib/types";
import { cn } from "@/lib/utils";
import { ErrorBox } from "@/components/common";
import {
  canonicalSettingsSection,
  isNavigationItemActive,
  settingsNavigation,
  workspaceNavigation,
  type NavigationItem,
} from "@/lib/workspace-navigation";

type WorkspaceContext = {
  tenant?: Tenant;
  project?: Project;
  tenantAdmin: boolean;
  relativePath: string;
};

function navigationState({ project, tenantAdmin, relativePath }: WorkspaceContext) {
  const context = { project, tenantAdmin };
  const base = project ? projectPath(project.slug) : "";
  const settingsBase = `${base}/settings`;
  const settings = relativePath === settingsBase || relativePath.startsWith(`${settingsBase}/`);
  const section = canonicalSettingsSection(relativePath.slice(settingsBase.length + 1) || undefined, context);
  const groups = settingsNavigation(context);
  const navigation = workspaceNavigation(context);
  const current = navigation.primary.find((item) => isNavigationItemActive(relativePath, item));
  const currentSetting = groups.flatMap((group) => group.items).find((item) => item.id === section);
  return { base, settingsBase, settings, section, groups, navigation, current, currentSetting };
}

function NavigationLink({ item, active }: { item: NavigationItem; active: boolean }) {
  const { t } = useUI();
  return (
    <Link href={item.path} className={cn("nav-item", active && "active")}
      aria-current={active ? "page" : undefined}>
      <item.icon aria-hidden="true" />
      {t(item.label)}
    </Link>
  );
}

export function WorkspaceSidebar({
  tenant, project, tenantAdmin, relativePath, projects, ready, mobile, onClose, navigationRef,
}: WorkspaceContext & {
  projects: Project[];
  ready: boolean;
  mobile: boolean;
  onClose: () => void;
  navigationRef: Ref<HTMLElement>;
}) {
  const { t } = useUI();
  const state = navigationState({ tenant, project, tenantAdmin, relativePath });
  return (
    <aside ref={navigationRef} id="workspace-navigation"
      role={mobile ? "dialog" : undefined} aria-modal={mobile ? true : undefined}
      className={cn("sidebar", mobile && "open")} aria-label={t("navigation.main")}>
      <Button variant="ghost" size="icon" className="mobile-menu sidebar-close"
        aria-label={t("navigation.close")} onClick={onClose}><X /></Button>
      <Link href="/tenants" className="brand">
        <span className="brand-mark"><BrandMark /></span>Artifactory
      </Link>
      {ready && tenant && (
        <div className="sidebar-context">
          {(project || state.settings) && (
            <Link href={state.settings && project ? state.base : "/"} className="sidebar-back">
              <ArrowLeft size={14} aria-hidden="true" />
              {t(state.settings && project ? "workspace.returnProject" : "workspace.returnTenant")}
            </Link>
          )}
          {project ? <ProjectSwitcher project={project} projects={projects} /> : (
            <div className="workspace-current-scope">
              <small>{t("workspace.tenantWorkbench")}</small>
              <strong title={tenant.name}>{tenant.name}</strong>
            </div>
          )}
        </div>
      )}
      <div className="sidebar-navigation">
        {ready && (state.settings ? (
          <nav className="workspace-settings-navigation" aria-label={t("workspace.managementSections")}>
            {state.groups.map((group, index) => (
              <div className="workspace-settings-group" key={group.label || index}>
                {group.label && <p className="nav-label">{t(group.label)}</p>}
                {group.items.map((item) => (
                  <NavigationLink key={item.id} item={item} active={item.id === state.section} />
                ))}
              </div>
            ))}
          </nav>
        ) : (
          <nav aria-label={t(project ? "workspace.project" : "workspace.tenant")}>
            {state.navigation.primary.map((item) => (
              <NavigationLink key={item.id} item={item} active={isNavigationItemActive(relativePath, item)} />
            ))}
          </nav>
        ))}
      </div>
      <div className="sidebar-bottom">
        {ready && !state.settings && state.navigation.settings && (
          <NavigationLink item={state.navigation.settings} active={false} />
        )}
        <a className="nav-item" href="/docs"><BookOpen aria-hidden="true" />{t("navigation.docs")}</a>
      </div>
    </aside>
  );
}

export function WorkspaceBreadcrumb(context: WorkspaceContext) {
  const { t } = useUI();
  const { tenant, project, relativePath } = context;
  const state = navigationState(context);
  const releaseRoute = /\/releases\/([^/]+)(?:\/builds\/([1-9][0-9]*))?$/.exec(relativePath);
  const release = useRemote<Release>(releaseRoute && releaseRoute[1] !== "new" ? `/releases/${releaseRoute[1]}` : null);
  const build = releaseRoute?.[2] || /\/builds\/([1-9][0-9]*)$/.exec(relativePath)?.[1];
  const settingsLabel = state.navigation.settings?.label || (project ? "workspace.manageProject" : "workspace.manageTenant");
  return (
    <nav className="workspace-breadcrumb" aria-label={t("redesign.navigation")}>
      <ErrorBox message={release.error} />
      {tenant ? <TenantSwitcher tenant={tenant} /> : <span>Artifactory</span>}
      {project && <><ChevronRight aria-hidden="true" /><Link href={state.base} title={project.name}>{project.name}</Link></>}
      {state.settings ? (
        <>
          <ChevronRight aria-hidden="true" />
          {settingsLabel === state.currentSetting?.label ? <span>{t(settingsLabel)}</span> : (
            <>
              <Link href={state.settingsBase} title={t(settingsLabel)}>{t(settingsLabel)}</Link>
              {state.currentSetting && <><ChevronRight aria-hidden="true" /><span title={t(state.currentSetting.label)}>{t(state.currentSetting.label)}</span></>}
            </>
          )}
        </>
      ) : state.current && <><ChevronRight aria-hidden="true" /><span>{t(state.current.label)}</span></>}
      {releaseRoute && <><ChevronRight aria-hidden="true" />{releaseRoute[1] === "new" ? <span>{t("releaseFlow.create")}</span> : <Link href={`${state.base}/releases/${releaseRoute[1]}`}>{release.data?.version || t("releaseFlow.version")}</Link>}</>}
      {build && <><ChevronRight aria-hidden="true" /><span>#{build}</span></>}
    </nav>
  );
}
