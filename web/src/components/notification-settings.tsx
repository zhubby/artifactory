/**
 * [INPUT]: 依赖账户偏好、邮箱验证、项目订阅和统一表单保护。
 * [OUTPUT]: 提供邮件主动订阅、邮箱确认和项目关注控件。
 * [POS]: 账户通知配置边界；只提交可编辑渠道，读取不启用外发。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useState, useEffect } from "react";
import { Bell, BellRing } from "lucide-react";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { Link } from "@/lib/tenant";
import { notify } from "@/lib/notify";
import {
  notificationCategories,
  type NotificationPreferences,
} from "@/lib/notification-types";
import { useNotificationCopy } from "@/lib/notification-copy";

export function NotificationSettings() {
  const c = useNotificationCopy();
  const { t } = useUI();
  const preferences = useRemote<NotificationPreferences>(
    "/notifications/preferences",
    false,
  );
  const value = preferences.data;
  const [error, setError] = useState<Error>();
  const [busy, setBusy] = useState(false);
  async function emailAction(path: string, method = "POST") {
    setBusy(true);
    setError(undefined);
    try {
      await mutation(path, {}, method);
      await preferences.reload({ progress: "background" });
      notify.success(t("common.operationSucceeded"));
    } catch (e) {
      setError(asError(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="notification-settings">
      <ErrorBox message={preferences.error || error} />
      {!value && !preferences.error ? (
        <Loading variant="form" />
      ) : (
        value && (
          <>
            <section>
              <h2>{c("email")}</h2>
              <p className="management-description">
                {value.email_available ? c("emailHint") : c("unavailable")}
              </p>
              {value.email_address && (
                <div className="notification-email-status">
                  <span>{value.email_address}</span>
                  <strong>
                    {value.email_verified ? c("verified") : c("pending")}
                  </strong>
                </div>
              )}
              <fieldset disabled={!value.email_available}>
                <SubmitForm
                  key={value.email_address || "no-email"}
                  label={c("saveEmail")}
                  onSubmit={async (form) => {
                    await mutation(
                      "/notifications/email",
                      { address: form.get("address") },
                      "PUT",
                    );
                    await preferences.reload({ progress: "background" });
                  }}
                >
                  <Field label={c("address")}>
                    <Input
                      name="address"
                      type="email"
                      maxLength={254}
                      required
                      defaultValue={value.email_address || ""}
                      disabled={!value.email_available}
                    />
                  </Field>
                </SubmitForm>
              </fieldset>
              {value.email_address && (
                <div className="notification-email-actions">
                  {!value.email_verified && (
                    <Button
                      variant="secondary"
                      disabled={busy || !value.email_available}
                      onClick={() =>
                        void emailAction("/notifications/email/resend")
                      }
                    >
                      {c("resend")}
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    disabled={busy}
                    onClick={() =>
                      void emailAction("/notifications/email", "DELETE")
                    }
                  >
                    {c("remove")}
                  </Button>
                </div>
              )}
            </section>
            <section>
              <h2>{c("settings")}</h2>
              <SubmitForm
                onSubmit={async (form) => {
                  const in_app = Object.fromEntries(
                    notificationCategories.map((key) => [
                      key,
                      form.get(`in_app.${key}`) === "on",
                    ]),
                  );
                  const email = Object.fromEntries(
                    notificationCategories.map((key) => [
                      key,
                      form.get(`email.${key}`) === "on",
                    ]),
                  );
                  await mutation(
                    "/notifications/preferences",
                    {
                      in_app,
                      ...(value.email_available && value.email_verified ? { email } : {}),
                      locale: form.get("locale"),
                    },
                    "PUT",
                  );
                  await preferences.reload({ progress: "background" });
                }}
              >
                <div className="notification-preference-table">
                  <div className="notification-preference-row notification-preference-heading">
                    <span>{c("category")}</span>
                    <span>{c("inApp")}</span>
                    <span>{c("emailChannel")}</span>
                  </div>
                  {notificationCategories.map((category) => (
                    <div className="notification-preference-row" key={category}>
                      <span>{c(category)}</span>
                      <input
                        aria-label={`${c(category)} · ${c("inApp")}`}
                        type="checkbox"
                        name={`in_app.${category}`}
                        defaultChecked={value.in_app[category] !== false}
                      />
                      {category === "delivery" ? (
                        <span>{c("inAppOnly")}</span>
                      ) : <input
                        key={`${value.email_address}/${value.email_verified}`}
                        aria-label={`${c(category)} · ${c("emailChannel")}`}
                        type="checkbox"
                        name={`email.${category}`}
                        disabled={
                          !value.email_available || !value.email_verified
                        }
                        defaultChecked={value.email[category] === true}
                      />}
                    </div>
                  ))}
                </div>
                <Field label={c("language")}>
                  <Select name="locale" defaultValue={value.locale}>
                    <option value="zh-CN">简体中文</option>
                    <option value="en">English</option>
                  </Select>
                </Field>
              </SubmitForm>
            </section>
          </>
        )
      )}
    </div>
  );
}
export function ProjectNotificationFollow({
  projectId,
}: {
  projectId: string;
}) {
  const c = useNotificationCopy();
  const { t } = useUI();
  const subscription = useRemote<{ following: boolean }>(
    `/projects/${projectId}/notification-subscription`,
    false,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Error>();
  return (
    <>
      <ErrorBox message={error || subscription.error} />
      <Button
        variant="secondary"
        disabled={busy || !subscription.data}
        title={c("followHint")}
        aria-pressed={subscription.data?.following || false}
        onClick={async () => {
          setBusy(true);
          setError(undefined);
          try {
            const next = await mutation<{ following: boolean }>(
              `/projects/${projectId}/notification-subscription`,
              { following: !subscription.data?.following },
              "PUT",
            );
            subscription.setData(next);
            notify.success(t("common.operationSucceeded"));
          } catch (e) {
            setError(asError(e));
          } finally {
            setBusy(false);
          }
        }}
      >
        {subscription.data?.following ? (
          <BellRing size={15} />
        ) : (
          <Bell size={15} />
        )}{" "}
        {subscription.data?.following ? c("following") : c("follow")}
      </Button>
    </>
  );
}
export function NotificationEmailVerify({
  unsubscribe = false,
}: {
  unsubscribe?: boolean;
}) {
  const c = useNotificationCopy();
  const [token, setToken] = useState("");
  const [done, setDone] = useState(false);
  useEffect(() => {
    setToken(new URLSearchParams(window.location.search).get("token") || "");
  }, []);
  return (
    <main className="notification-verify">
      <p className="eyebrow">ARTIFACTORY ACCOUNT</p>
      <h1>{unsubscribe ? c("unsubscribeTitle") : c("verifyTitle")}</h1>
      {done ? (
        <p role="status">{unsubscribe ? c("unsubscribed") : c("verified")}</p>
      ) : (
        <>
          <p>{unsubscribe ? c("unsubscribeHint") : c("verifyHint")}</p>
          <SubmitForm
            label={unsubscribe ? c("unsubscribe") : c("verify")}
            onSubmit={async () => {
              if (!token) throw new Error(c("invalidToken"));
              await mutation(
                unsubscribe
                  ? "/notifications/email/unsubscribe"
                  : "/notifications/email/verify",
                { token },
              );
              setToken("");
              history.replaceState(null, "", location.pathname);
              setDone(true);
            }}
          >
            {null}
          </SubmitForm>
        </>
      )}
      <Link href="/notifications">{c("back")}</Link>
    </main>
  );
}
