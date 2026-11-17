/**
 * [INPUT]: 依赖业务 API、UI 词条和输入控件。
 * [OUTPUT]: 提供项目、节点、凭据及密钥表单。
 * [POS]: 收集业务输入并呈现提交反馈。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useUI } from "@/components/providers";

import { useRef, useState, type FormEvent, type ReactNode } from "react";
import { CopyButton, ErrorBox, Field } from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input, Textarea } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { mutation, useRemote } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import type {
  Credential,
  ModelProfile,
  Node,
  Pool,
  Project,
} from "@/lib/types";

export function SubmitForm({
  children,
  onSubmit,
  label,
  onCancel,
}: {
  children: ReactNode;
  onSubmit: (form: FormData, markSaved: () => void) => Promise<void>;
  label?: string;
  onCancel?: () => void;
}) {
  const { t } = useUI();
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const element = event.currentTarget;
    element.dataset.submitting = "true";
    const form = new FormData(event.currentTarget);
    setBusy(true);
    setError(undefined);
    try {
      await onSubmit(form, () => {
        element.dataset.dirty = "false";
      });
      element.dataset.dirty = "false";
    } catch (error) {
      setError(
        error instanceof SyntaxError
          ? new LocalizedError("common.invalidJson")
          : asError(error),
      );
    } finally {
      element.dataset.submitting = "false";
      setBusy(false);
    }
  }
  return (
    <form
      onSubmit={submit}
      onChange={(event) => {
        event.currentTarget.dataset.dirty = "true";
      }}
      className="form-stack"
    >
      <ErrorBox message={error} />
      {children}
      <div className="form-footer">
        {onCancel && (
          <Button variant="secondary" type="button" onClick={onCancel}>
            {t("common.cancel")}
          </Button>
        )}
        <Button type="submit" disabled={busy}>
          {busy ? t("common.processing") : (label ?? t("common.save"))}
        </Button>
      </div>
    </form>
  );
}

const string = (form: FormData, name: string) => String(form.get(name) || "");
const csv = (value: string) =>
  value
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);

export function ProjectForm({
  pools,
  credentials,
  project,
  nodes = [],
  canManageResources = true,
  section = "all",
  onCancel,
  done,
}: {
  pools: Pool[];
  credentials: Credential[];
  project?: Project;
  nodes?: Node[];
  canManageResources?: boolean;
  section?: "all" | "general" | "resources";
  onCancel?: () => void;
  done: () => void;
}) {
  const { t } = useUI();
  const general = section !== "resources";
  const nameInput = useRef<HTMLInputElement>(null);
  const resources = section !== "general";
  const grantsEditable = canManageResources && resources;
  const models =
    useRemote<Pick<ModelProfile, "id" | "name" | "enabled">[]>(
      "/model-options",
    );
  const [model, setModel] = useState(project?.model_profile_id || "");
  const [accessMode, setAccessMode] = useState<"pools" | "nodes">(
    project?.node_access?.mode || "pools",
  );
  const [grants, setGrants] = useState<{ pools: string[]; nodes: string[] }>(
    () => ({
      pools:
        project?.pool_ids ||
        (pools.find((pool) => pool.is_default)
          ? [pools.find((pool) => pool.is_default)!.id]
          : []),
      nodes:
        project?.node_ids ||
        (project?.node_access?.mode === "nodes" ? project.node_access.ids : []),
    }),
  );
  const [deviceQuery, setDeviceQuery] = useState("");
  const [deviceOffset, setDeviceOffset] = useState(0);
  const devices = useRemote<Node[]>(
    canManageResources && accessMode === "nodes"
      ? `/nodes?limit=100&offset=${deviceOffset}&q=${encodeURIComponent(deviceQuery)}`
      : null,
  );
  const [bindings, setBindings] = useState(project?.source_credentials || []);
  return (
    <SubmitForm
      onCancel={onCancel ?? done}
      onSubmit={async (form) => {
        const name = general ? string(form, "name") : project!.name;
        if (general && new TextEncoder().encode(name).length > 100) {
          nameInput.current?.focus();
          throw new LocalizedError("projects.nameTooLong");
        }
        await mutation(
          project ? `/projects/${project.id}` : "/projects",
          {
            name,
            repository_url: general
              ? string(form, "repository_url")
              : project!.repository_url,
            credential_id: grantsEditable
              ? string(form, "credential_id") || null
              : project?.credential_id,
            ...(grantsEditable
              ? { node_access: { mode: accessMode, ids: grants[accessMode] } }
              : {}),
            signing_profiles: grantsEditable
              ? csv(string(form, "signing_profiles"))
              : project?.signing_profiles,
            model_profile_id: model || null,
            build_context: general
              ? string(form, "build_context")
              : project?.build_context || "",
            source_credentials: grantsEditable
              ? bindings
              : project?.source_credentials,
          },
          project ? "PATCH" : "POST",
        );
        done();
      }}
    >
      {general && (
        <>
          <Field label={t("projects.name")}>
            <Input
              ref={nameInput}
              name="name"
              defaultValue={project?.name}
              required
              maxLength={100}
              placeholder={t("projects.namePlaceholder")}
            />
          </Field>
          <Field
            label={t("projects.repository")}
            hint={t("projects.repositoryHint")}
          >
            <Input
              name="repository_url"
              defaultValue={project?.repository_url}
              required
              placeholder="https://git.example.com/team/app.git"
            />
          </Field>
        </>
      )}
      {resources && (
        <fieldset disabled={!canManageResources} className="form-stack">
          <Field label={t("projects.credential")}>
            <select
              name="credential_id"
              className="input"
              defaultValue={project?.credential_id || ""}
            >
              <option value="">{t("projects.anonymous")}</option>
              {credentials.map((credential) => (
                <option value={credential.id} key={credential.id}>
                  {credential.name} · {credential.kind}
                </option>
              ))}
            </select>
          </Field>
          <Field
            label={t("tenants.deviceMode")}
            hint={t("tenants.deviceDetail")}
          >
            <select
              className="input"
              value={accessMode}
              onChange={(event) =>
                setAccessMode(event.target.value as "pools" | "nodes")
              }
            >
              <option value="pools">{t("projects.pools")}</option>
              <option value="nodes">{t("navigation.nodes")}</option>
            </select>
          </Field>
          {accessMode === "nodes" && (
            <Field label={t("search.label")}>
              <Input
                value={deviceQuery}
                onChange={(event) => {
                  setDeviceQuery(event.target.value);
                  setDeviceOffset(0);
                }}
              />
            </Field>
          )}
          <ErrorBox message={devices.error} />
          {devices.error && (
            <Button
              type="button"
              variant="secondary"
              onClick={() => void devices.reload()}
            >
              {t("common.retry")}
            </Button>
          )}
          <div
            className="grid max-h-56 gap-3 overflow-y-auto rounded-lg border border-border p-4"
            key={accessMode}
          >
            {(accessMode === "pools" ? pools : devices.data || nodes).map(
              (resource) => (
                <label
                  key={resource.id}
                  className="flex min-w-0 items-center gap-3 text-sm"
                >
                  <input
                    type="checkbox"
                    name={accessMode}
                    value={resource.id}
                    checked={grants[accessMode].includes(resource.id)}
                    onChange={(event) =>
                      setGrants((previous) => ({
                        ...previous,
                        [accessMode]: event.target.checked
                          ? [...previous[accessMode], resource.id]
                          : previous[accessMode].filter(
                              (id) => id !== resource.id,
                            ),
                      }))
                    }
                  />
                  <span className="break-words">{resource.name}</span>
                </label>
              ),
            )}
          </div>
          {accessMode === "nodes" && (
            <div className="flex flex-wrap items-center justify-between gap-3 text-xs">
              <span>
                {t("tenants.selectedDevices", { count: grants.nodes.length })}
              </span>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="secondary"
                  disabled={!deviceOffset}
                  onClick={() =>
                    setDeviceOffset(Math.max(0, deviceOffset - 100))
                  }
                >
                  {t("common.previous")}
                </Button>
                <Button
                  type="button"
                  variant="secondary"
                  disabled={(devices.data?.length || 0) < 100}
                  onClick={() => setDeviceOffset(deviceOffset + 100)}
                >
                  {t("common.next")}
                </Button>
              </div>
            </div>
          )}
          <Field label={t("projects.signing")} hint={t("projects.signingHint")}>
            <Input
              name="signing_profiles"
              defaultValue={project?.signing_profiles.join(", ")}
              placeholder="apple-team, windows-release"
            />
          </Field>
        </fieldset>
      )}
      {resources && !canManageResources && (
        <p className="notice">{t("tenants.resourcesAdminOnly")}</p>
      )}
      {general && (
        <>
          <Field label={t("ai.modelProfile")} hint={t("ai.projectModelHint")}>
            <select
              className="input"
              value={model}
              onChange={(event) => setModel(event.target.value)}
            >
              <option value="">{t("ai.platformDefault")}</option>
              {(models.data || []).map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </Field>
          <Field
            label={t("ai.projectContext")}
            hint={t("ai.projectContextHint")}
          >
            <Textarea
              name="build_context"
              defaultValue={project?.build_context}
              maxLength={16000}
            />
          </Field>
        </>
      )}
      {resources && (
        <fieldset disabled={!canManageResources} className="space-y-3">
          <legend className="text-sm font-medium">
            {t("ai.submoduleCredentials")}
          </legend>
          <p className="text-xs leading-5 text-muted-foreground">
            {t("ai.submoduleHint")}
          </p>
          {bindings.map((binding, index) => (
            <div className="grid gap-2" key={index}>
              <Input
                aria-label={t("projects.repository")}
                placeholder="https://git.example.com/team/library.git"
                value={binding.repository_url}
                onChange={(event) =>
                  setBindings(
                    bindings.map((entry, i) =>
                      i === index
                        ? { ...entry, repository_url: event.target.value }
                        : entry,
                    ),
                  )
                }
                required
              />
              <div className="flex gap-2">
                <select
                  className="input min-w-0 flex-1"
                  aria-label={t("projects.credential")}
                  required
                  value={binding.credential_id}
                  onChange={(event) =>
                    setBindings(
                      bindings.map((entry, i) =>
                        i === index
                          ? { ...entry, credential_id: event.target.value }
                          : entry,
                      ),
                    )
                  }
                >
                  <option value="">{t("projects.credential")}</option>
                  {credentials.map((credential) => (
                    <option key={credential.id} value={credential.id}>
                      {credential.name}
                    </option>
                  ))}
                </select>
                <Button
                  type="button"
                  variant="ghost"
                  onClick={(event) => {
                    event.currentTarget.form?.setAttribute("data-dirty", "true");
                    setBindings(bindings.filter((_, i) => i !== index));
                  }}
                >
                  {t("ai.remove")}
                </Button>
              </div>
            </div>
          ))}
          <Button
            type="button"
            variant="secondary"
            onClick={(event) => {
              event.currentTarget.form?.setAttribute("data-dirty", "true");
              setBindings([
                ...bindings,
                { repository_url: "", credential_id: "" },
              ]);
            }}
          >
            {t("ai.addBinding")}
          </Button>
        </fieldset>
      )}
    </SubmitForm>
  );
}

export function NodeForm({
  node,
  pools,
  done,
}: {
  node: Node;
  pools: Pool[];
  done: () => void;
}) {
  const { t } = useUI();
  return (
    <SubmitForm
      onCancel={done}
      onSubmit={async (form) => {
        await mutation(
          `/nodes/${node.id}`,
          {
            draining: form.has("draining"),
            revoked: node.revoked,
            slots: Number(form.get("slots")),
            pool_id: string(form, "pool_id"),
            labels: JSON.parse(string(form, "labels")),
            signing_profiles: csv(string(form, "signing_profiles")),
          },
          "PATCH",
        );
        done();
      }}
    >
      <Field label={t("nodes.pool")}>
        <select className="input" name="pool_id" defaultValue={node.pool_id}>
          {pools.map((pool) => (
            <option value={pool.id} key={pool.id}>
              {pool.name}
            </option>
          ))}
        </select>
      </Field>
      <Field label={t("nodes.concurrency")} hint={t("nodes.concurrencyHint")}>
        <Input
          name="slots"
          type="number"
          min={1}
          max={32}
          defaultValue={node.slots}
          required
        />
      </Field>
      <Field label={t("nodes.labels")}>
        <Textarea
          name="labels"
          defaultValue={JSON.stringify(node.labels, null, 2)}
          spellCheck={false}
        />
      </Field>
      <Field label={t("projects.signing")} hint={t("nodes.signingHint")}>
        <Input
          name="signing_profiles"
          defaultValue={node.signing_profiles.join(", ")}
        />
      </Field>
      <label className="flex gap-2 text-sm">
        <input type="checkbox" name="draining" defaultChecked={node.draining} />
        {t("nodes.drain")}
      </label>
    </SubmitForm>
  );
}

export function CredentialForm({ done }: { done: () => void }) {
  const { t } = useUI();
  const [kind, setKind] = useState("https");
  return (
    <SubmitForm
      onCancel={done}
      onSubmit={async (form) => {
        await mutation("/credentials", {
          name: string(form, "name"),
          kind,
          username: string(form, "username"),
          secret: string(form, "secret"),
          known_hosts: string(form, "known_hosts"),
        });
        done();
      }}
    >
      <Field label={t("credentials.name")}>
        <Input name="name" required maxLength={100} />
      </Field>
      <Field label={t("credentials.method")}>
        <select
          className="input"
          value={kind}
          onChange={(event) => setKind(event.target.value)}
        >
          <option value="https">{t("credentials.https")}</option>
          <option value="ssh">{t("credentials.ssh")}</option>
        </select>
      </Field>
      {kind === "https" && (
        <Field label={t("credentials.username")}>
          <Input name="username" autoComplete="off" />
        </Field>
      )}
      <Field
        label={
          kind === "ssh" ? t("credentials.privateKey") : t("credentials.secret")
        }
        hint={t("credentials.secretHint")}
      >
        {kind === "ssh" ? (
          <Textarea
            name="secret"
            required
            autoComplete="off"
            spellCheck={false}
          />
        ) : (
          <Input
            name="secret"
            type="password"
            required
            autoComplete="new-password"
          />
        )}
      </Field>
      {kind === "ssh" && (
        <Field
          label={t("credentials.knownHosts")}
          hint={t("credentials.knownHostsHint")}
        >
          <Textarea name="known_hosts" required spellCheck={false} />
        </Field>
      )}
    </SubmitForm>
  );
}

export function SecretDialog({
  value,
  title,
  close,
  hint,
}: {
  value?: string;
  title: string;
  close: () => void;
  hint?: string;
}) {
  const { t } = useUI();
  return (
    <Modal
      open={!!value}
      onOpenChange={(open) => {
        if (!open) close();
      }}
      title={title}
      description={t("secrets.description")}
    >
      <div className="flex items-start gap-2 rounded-md border border-border bg-background p-3">
        <code className="min-w-0 flex-1 break-all text-xs leading-6 text-primary">
          {value}
        </code>
        <CopyButton value={value || ""} />
      </div>
      <div className="mt-5 text-xs leading-6 text-muted-foreground">
        {hint || t("secrets.emissaryHint")}
      </div>
      <Button className="mt-6 w-full" onClick={close}>
        {t("secrets.saved")}
      </Button>
    </Modal>
  );
}
