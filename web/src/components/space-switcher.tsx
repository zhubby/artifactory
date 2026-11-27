/**
 * [INPUT]: 依赖租户 API、全局错误通知、项目路由及 UI 词条。
 * [OUTPUT]: 提供顶栏工作空间与侧栏项目切换器，按视口约束菜单。
 * [POS]: 工作空间进入项目目录；项目切换保留栏目或返回项目概览。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useRef, useState } from "react";
import {
  Building2,
  ChevronDown,
  FolderGit2,
  Search,
  ArrowLeft,
  Check,
} from "lucide-react";
import { usePathname } from "next/navigation";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { projectSwitchPath } from "@/lib/routes";
import { useUI } from "@/components/providers";
import type { Tenant, Project } from "@/lib/types";
import { ErrorBox } from "@/components/common";

function fitSelectorMenu(selector: HTMLDetailsElement | null) {
  if (!selector?.open) return;
  const menu = selector.querySelector<HTMLElement>(".space-selector-menu");
  if (!menu) return;
  const available = window.innerHeight - menu.getBoundingClientRect().top - 16;
  menu.style.setProperty("--space-menu-height", `${Math.max(0, available)}px`);
}

function Selector({
  kind,
  label,
  children,
}: {
  kind: "tenant" | "project";
  label: string;
  children: (query: string) => React.ReactNode;
}) {
  const { t } = useUI();
  const [query, setQuery] = useState("");
  const ref = useRef<HTMLDetailsElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const focusFrame = useRef(0);
  useEffect(() => {
    const fit = () => fitSelectorMenu(ref.current);
    const outside = (event: PointerEvent) => {
      if (
        !ref.current?.contains(event.target as globalThis.Node) &&
        ref.current
      )
        ref.current.open = false;
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("resize", fit);
    const resize = new ResizeObserver(fit);
    if (ref.current) resize.observe(ref.current);
    void document.fonts.ready.then(fit);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("resize", fit);
      resize.disconnect();
      cancelAnimationFrame(focusFrame.current);
    };
  }, []);
  return (
    <details
      ref={ref}
      name="workspace-selector"
      className={`space-selector ${kind}`}
      onToggle={(event) => {
        cancelAnimationFrame(focusFrame.current);
        if (event.currentTarget.open) {
          fitSelectorMenu(event.currentTarget);
          document
            .querySelectorAll<HTMLDetailsElement>(".space-selector[open]")
            .forEach((other) => {
              if (other !== event.currentTarget) other.open = false;
            });
          setQuery("");
          focusFrame.current = requestAnimationFrame(() => {
            if (ref.current?.open) input.current?.focus({ preventScroll: true });
          });
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape" && ref.current?.open) {
          event.preventDefault();
          ref.current!.open = false;
          ref.current?.querySelector("summary")?.focus();
          return;
        }
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key))
          return;
        if (
          event.target === input.current &&
          ["Home", "End"].includes(event.key)
        )
          return;
        event.preventDefault();
        if (!ref.current!.open) {
          ref.current!.open = true;
          return;
        }
        const items = Array.from(
          ref.current!.querySelectorAll<HTMLAnchorElement>(
            ".space-selector-options a",
          ),
        );
        const current = items.indexOf(
          document.activeElement as HTMLAnchorElement,
        );
        const next =
          event.key === "Home"
            ? 0
            : event.key === "End"
              ? items.length - 1
              : event.key === "ArrowDown"
                ? (current + 1) % items.length
                : current < 0
                  ? items.length - 1
                  : (current - 1 + items.length) % items.length;
        items[next]?.focus();
      }}
    >
      <summary
        aria-label={t(
          `workspace.switch${kind === "tenant" ? "Tenant" : "Project"}`,
        )}
      >
        {kind === "tenant" ? <Building2 size={17} /> : <FolderGit2 size={17} />}
        <span>
          <small>{t(`workspace.${kind}`)}</small>
          <strong>{label}</strong>
        </span>
        <ChevronDown size={14} />
      </summary>
      <div className="space-selector-menu">
        <label className="space-search">
          <Search size={15} />
          <input
            ref={input}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("workspace.search")}
            aria-label={t(
              `workspace.search${kind === "tenant" ? "Tenant" : "Project"}`,
            )}
          />
        </label>
        <div
          className="space-selector-options"
          onClick={(event) => {
            if ((event.target as Element).closest("a"))
              ref.current!.open = false;
          }}
        >
          {children(query)}
        </div>
      </div>
    </details>
  );
}
export function TenantSwitcher({ tenant }: { tenant: Tenant }) {
  const { t } = useUI();
  const tenants = useRemote<Tenant[]>("/tenants", 30000);
  return (
    <>
      <ErrorBox message={tenants.error} />
      <Selector kind="tenant" label={tenant.name}>
        {(query) => (
          <>
            {tenants.data
              ?.filter(
                (item) =>
                  item.name.toLowerCase().includes(query.toLowerCase()) ||
                  item.slug.includes(query.toLowerCase()),
              )
              .map((item) => (
                <Link
                  key={item.id}
                  href={`/tenants/${item.slug}/projects`}
                  className="space-option"
                  aria-current={item.id === tenant.id ? "true" : undefined}
                >
                  <Building2 size={16} />
                  <span>
                    {item.name}
                    <small>
                      {t(`tenants.${item.role}`)} ·{" "}
                      {t(`lifecycle.${item.status}`)}
                    </small>
                  </span>
                  {item.id === tenant.id && <Check size={14} />}
                </Link>
              ))}
            <div className="space-menu-actions">
              <Link href="/tenants">
                <ArrowLeft size={15} />
                {t("workspace.viewAllTenants")}
              </Link>
            </div>
          </>
        )}
      </Selector>
    </>
  );
}

export function ProjectSwitcher({
  project,
  projects,
}: {
  project: Project;
  projects: Project[];
}) {
  const { t } = useUI();
  const pathname = usePathname();
  return (
      <Selector
        kind="project"
        label={project.name}
      >
        {(query) => (
          <>
            {projects
              .filter(
                (item) =>
                  item.name.toLowerCase().includes(query.toLowerCase()) ||
                  item.slug.includes(query.toLowerCase()),
              )
              .map((item) => (
                <Link
                  key={item.id}
                  href={projectSwitchPath(pathname, item.slug)}
                  className="space-option"
                  aria-current={item.id === project?.id ? "true" : undefined}
                >
                  <FolderGit2 size={16} />
                  <span>
                    {item.name}
                    <small>{t(`lifecycle.${item.status}`)}</small>
                  </span>
                  {item.id === project?.id && <Check size={14} />}
                </Link>
              ))}
            {!projects.some((item) =>
                item.name.toLowerCase().includes(query.toLowerCase()) ||
                item.slug.includes(query.toLowerCase()),
              ) && <p className="p-3 text-xs text-muted-foreground">{t("workspace.noResults")}</p>}
          </>
        )}
      </Selector>
  );
}
