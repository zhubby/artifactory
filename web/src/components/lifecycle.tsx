/**
 * [INPUT]: 依赖生命周期 API、共享幂等键、统一表单和通知。
 * [OUTPUT]: 提供归档、恢复、删除及当前操作的撤销、重试入口。
 * [POS]: 影响与当前操作首读就绪后开放动作；轮询保留已展示内容。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState } from "react";
import {
  Archive,
  RotateCcw,
  Trash2,
  RefreshCw,
  X,
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
import { notify } from "@/lib/notify";
import { createIdempotencyKey } from "@/lib/idempotency";

function latestOperation(rows: LifecycleOperation[] | undefined, tenant: string, project?: string) {
  return rows?.filter(op => op.tenant_id === tenant && op.project_id === (project ?? null))
    .reduce<LifecycleOperation | undefined>((latest, op) =>
      !latest || op.created_at > latest.created_at ? op : latest, undefined);
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
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  const [recoveryError, setRecoveryError] = useState<Error>();
  const operations = useRemote<LifecycleOperation[]>(allowed ? "/operations" : null, rows => {
    const current = latestOperation(rows, tenant, project);
    return current && ["waiting", "running"].includes(current.state) ? 2000 : false;
  });
  const current = latestOperation(operations.data, tenant, project);
  const impact = useRemote<Record<string, number>>(
    allowed ? `${path}/lifecycle` : null,
    10000,
  );
  const actionsReady = !!impact.data && !!operations.data;
  function begin(action: "archive" | "restore" | "delete") {
    setKey(createIdempotencyKey());
    setAction(action);
  }
  async function recover(verb: "cancel" | "retry") {
    if (!allowed || !current || recoveryBusy) return;
    setRecoveryBusy(true);
    setRecoveryError(undefined);
    try {
      const updated = await mutation<LifecycleOperation>(`/operations/${current.id}/${verb}`, {});
      operations.setData(rows => [updated, ...(rows || []).filter(op => op.id !== updated.id)]);
      await operations.reload();
      onChange();
      notify.success(t(verb === "retry" ? "lifecycle.requestSubmitted" : "common.operationSucceeded"));
    } catch (error) {
      setRecoveryError(asError(error));
    } finally {
      setRecoveryBusy(false);
    }
  }
  return (
    <div className="management-stack">
      <section className="panel management-section">
        <h2>{t("lifecycle.title")}</h2>
        <p className="management-description">{t("lifecycle.description")}</p>
        <Status value={status} />
        <ErrorBox message={impact.error || operations.error || recoveryError} />
        <ErrorBox message={current?.state === "failed" ? current.error || t("common.operationFailed") : undefined} />
        {allowed && !impact.data && !impact.error && <Loading variant="form" />}
        {allowed && !operations.data && !operations.error && <Loading variant="detail" />}
        {impact.data && <div className="lifecycle-counts">
          {Object.entries(impact.data).map(([key, value]) => (
            <div key={key}>
              <strong>{value}</strong>
              <span>{t(`lifecycle.count_${key}`)}</span>
            </div>
          ))}
        </div>}
        {allowed && ((impact.error && !impact.data) || (operations.error && !operations.data)) && <Button variant="secondary" className="mb-4" onClick={() => {
          void impact.reload();
          void operations.reload();
        }}>{t("common.retry")}</Button>}
        {allowed ? actionsReady && (
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
            {current?.state === "waiting" && <Button variant="secondary" disabled={recoveryBusy} onClick={() => void recover("cancel")}>
              <X />
              {t("lifecycle.cancel")}
            </Button>}
            {current?.state === "failed" && <Button variant="secondary" disabled={recoveryBusy} onClick={() => void recover("retry")}>
              <RefreshCw />
              {t("common.retry")}
            </Button>}
          </div>
        ) : (
          <p className="notice mt-5">{t("lifecycle.ownerRequired")}</p>
        )}
      </section>
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
            successMessage={t("lifecycle.requestSubmitted")}
            onSubmit={async (form) => {
              const submitted = await mutation<LifecycleOperation>(`${path}/lifecycle`, {
                action,
                idempotency_key: key,
                ...(action === "delete"
                  ? {
                      slug: form.get("slug"),
                      current_password: form.get("current_password"),
                    }
                  : {}),
              });
              operations.setData(rows => [submitted, ...(rows || []).filter(op => op.id !== submitted.id)]);
              setAction(undefined);
              onChange();
              void operations.reload();
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
