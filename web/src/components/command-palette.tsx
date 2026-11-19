/**
 * [INPUT]: 依赖统一工作区导航、授权项目与 Modal。
 * [OUTPUT]: 提供 CommandPalette 快捷导航面板。
 * [POS]: 按当前作用域搜索业务与设置，保留项目切换。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useRef, useState } from "react";
import { Search, CornerDownLeft } from "lucide-react";
import { usePathname } from "next/navigation";
import { useRouter } from "@/lib/tenant";
import { projectPath, projectSwitchPath } from "@/lib/routes";
import { settingsNavigation, workspaceNavigation } from "@/lib/workspace-navigation";
import { useUI } from "@/components/providers";
import { Modal } from "@/components/ui/dialog";
import type { Project, User } from "@/lib/types";

export function CommandPalette({ projects, project, user, readOnly, tenantName }: {
  projects: Project[]; project?: Project; user: User; readOnly: boolean; tenantName?: string;
}) {
  const { t } = useUI();
  const router = useRouter();
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const optionRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const searchRef = useRef<HTMLInputElement>(null);
  const id = useId();
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k" && !event.isComposing) {
        event.preventDefault();
        const anotherModal = Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]'))
          .some(dialog => dialog.getClientRects().length > 0 && !dialog.contains(searchRef.current));
        if (anotherModal) return;
        setQuery("");
        setActive(0);
        setOpen(previous => !previous);
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  const navigationContext = { project, tenantAdmin: user.role === "admin" };
  const navigation = workspaceNavigation(navigationContext);
  const settingsItems = settingsNavigation(navigationContext).flatMap(group => group.items);
  const scope = project?.name || tenantName || t("workspace.tenant");
  const items = [
    ...navigation.primary.map(item => ({ path: item.path, label: t(item.label), group: scope })),
    ...(navigation.settings && !settingsItems.some(item => item.label === navigation.settings?.label)
      ? [{ path: navigation.settings.path, label: t(navigation.settings.label), group: scope }] : []),
    ...settingsItems.map(item => ({
      path: item.path, label: t(item.label), group: scope,
    })),
    ...projects.map(item => ({ path: projectSwitchPath(pathname, item.slug), label: item.name, group: t("workspace.project") })),
    ...(project && !readOnly && project.effective_role !== "viewer" ? [{
      path: projectPath(project.slug) + "/releases/new", label: t("releaseFlow.create"), group: project.name,
    }] : []),
    ...(project ? [{ path: "/", label: t("workspace.returnTenant"), group: tenantName || t("workspace.tenant") }] : []),
    { path: "/docs", label: t("navigation.docs"), group: "Artifactory" },
  ].filter(item => (item.label + " " + item.group).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const selected = Math.min(active, Math.max(0, items.length - 1));
  function navigate(index: number) {
    const item = items[index];
    if (!item) return;
    setOpen(false);
    router.push(item.path);
  }
  return <>
    <button type="button" className="command-trigger" aria-label={t("redesign.command")} aria-haspopup="dialog"
      onClick={() => { setQuery(""); setActive(0); setOpen(true); }}>
      <Search size={14} aria-hidden="true" /><span>{t("redesign.quickFind")}</span><kbd>⌘ K</kbd>
    </button>
    <Modal open={open} onOpenChange={setOpen} title={t("redesign.command")} description={t("redesign.commandHint")} initialFocus={searchRef}>
      <label className="command-input">
        <Search size={18} aria-hidden="true" />
        <input ref={searchRef} autoComplete="off" role="combobox" aria-label={t("redesign.commandSearch")}
          aria-autocomplete="list" aria-expanded="true" aria-controls={id + "-results"}
          aria-activedescendant={items.length ? id + "-option-" + selected : undefined}
          value={query} placeholder={t("redesign.commandSearch")}
          onChange={event => { setQuery(event.target.value); setActive(0); }}
          onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (event.key === "Enter") { event.preventDefault(); navigate(selected); }
            if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && items.length) {
              event.preventDefault();
              const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1
                : (selected + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length;
              setActive(next);
              optionRefs.current[next]?.scrollIntoView({ block: "nearest" });
            }
          }} />
      </label>
      <div id={id + "-results"} className="command-results" role="listbox" aria-label={t("redesign.commandResults")}>
        {items.map((item, index) => <button type="button" role="option" tabIndex={-1} className="command-result"
          ref={element => { optionRefs.current[index] = element; }} id={id + "-option-" + index}
          aria-selected={selected === index} key={item.group + item.path} onMouseMove={() => setActive(index)}
          onClick={() => navigate(index)}>
          <span className="min-w-0 break-words">{item.label}</span><small>{item.group}</small>
        </button>)}
        {!items.length && <p className="py-8 text-center text-sm text-muted-foreground">{t("workspace.noResults")}</p>}
      </div>
      <p className="mt-4 flex items-center gap-2 border-t border-border pt-3 text-[11px] text-muted-foreground">
        <CornerDownLeft size={12} aria-hidden="true" />{t("redesign.commandKeyboard")}
      </p>
    </Modal>
  </>;
}
