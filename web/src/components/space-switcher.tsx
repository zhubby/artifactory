"use client";
import { useEffect, useRef, useState } from "react";
import {
  Building2,
  ChevronDown,
  FolderGit2,
  Search,
  Settings2,
  ArrowLeft,
  Check,
} from "lucide-react";
import { usePathname } from "next/navigation";
import { useRemote } from "@/lib/api";
import { Link } from "@/lib/tenant";
import { projectPath, projectSwitchPath } from "@/lib/routes";
import { useUI } from "@/components/providers";
import type { Tenant, Project } from "@/lib/types";

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
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (
        !ref.current?.contains(event.target as globalThis.Node) &&
        ref.current
      )
        ref.current.open = false;
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, []);
  return (
    <details
      ref={ref}
      name="workspace-selector"
      className={`space-selector ${kind}`}
      onToggle={(event) => {
        if (event.currentTarget.open) {
          document
            .querySelectorAll<HTMLDetailsElement>(".space-selector[open]")
            .forEach((other) => {
              if (other !== event.currentTarget) other.open = false;
            });
          setQuery("");
          requestAnimationFrame(() => input.current?.focus());
        }
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
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
export function SpaceSwitcher({
  tenant,
  project,
  projects,
}: {
  tenant: Tenant;
  project?: Project;
  projects: Project[];
}) {
  const { t } = useUI();
  const tenants = useRemote<Tenant[]>("/tenants", 30000);
  const pathname = usePathname();
  return (
    <div className="space-switchers">
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
                  href={`/tenants/${item.slug}`}
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
              {["owner", "admin"].includes(tenant.role) && (
                <Link href="/settings">
                  <Settings2 size={15} />
                  {t("workspace.manageTenant")}
                </Link>
              )}
              <Link href="/tenants">
                <ArrowLeft size={15} />
                {t("tenants.choose")}
              </Link>
            </div>
          </>
        )}
      </Selector>
      <span className="space-divider">/</span>
      <Selector
        kind="project"
        label={project?.name || t("workspace.allProjects")}
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
            <div className="space-menu-actions">
              {project?.effective_role === "admin" && (
                <Link href={`${projectPath(project.slug)}/settings`}>
                  <Settings2 size={15} />
                  {t("workspace.manageProject")}
                </Link>
              )}
              <Link href="/">
                <ArrowLeft size={15} />
                {t("workspace.allProjects")}
              </Link>
            </div>
          </>
        )}
      </Selector>
    </div>
  );
}
