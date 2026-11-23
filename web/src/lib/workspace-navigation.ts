/**
 * [INPUT]: 依赖项目权限、项目路由和 Lucide 图标。
 * [OUTPUT]: 提供工作区导航、通知集成设置和规范分区判断。
 * [POS]: 统一侧栏、设置内容与命令面板的导航契约。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import {
  Archive, BookOpen, Cpu, FileClock, FolderKanban, KeyRound,
  LayoutDashboard, Settings, ShieldCheck, SlidersHorizontal,
  Sparkles, Users, Workflow, Webhook, type LucideIcon,
} from "lucide-react";
import { projectPath } from "@/lib/routes";
import type { Project } from "@/lib/types";

export type NavigationContext = { project?: Project; tenantAdmin: boolean };
export type NavigationItem = { id: string; path: string; label: string; icon: LucideIcon };
export type NavigationGroup = { label?: string; items: NavigationItem[] };

export function workspaceNavigation(context: NavigationContext): {
  primary: NavigationItem[]; settings?: NavigationItem;
} {
  const { project, tenantAdmin } = context;
  if (project) {
    const base = projectPath(project.slug);
    return {
      primary: [
        { id: "overview", path: base, label: "navigation.overview", icon: LayoutDashboard },
        { id: "releases", path: `${base}/releases`, label: "navigation.releases", icon: Archive },
      ],
      settings: project.effective_role === "admin"
        ? { id: "settings", path: `${base}/settings`, label: "workspace.manageProject", icon: Settings }
        : undefined,
    };
  }
  return {
    primary: [
      { id: "projects", path: "/", label: "navigation.projects", icon: FolderKanban },
      { id: "nodes", path: "/nodes", label: "workspace.sharedDevices", icon: Cpu },
    ],
    settings: {
      id: "settings", path: tenantAdmin ? "/settings" : "/settings/tokens",
      label: tenantAdmin ? "workspace.manageTenant" : "security.tokens", icon: Settings,
    },
  };
}

export function settingsNavigation(context: NavigationContext): NavigationGroup[] {
  const { project, tenantAdmin } = context;
  const base = project ? `${projectPath(project.slug)}/settings` : "/settings";
  const item = (id: string, label: string, icon: LucideIcon): NavigationItem => ({
    id, path: `${base}/${id}`, label, icon,
  });
  const basics = [
    item("general", "workspace.section_general", SlidersHorizontal),
    item("members", "workspace.section_members", Users),
  ];
  const lifecycle = item("lifecycle", "workspace.section_lifecycle", Archive);
  if (project) return project.effective_role === "admin" ? [
    { label: "workspace.settingsBasics", items: basics },
    { label: "workspace.settingsDelivery", items: [
      item("resources", "workspace.settingsProjectResources", Cpu),
      item("signing", "workspace.settingsSigningDefaults", ShieldCheck),
      item("integrations", "workspace.settingsIntegrations", Webhook),
    ] },
    { label: "workspace.settingsDanger", items: [lifecycle] },
  ] : [];
  const tokens = item("tokens", "security.tokens", KeyRound);
  if (!tenantAdmin) return [{ label: "workspace.settingsAccess", items: [tokens] }];
  return [
    { label: "workspace.settingsBasics", items: basics },
    { label: "workspace.settingsDelivery", items: [
      item("ai", "ai.settings", Sparkles),
      item("emissary", "ota.title", Workflow),
      item("integrations", "workspace.settingsIntegrations", Webhook),
      item("signing", "workspace.settingsSigningManagement", ShieldCheck),
    ] },
    { label: "workspace.settingsAccess", items: [
      tokens,
      item("credentials", "security.credentials", BookOpen),
      item("audit", "security.audit", FileClock),
    ] },
    { label: "workspace.settingsDanger", items: [lifecycle] },
  ];
}

export function canonicalSettingsSection(section: string | undefined, context: NavigationContext): string {
  if (!section) return !context.project && !context.tenantAdmin ? "tokens" : "general";
  if (!context.project && section === "security") return "tokens";
  if (!context.project && section === "resources") return "ai";
  return section;
}

export function isNavigationItemActive(relative: string, item: NavigationItem): boolean {
  const path = relative.replace(/\/+$/, "") || "/";
  if (item.id === "overview" || item.path === "/") return path === item.path;
  return path === item.path || path.startsWith(`${item.path}/`);
}
