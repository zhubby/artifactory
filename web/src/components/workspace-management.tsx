"use client";
/**
 * [INPUT]: 依赖工作区权限、路由和资源设置组件。
 * [OUTPUT]: 提供租户与项目配置导航。
 * [POS]: 签名资源和项目默认方案的路由入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { Link } from "@/lib/tenant";
import { useUI } from "@/components/providers";
import { useRemote, mutation } from "@/lib/api";
import type {
  TenantContext,
  Project,
  User,
  Pool,
  Credential,
} from "@/lib/types";
import { SigningSettings, ProjectSigning } from "@/components/signing-settings";
import { LifecycleSettings } from "@/components/lifecycle";
import { Settings } from "@/components/settings";
import { AISettings } from "@/components/ai-settings";
import { EmissarySettings } from "@/components/emissary-ota";
import { TenantMembers, ProjectMembers } from "@/components/tenant-members";
import { ProjectForm, SubmitForm } from "@/components/forms";
import { PageTitle, Field, ErrorBox, Loading } from "@/components/common";
import { Input } from "@/components/ui/input";
import { projectPath } from "@/lib/routes";

export function WorkspaceManagement({
  context,
  project,
  projects,
  user,
  section = "general",
  reload,
}: {
  context: TenantContext;
  project?: Project;
  projects: Project[];
  user: User;
  section?: string;
  reload: () => void;
}) {
  const { t } = useUI();
  const admin = project
    ? project.effective_role === "admin"
    : user.role === "admin";
  const base = project ? `${projectPath(project.slug)}/settings` : "/settings";
  const readOnly =
    context.tenant.status !== "active" ||
    (!!project && project.status !== "active");
  const resources = ["resources", "ai", "emissary"].includes(section);
  const current = resources
    ? "resources"
    : ["security", "tokens", "credentials", "audit"].includes(section)
      ? "security"
      : section;
  const pools = useRemote<Pool[]>(
    project && admin && ["general", "resources"].includes(section)
      ? "/pools"
      : null,
  );
  const credentials = useRemote<Credential[]>(
    project && user.role === "admin" && section === "resources"
      ? "/credentials"
      : null,
  );
  const tabs = project
    ? ["general", "members", "resources", "signing", "lifecycle"]
    : admin
      ? ["general", "members", "resources", "signing", "security", "lifecycle"]
      : ["security"];
  let content;
  if (current === "signing")
    content = project ? (
      <fieldset disabled={readOnly}>
        <ProjectSigning
          project={project}
          editable={admin && user.role === "admin"}
          reload={reload}
        />
      </fieldset>
    ) : admin ? (
      <fieldset disabled={readOnly}>
        <SigningSettings />
      </fieldset>
    ) : (
      <p className="notice">{t("workspace.adminRequired")}</p>
    );
  else if (!admin && !(current === "security" && !project))
    content = <p className="notice">{t("workspace.adminRequired")}</p>;
  else if (current === "lifecycle")
    content = (
      <LifecycleSettings
        path={project ? `/projects/${project.id}` : "/tenant"}
        slug={project?.slug || context.tenant.slug}
        status={project?.status || context.tenant.status}
        allowed={project ? admin : context.tenant.role === "owner"}
        tenant={context.tenant.id}
        project={project?.id}
        onChange={reload}
      />
    );
  else if (current === "members")
    content = (
      <fieldset disabled={readOnly}>
        {project ? (
          <ProjectMembers project={project} user={user} />
        ) : (
          <TenantMembers user={user} />
        )}
      </fieldset>
    );
  else if (project)
    content = (
      <section className="panel management-section">
        <h2>
          {t(
            section === "resources"
              ? "workspace.section_resources"
              : "workspace.projectConfiguration",
          )}
        </h2>
        <ErrorBox message={pools.error || credentials.error} />
        {!pools.data ? (
          <Loading />
        ) : (
          <fieldset
            disabled={
              readOnly || (section === "resources" && user.role !== "admin")
            }
          >
            <ProjectForm
              key={`${project.id}/${section}`}
              section={section === "resources" ? "resources" : "general"}
              project={project}
              pools={pools.data}
              credentials={credentials.data || []}
              canManageResources={user.role === "admin"}
              done={reload}
            />
          </fieldset>
        )}
      </section>
    );
  else if (current === "general")
    content = (
      <section className="panel management-section">
        <h2>{t("workspace.tenantIdentity")}</h2>
        <p className="management-description">{t("workspace.slugStable")}</p>
        <fieldset disabled={readOnly}>
          <SubmitForm
            key={context.tenant.id}
            onSubmit={async (form) => {
              await mutation("/tenant", { name: form.get("name") }, "PATCH");
              reload();
            }}
          >
            <Field label={t("tenants.name")}>
              <Input
                name="name"
                defaultValue={context.tenant.name}
                maxLength={100}
                required
              />
            </Field>
            <Field label="Slug">
              <Input value={context.tenant.slug} readOnly />
            </Field>
          </SubmitForm>
        </fieldset>
      </section>
    );
  else if (resources)
    content = (
      <>
        <nav
          className="management-subnav"
          aria-label={t("workspace.section_resources")}
        >
          <Link
            href="/settings/ai"
            aria-current={section !== "emissary" ? "page" : undefined}
          >
            {t("ai.settings")}
          </Link>
          <Link
            href="/settings/emissary"
            aria-current={section === "emissary" ? "page" : undefined}
          >
            {t("ota.title")}
          </Link>
        </nav>
        <fieldset disabled={readOnly}>
          {section === "emissary" ? (
            <EmissarySettings user={user} />
          ) : (
            <AISettings />
          )}
        </fieldset>
      </>
    );
  else
    content = (
      <fieldset disabled={readOnly}>
        <Settings
          user={user}
          projects={projects}
          section={section === "security" ? "tokens" : section}
        />
      </fieldset>
    );
  return (
    <>
      <PageTitle
        eyebrow={t(project ? "workspace.project" : "workspace.tenant")}
        title={t(
          project ? "workspace.manageProject" : "workspace.manageTenant",
        )}
        description={project?.name || context.tenant.name}
      />
      <nav
        className="management-tabs"
        aria-label={t("workspace.managementSections")}
      >
        {tabs.map((tab) => (
          <Link
            key={tab}
            href={`${base}/${tab}`}
            aria-current={current === tab ? "page" : undefined}
          >
            {t(`workspace.section_${tab}`)}
          </Link>
        ))}
      </nav>
      {content}
    </>
  );
}
