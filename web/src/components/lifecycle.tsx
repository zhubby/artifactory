"use client";
import { useState } from "react";
import {
  Archive,
  RotateCcw,
  Trash2,
  RefreshCw,
  X,
  Clock3,
  ArrowUpRight,
} from "lucide-react";
import { useUI } from "@/components/providers";
import { mutation, useRemote } from "@/lib/api";
import type { LifecycleOperation, WorkspaceStatus } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { SubmitForm } from "@/components/forms";
import { ErrorBox, Field, Loading, Status } from "@/components/common";
import { asError } from "@/lib/errors";
import { Link } from "@/lib/tenant";

export function OperationHistory({
  tenant,
  project,
}: {
  tenant?: string;
  project?: string;
}) {
  const { t, date } = useUI();
  const operations = useRemote<LifecycleOperation[]>("/operations", 2000);
  const [error, setError] = useState<Error>();
  const rows = operations.data?.filter(
    (op) =>
      (!tenant || op.tenant_id === tenant) &&
      (!project || op.project_id === project),
  );
  async function action(op: LifecycleOperation, verb: string) {
    try {
      await mutation(`/operations/${op.id}/${verb}`, {});
      await operations.reload();
    } catch (error) {
      setError(asError(error));
    }
  }
  if (!rows?.length && !operations.error) return null;
  return (
    <section className="panel operation-history">
      <div className="section-title">
        <h2>{t("lifecycle.operations")}</h2>
      </div>
      <ErrorBox message={error || operations.error} />
      {rows?.map((op) => (
        <article key={op.id} className="operation-row">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h3>
                {t(`lifecycle.${op.action}`)} ·{" "}
                <span className="font-mono">{op.slug}</span>
              </h3>
              <p className="mt-1 text-xs text-muted-foreground">
                {date(op.created_at)} · {t(`lifecycle.phase_${op.phase}`)}
              </p>
            </div>
            <Status value={op.state} />
          </div>
          {op.state === "waiting" && (
            <p className="mt-3 text-sm text-muted-foreground">
              {t("lifecycle.waitingNotice")}
            </p>
          )}
          {!!op.blockers.length && (
            <ul className="operation-blockers">
              {op.blockers.map((task) => (
                <li key={`${task.kind}:${task.id}`}>
                  <Clock3 size={13} />
                  <span>{task.kind}</span>
                  {task.kind === "build" ? (
                    <Link
                      className="text-primary"
                      href={`/tenants/${task.tenant_slug}/projects/${task.project_slug}/builds/${task.number}`}
                    >
                      {task.project_slug} #{task.number}{" "}
                      <ArrowUpRight size={12} className="inline" />
                    </Link>
                  ) : (
                    <code>{task.id}</code>
                  )}
                  <Status small value={task.status} />
                  {task.kind === "build" && (
                    <span className="text-xs">
                      {t("lifecycle.cancelSeparately")}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
          {op.error && (
            <p role="alert" className="mt-3 text-sm text-destructive">
              {t("lifecycle.failedNotice")}
            </p>
          )}
          {(op.state === "waiting" || op.state === "failed") && (
            <div className="mt-3 flex gap-2">
              {op.state === "waiting" ? (
                <Button
                  variant="secondary"
                  onClick={() => void action(op, "cancel")}
                >
                  <X />
                  {t("lifecycle.cancel")}
                </Button>
              ) : (
                <Button
                  variant="secondary"
                  onClick={() => void action(op, "retry")}
                >
                  <RefreshCw />
                  {t("common.retry")}
                </Button>
              )}
            </div>
          )}
        </article>
      ))}
    </section>
  );
}
export function LifecycleSettings({
  path,
  slug,
  status,
  allowed,
  onChange,
  tenant,
  project,
}: {
  path: string;
  slug: string;
  status: WorkspaceStatus;
  allowed: boolean;
  onChange: () => void;
  tenant: string;
  project?: string;
}) {
  const { t } = useUI();
  const [action, setAction] = useState<"archive" | "restore" | "delete">();
  const [key, setKey] = useState("");
  const impact = useRemote<Record<string, number>>(
    allowed ? `${path}/lifecycle` : null,
    10000,
  );
  function begin(action: "archive" | "restore" | "delete") {
    setKey(crypto.randomUUID());
    setAction(action);
  }
  return (
    <div className="management-stack">
      <section className="panel management-section">
        <h2>{t("lifecycle.title")}</h2>
        <p className="management-description">{t("lifecycle.description")}</p>
        <Status value={status} />
        <ErrorBox message={impact.error} />
        <div className="lifecycle-counts">
          {Object.entries(impact.data || {}).map(([key, value]) => (
            <div key={key}>
              <strong>{value}</strong>
              <span>{t(`lifecycle.count_${key}`)}</span>
            </div>
          ))}
        </div>
        {allowed ? (
          <div className="lifecycle-actions">
            {status === "active" && (
              <Button variant="secondary" onClick={() => begin("archive")}>
                <Archive />
                {t("lifecycle.archive")}
              </Button>
            )}
            {status === "archived" && (
              <Button variant="secondary" onClick={() => begin("restore")}>
                <RotateCcw />
                {t("lifecycle.restore")}
              </Button>
            )}
            {["active", "archived"].includes(status) && (
              <Button variant="destructive" onClick={() => begin("delete")}>
                <Trash2 />
                {t("lifecycle.delete")}
              </Button>
            )}
          </div>
        ) : (
          <p className="notice mt-5">{t("lifecycle.ownerRequired")}</p>
        )}
      </section>
      <OperationHistory tenant={tenant} project={project} />
      <Modal
        open={!!action}
        onOpenChange={(open) => {
          if (!open) setAction(undefined);
        }}
        title={action ? t(`lifecycle.${action}`) : ""}
        description={t(
          action === "delete"
            ? "lifecycle.deleteNotice"
            : action === "archive"
              ? "lifecycle.archiveNotice"
              : "lifecycle.restoreNotice",
        )}
      >
        {action && (
          <SubmitForm
            key={key}
            label={t(`lifecycle.${action}`)}
            onSubmit={async (form) => {
              await mutation(`${path}/lifecycle`, {
                action,
                idempotency_key: key,
                ...(action === "delete"
                  ? {
                      slug: form.get("slug"),
                      current_password: form.get("current_password"),
                    }
                  : {}),
              });
              setAction(undefined);
              onChange();
            }}
          >
            {action === "delete" && (
              <>
                <div className="notice">
                  {Object.entries(impact.data || {}).map(([key, value]) => (
                    <p key={key}>
                      {t(`lifecycle.count_${key}`)}: {value}
                    </p>
                  ))}
                </div>
                <Field label={t("lifecycle.confirmSlug", { slug })}>
                  <Input
                    name="slug"
                    required
                    autoComplete="off"
                    pattern={slug}
                    placeholder={slug}
                  />
                </Field>
                <Field label={t("account.currentPassword")}>
                  <Input
                    name="current_password"
                    type="password"
                    required
                    maxLength={256}
                    autoComplete="current-password"
                  />
                </Field>
              </>
            )}
          </SubmitForm>
        )}
      </Modal>
    </div>
  );
}
