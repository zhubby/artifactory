/**
 * [INPUT]: 依赖成员与邀请 API、统一表单、Select 和全局通知。
 * [OUTPUT]: 提供租户成员、邀请和项目成员编辑界面。
 * [POS]: 成员与邀请独立展示首读骨架，刷新保留列表和表单草稿。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useState } from "react";
import {
  CopyButton,
  ErrorBox,
  Field,
  Loading,
  SectionTitle,
} from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Modal } from "@/components/ui/dialog";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type {
  Invitation,
  Project,
  ProjectMember,
  TenantMember,
  User,
} from "@/lib/types";

export function TenantMembers({ user }: { user: User }) {
  const { t, date } = useUI();
  const members = useRemote<TenantMember[]>("/tenant/members");
  const invites = useRemote<Invitation[]>("/tenant/invitations");
  const [error, setError] = useState<Error>();
  const [link, setLink] = useState<string>();
  const [confirm, setConfirm] = useState<{
    kind: "remove" | "transfer";
    member: TenantMember;
  }>();
  async function act(path: string, body: unknown = {}, method = "POST") {
    setError(undefined);
    try {
      await mutation(path, body, method);
      await Promise.all([members.reload(), invites.reload()]);
      notify.success(t("common.operationSucceeded"));
    } catch (error) {
      setError(asError(error));
    }
  }
  return (
    <div className="grid min-w-0 gap-6">
      <ErrorBox message={error || members.error || invites.error} />
      {(members.error || invites.error) && (
        <Button
          variant="secondary"
          onClick={() => void Promise.all([members.reload(), invites.reload()])}
        >
          {t("common.retry")}
        </Button>
      )}
      <section className="panel p-5 sm:p-6">
        <SectionTitle
          title={t("tenants.members")}
          detail={t("tenants.memberDetail")}
        />
        <SubmitForm
          label={t("tenants.add")}
          onSubmit={async (form) => {
            await mutation("/tenant/members", {
              username: form.get("username"),
              role: form.get("role"),
            });
            await members.reload();
          }}
        >
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t("login.username")}>
              <Input name="username" required maxLength={100} />
            </Field>
            <Field label={t("common.role")}>
              <Select className="input" name="role">
                <option value="member">{t("tenants.member")}</option>
                <option value="admin">{t("tenants.admin")}</option>
              </Select>
            </Field>
          </div>
        </SubmitForm>
        {!members.data ? (
          !members.error && <Loading />
        ) : (
          <ul className="mt-6 divide-y divide-border">
            {members.data.map((member) => (
              <li
                key={member.id}
                className="flex flex-wrap items-center gap-3 py-4"
              >
                <span className="min-w-0 flex-1 break-words text-sm">
                  {member.username}
                </span>
                {member.role === "owner" ? (
                  <span className="tag">{t("tenants.owner")}</span>
                ) : (
                  <>
                    <label className="sr-only" htmlFor={`role-${member.id}`}>
                      {member.username} {t("common.role")}
                    </label>
                    <Select
                      id={`role-${member.id}`}
                      className="input w-auto"
                      value={member.role}
                      onChange={(e) =>
                        void act(
                          `/tenant/members/${member.id}`,
                          { role: e.target.value },
                          "PATCH",
                        )
                      }
                    >
                      <option value="member">{t("tenants.member")}</option>
                      <option value="admin">{t("tenants.admin")}</option>
                    </Select>
                    <Button
                      variant="ghost"
                      onClick={() => setConfirm({ kind: "remove", member })}
                    >
                      {t("tenants.remove")}
                    </Button>
                    {user.tenant_role === "owner" && (
                      <Button
                        variant="secondary"
                        onClick={() => setConfirm({ kind: "transfer", member })}
                      >
                        {t("tenants.transfer")}
                      </Button>
                    )}
                  </>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
      <section className="panel p-5 sm:p-6">
        <SectionTitle
          title={t("tenants.invitations")}
          detail={t("tenants.inviteDetail")}
        />
        <SubmitForm
          label={t("tenants.generateInvite")}
          onSubmit={async (form) => {
            const result = await mutation<{ token: string }>(
              "/tenant/invitations",
              { role: form.get("role") },
            );
            setLink(`${window.location.origin}/join/${result.token}`);
            await invites.reload();
          }}
        >
          <Field label={t("common.role")}>
            <Select name="role" className="input">
              <option value="member">{t("tenants.member")}</option>
              <option value="admin">{t("tenants.admin")}</option>
            </Select>
          </Field>
        </SubmitForm>
        {link && (
          <div className="notice mt-4 flex min-w-0 items-start gap-3">
            <code className="min-w-0 flex-1 break-all text-xs">{link}</code>
            <CopyButton value={link} />
          </div>
        )}
        {!invites.data ? (
          !invites.error && <Loading variant="list" />
        ) : <ul className="mt-4 divide-y divide-border">
          {invites.data.map((invite) => (
            <li
              key={invite.id}
              className="flex flex-wrap items-center gap-3 py-3 text-xs"
            >
              <span className="min-w-0 flex-1">
                {t(`tenants.${invite.role}`)} · {date(invite.expires_at)}
              </span>
              <span className="text-muted-foreground">
                {invite.accepted_at
                  ? t("tenants.used")
                  : invite.revoked_at
                    ? t("status.revoked")
                    : Date.parse(invite.expires_at) <= Date.now()
                      ? t("status.expired")
                      : t("status.valid")}
              </span>
              {!invite.accepted_at &&
                !invite.revoked_at &&
                Date.parse(invite.expires_at) > Date.now() && (
                  <Button
                    variant="ghost"
                    onClick={() =>
                      void act(`/tenant/invitations/${invite.id}`, {}, "DELETE")
                    }
                  >
                    {t("tenants.revoke")}
                  </Button>
                )}
            </li>
          ))}
        </ul>}
      </section>
      <Modal
        description={t("tenants.memberDetail")}
        open={!!confirm}
        onOpenChange={(open) => {
          if (!open) setConfirm(undefined);
        }}
        title={t(
          confirm?.kind === "transfer" ? "tenants.transfer" : "tenants.remove",
        )}
      >
        <p className="mb-5 break-words text-sm">
          {t(
            confirm?.kind === "transfer"
              ? "tenants.transferConfirm"
              : "tenants.removeConfirm",
            { name: confirm?.member.username || "" },
          )}
        </p>
        <SubmitForm
          children={null}
          label={t("common.confirm")}
          successMessage={confirm?.kind === "transfer" ? false : undefined}
          onSubmit={async () => {
            if (!confirm) return;
            if (confirm.kind === "transfer") {
              await mutation("/tenant/owner", { user_id: confirm.member.id });
              notify.success(t("common.operationSucceeded"), { persist: true });
              window.location.reload();
            } else {
              await mutation(
                `/tenant/members/${confirm.member.id}`,
                {},
                "DELETE",
              );
              setConfirm(undefined);
              await members.reload();
            }
          }}
        />
      </Modal>
    </div>
  );
}

export function ProjectMembers({ project }: { project: Project; user: User }) {
  const { t } = useUI();
  const members = useRemote<ProjectMember[]>(`/projects/${project.id}/members`);
  const options = useRemote<TenantMember[]>(
    `/projects/${project.id}/member-options`,
  );
  return (
    <section className="panel mt-6 p-5 sm:p-6">
      <SectionTitle
        title={t("tenants.projectMembers")}
        detail={t("tenants.projectMemberDetail")}
      />
      <ErrorBox message={members.error || options.error} />
      {(members.error || options.error) && (
        <Button
          variant="secondary"
          onClick={() => void Promise.all([members.reload(), options.reload()])}
        >
          {t("common.retry")}
        </Button>
      )}
      <SubmitForm
        label={t("tenants.add")}
        onSubmit={async (form) => {
          await mutation(
            `/projects/${project.id}/members/${form.get("user")}`,
            { role: form.get("role") },
            "PUT",
          );
          await members.reload();
        }}
      >
        <div className="grid gap-4 sm:grid-cols-2">
          <Field label={t("tenants.members")}>
            <Select className="input" name="user" required>
              <option value="">{t("common.select")}</option>
              {options.data?.map((member) => (
                <option key={member.id} value={member.id}>
                  {member.username}
                </option>
              ))}
            </Select>
          </Field>
          <Field label={t("common.role")}>
            <Select className="input" name="role">
              <option value="viewer">{t("security.viewer")}</option>
              <option value="operator">{t("security.operator")}</option>
              <option value="admin">{t("security.admin")}</option>
            </Select>
          </Field>
        </div>
      </SubmitForm>
      {!members.data ? (
        !members.error && <Loading />
      ) : (
        <ul className="mt-4 divide-y divide-border">
          {members.data.map((member) => (
            <li
              className="flex flex-wrap items-center gap-4 py-3"
              key={member.user_id}
            >
              <span className="min-w-0 flex-1 break-words text-sm">
                {member.username}
              </span>
              <span className="tag">{t(`security.${member.role}`)}</span>
              <SubmitForm
                children={null}
                label={t("tenants.remove")}
                onSubmit={async () => {
                  await mutation(
                    `/projects/${project.id}/members/${member.user_id}`,
                    {},
                    "DELETE",
                  );
                  await members.reload();
                }}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
