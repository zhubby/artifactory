/**
 * [INPUT]: 依赖统一设置导航、项目 API 与签名配置。
 * [OUTPUT]: 提供 WorkspaceManagement 工作区设置。
 * [POS]: 渲染当前设置内容，侧栏独立提供分区导航。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState } from "react";
import { useUI } from "@/components/providers";
import { useRemote, mutation } from "@/lib/api";
import type {
  TenantContext,
  Project,
  User,
  Pool,
  Credential,
} from "@/lib/types";
import { ImageCredentials } from "@/components/image-credentials";
import { SigningSettings, ProjectSigning } from "@/components/signing-settings";
import { LifecycleSettings } from "@/components/lifecycle";
import { Settings } from "@/components/settings";
import { AISettings } from "@/components/ai-settings";
import { EmissarySettings } from "@/components/emissary-ota";
import { TenantMembers, ProjectMembers } from "@/components/tenant-members";
import { ProjectForm, SubmitForm } from "@/components/forms";
import { PageTitle, Field, ErrorBox, Loading } from "@/components/common";
import { Input } from "@/components/ui/input";
import { canonicalSettingsSection, settingsNavigation } from "@/lib/workspace-navigation";

export function WorkspaceManagement({
  context,
  project,
  projects,
  user,
  section: requestedSection,
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
  const [formVersion, setFormVersion] = useState(0);
  const admin = project
    ? project.effective_role === "admin"
    : user.role === "admin";
  const navigationContext = { project, tenantAdmin: user.role === "admin" };
  const section = canonicalSettingsSection(requestedSection, navigationContext);
  const selected = settingsNavigation(navigationContext)
    .flatMap(group => group.items).find(item => item.id === section);
  const readOnly =
    context.tenant.status !== "active" ||
    (!!project && project.status !== "active");
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
  let content;
  if (section === "signing")
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
  else if (!admin && !(!project && section === "tokens"))
    content = <p className="notice">{t("workspace.adminRequired")}</p>;
  else if (section === "lifecycle")
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
  else if (section === "members")
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
              key={`${project.id}/${section}/${formVersion}`}
              section={section === "resources" ? "resources" : "general"}
              project={project}
              pools={pools.data}
              credentials={credentials.data || []}
              canManageResources={user.role === "admin"}
              onCancel={() => setFormVersion(value => value + 1)}
              done={reload}
            />
          </fieldset>
        )}
      </section>
    );
  else if (section === "general")
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
  else if (["ai", "emissary"].includes(section))
    content = (
        <fieldset disabled={readOnly}>
          {section === "emissary" ? (
            <EmissarySettings user={user} />
          ) : (
            <AISettings />
          )}
        </fieldset>
    );
  else
    content = (
      <fieldset disabled={readOnly}>
        <Settings
          user={user}
          projects={projects}
          section={section}
        />
      </fieldset>
    );
  if (project && section === "resources") content = <>{content}<ImageCredentials projectId={project.id} editable={admin && !readOnly} /></>;
  return (
    <>
      <PageTitle
        eyebrow={t(project ? "workspace.project" : "workspace.tenant")}
        title={t(selected?.label || (project ? "workspace.manageProject" : "workspace.manageTenant"))}
        description={project?.name || context.tenant.name}
      />
      <div className="management-content">{content}</div>
    </>
  );
}
