/**
 * [INPUT]: 依赖租户或项目 Webhook API、统一表单与安全投递摘要。
 * [OUTPUT]: 提供管理员端点配置、密钥轮换、测试和失败重试。
 * [POS]: 设置内的外部通知边界；新端点默认关闭且密钥仅短暂显示。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { useNotificationCopy } from "@/lib/notification-copy";
import type {
  WebhookEndpoint,
  WebhookDelivery,
} from "@/lib/notification-types";

export function NotificationWebhooks({ projectId }: { projectId?: string }) {
  const c = useNotificationCopy();
  const path = projectId
    ? `/projects/${projectId}/notification-webhooks`
    : "/tenant/notification-webhooks";
  const endpoints = useRemote<WebhookEndpoint[]>(path, false);
  const [secret, setSecret] = useState("");
  useEffect(() => {
    setSecret("");
  }, [path]);
  return (
    <section className="notification-webhooks">
      <h2>{c("webhooks")}</h2>
      <p className="management-description">{c("webhookHint")}</p>
      <ErrorBox message={endpoints.error} />
      {secret && (
        <aside className="notification-secret">
          <p>{c("secret")}</p>
          <code>{secret}</code>
          <Button variant="secondary" onClick={() => setSecret("")}>
            {c("close")}
          </Button>
        </aside>
      )}
      {!endpoints.data && !endpoints.error ? (
        <Loading variant="list" />
      ) : endpoints.data?.length === 0 ? (
        <p className="notice">{c("noEndpoints")}</p>
      ) : (
        endpoints.data?.map((endpoint) => (
          <WebhookRow
            key={endpoint.id}
            endpoint={endpoint}
            path={path}
            reveal={setSecret}
            reload={() => endpoints.reload({ progress: "background" })}
          />
        ))
      )}
      <details className="notification-webhook-create">
        <summary>{c("create")}</summary>
        <SubmitForm
          label={c("create")}
          onSubmit={async (form, markSaved) => {
            const result = await mutation<{ secret: string }>(path, {
              name: form.get("name"),
              url: form.get("url"),
              enabled: form.get("enabled") === "on",
              event_types: String(form.get("event_types"))
                .split(",")
                .map((v) => v.trim())
                .filter(Boolean),
            });
            setSecret(result.secret);
            markSaved();
            await endpoints.reload({ progress: "background" });
          }}
        >
          <WebhookFields />
        </SubmitForm>
      </details>
    </section>
  );
}
function WebhookFields({ endpoint }: { endpoint?: WebhookEndpoint }) {
  const c = useNotificationCopy();
  return (
    <>
      <Field label={c("name")}>
        <Input
          required
          name="name"
          maxLength={100}
          defaultValue={endpoint?.name || ""}
        />
      </Field>
      <Field label={c("url")}>
        <Input
          required={!endpoint}
          type="url"
          name="url"
          placeholder={endpoint?.url_display || "https://"}
        />
      </Field>
      <Field label={c("eventTypes")}>
        <Input
          required
          name="event_types"
          defaultValue={
            endpoint?.event_types.join(", ") ||
            "build.status.changed, release.publication.changed"
          }
        />
      </Field>
      <label className="notification-check">
        <input
          name="enabled"
          type="checkbox"
          defaultChecked={endpoint?.enabled || false}
        />
        {c("enabled")}
      </label>
    </>
  );
}
function WebhookRow({
  endpoint,
  path,
  reveal,
  reload,
}: {
  endpoint: WebhookEndpoint;
  path: string;
  reveal: (value: string) => void;
  reload: () => Promise<void>;
}) {
  const c = useNotificationCopy();
  const { t, date } = useUI();
  const base = `${path}/${endpoint.id}`;
  const [history, setHistory] = useState(false);
  const deliveries = useRemote<WebhookDelivery[]>(
    history ? `${base}/deliveries` : null,
    false,
  );
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  async function action(suffix: string, method = "POST") {
    setBusy(true);
    setError(undefined);
    try {
      const result = await mutation<{ secret?: string }>(
        `${base}${suffix}`,
        {},
        method,
      );
      if (result?.secret) reveal(result.secret);
      await reload();
      if (history) await deliveries.reload({ progress: "background" });
      notify.success(t("common.operationSucceeded"));
    } catch (e) {
      setError(asError(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <article className="notification-webhook-row">
      <header>
        <div>
          <h3>{endpoint.name}</h3>
          <p>{endpoint.url_display}</p>
          <small>
            {endpoint.enabled ? c("enabled") : c("disabled")} ·{" "}
            {endpoint.last_status || "—"} · v{endpoint.generation}
          </small>
        </div>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => {
            if (window.confirm(c("deleteConfirm"))) void action("", "DELETE");
          }}
        >
          {c("delete")}
        </Button>
      </header>
      <ErrorBox message={error || deliveries.error} />
      <details>
        <summary>{c("settings")}</summary>
        <SubmitForm
          onSubmit={async (form) => {
            const url = String(form.get("url") || "").trim();
            await mutation(
              base,
              {
                name: form.get("name"),
                ...(url ? { url } : {}),
                enabled: form.get("enabled") === "on",
                event_types: String(form.get("event_types"))
                  .split(",")
                  .map((v) => v.trim())
                  .filter(Boolean),
              },
              "PATCH",
            );
            await reload();
          }}
        >
          <WebhookFields endpoint={endpoint} />
        </SubmitForm>
      </details>
      <div className="notification-webhook-actions">
        <Button
          variant="secondary"
          disabled={busy || !endpoint.enabled}
          onClick={() => void action("/test")}
        >
          {c("test")}
        </Button>
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => void action("/rotate")}
        >
          {c("rotate")}
        </Button>
        <Button
          variant="ghost"
          onClick={() => setHistory((v) => !v)}
          aria-expanded={history}
        >
          {c("deliveries")}
        </Button>
      </div>
      {history &&
        (!deliveries.data && !deliveries.error ? (
          <Loading variant="list" />
        ) : (
          <ul className="notification-deliveries">
            {deliveries.data?.map((delivery) => (
              <li key={delivery.id}>
                <div>
                  <strong>{delivery.event_type}</strong>
                  <p>
                    {delivery.status} · {delivery.attempts} ·{" "}
                    {date(delivery.created_at)}
                  </p>
                  {delivery.last_error && <p>{delivery.last_error}</p>}
                  {delivery.next_attempt_at && (
                    <small>{date(delivery.next_attempt_at)}</small>
                  )}
                </div>
                {["failed", "dead", "exhausted"].includes(delivery.status) && (
                  <Button
                    variant="secondary"
                    disabled={busy}
                    onClick={() =>
                      void action(`/deliveries/${delivery.id}/retry`)
                    }
                  >
                    {c("retry")}
                  </Button>
                )}
              </li>
            ))}
          </ul>
        ))}
    </article>
  );
}
