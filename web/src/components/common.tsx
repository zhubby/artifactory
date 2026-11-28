/**
 * [INPUT]: 依赖租户下载 API、全局通知和带验签证据的制品类型。
 * [OUTPUT]: 提供内容标题、页面动作、骨架和可键盘滚动的制品验签表。
 * [POS]: 业务页面复用的展示与反馈边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { api } from "@/lib/api";

import { useUI } from "@/components/providers";

import {
  CheckCircle2,
  Circle,
  CircleDashed,
  Clock3,
  Copy,
  Download,
  LoaderCircle,
  XCircle,
} from "lucide-react";
import { useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { Artifact, ApplicationPlatform, TargetPlatform } from "@/lib/types";
import { bytes, short } from "@/lib/utils";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import { ErrorNotification } from "@/components/notifications";

export function TargetPlatformLabel({ target }: { target: TargetPlatform }) {
  const { t } = useUI();
  const platforms: Record<ApplicationPlatform, string> = {
    macos: "macOS", ios: "iOS", android: "Android", linux: "Linux", windows: "Windows",
  };
  const application = target.application_platform ? platforms[target.application_platform] : t("builds.nonApplication");
  return <span>{application} · {t("builds.hostPlatform", { os: target.os, arch: target.arch })}</span>;
}

export function Status({
  value,
  small = false,
}: {
  value: string;
  small?: boolean;
}) {
  const { t } = useUI();
  const labels: Record<string, string> = {
    active: t("status.active"),
    draft: t("status.draft"),
    publishing: t("status.publishing"),
    published: t("status.published"),
    archived: t("status.archived"),
    deleting: t("status.deleting"),
    waiting: t("status.waiting"),
    resolving: t("status.resolving"),
    planning: t("status.planning"),
    replanning: t("status.replanning"),
    interrupted: t("status.interrupted"),
    needs_input: t("status.needs_input"),
    queued: t("status.queued"),
    leased: t("status.leased"),
    running: t("status.running"),
    preparing: t("status.preparing"),
    publish_failed: t("status.publish_failed"),
    succeeded: t("status.succeeded"),
    failed: t("status.failed"),
    canceled: t("status.canceled"),
    canceling: t("status.canceling"),
    lost: t("status.lost"),
    pending: t("status.pending"),
    not_requested: t("status.not_requested"),
    online: t("status.online"),
    offline: t("status.offline"),
    draining: t("status.draining"),
    revoked: t("status.revoked"),
  };
  const Icon = ["running", "resolving", "canceling", "preparing", "publishing"].includes(value)
    ? LoaderCircle
    : ["succeeded", "online", "published"].includes(value)
      ? CheckCircle2
      : ["failed", "lost", "revoked", "publish_failed"].includes(value)
        ? XCircle
        : ["queued", "leased", "pending"].includes(value)
          ? Clock3
          : CircleDashed;
  return (
    <span className={cn("status", small && "status-small", `status-${value}`)}>
      <Icon
        className={cn(
          "size-3.5",
          ["running", "resolving"].includes(value) &&
            "motion-safe:animate-spin",
        )}
      />
      {labels[value] || value}
    </span>
  );
}

export function Empty({
  title,
  detail,
  action,
}: {
  title: string;
  detail: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="mb-4 flex size-11 items-center justify-center rounded-xl border border-border bg-secondary">
        <CircleDashed size={22} />
      </div>
      <h3 className="font-medium text-foreground">{title}</h3>
      <p className="mt-2 max-w-md text-sm leading-6">{detail}</p>
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}
export function Loading({
  variant = "list",
}: {
  variant?: "list" | "cards" | "table" | "detail" | "form" | "log";
}) {
  const { t } = useUI();
  return (
    <div
      role="status"
      aria-busy="true"
      className={`loading-state skeleton-${variant}`}
    >
      <span className="sr-only">{t("common.loading")}</span>
      <div className="skeleton-layout" aria-hidden="true">
        {Array.from({ length: variant === "cards" ? 2 : variant === "log" ? 7 : 3 }, (_, index) => (
          <div className="skeleton-row" key={index}>
            {(variant === "cards" || variant === "list") && <span className="skeleton-block skeleton-icon" />}
            <div className="skeleton-copy">
              <span className="skeleton-block skeleton-title" />
              {variant !== "log" && <span className="skeleton-block skeleton-caption" />}
            </div>
            {(variant === "table" || variant === "detail" || variant === "cards") && <span className="skeleton-block skeleton-value" />}
          </div>
        ))}
      </div>
    </div>
  );
}
export function ErrorBox({ message }: { message?: string | Error | null }) {
  return <ErrorNotification message={message} />;
}
export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <label className="grid gap-2 text-sm">
      <span className="font-medium">{label}</span>
      {children}
      {hint && (
        <span className="text-xs leading-5 text-muted-foreground">{hint}</span>
      )}
    </label>
  );
}
export function PageTitle({
  eyebrow,
  title,
  description,
  titleAccessory,
  children,
}: {
  eyebrow: string;
  title: string;
  description: string;
  titleAccessory?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="page-title">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <div className="page-title-heading"><h1>{title}</h1>{titleAccessory}</div>
        <p className="mt-2 text-sm leading-6 text-muted-foreground">
          {description}
        </p>
      </div>
      {children && <div className="page-actions">{children}</div>}
    </div>
  );
}
export function SectionTitle({
  title,
  detail,
  live = false,
  children,
}: {
  title: string;
  detail?: string;
  live?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className="section-title">
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h2>{title}</h2>
        {detail && (
          <span
            className={cn(
              "text-xs text-muted-foreground",
              !live && "hidden sm:block",
            )}
          >
            {detail}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}
export function CopyButton({ value }: { value: string }) {
  const { t } = useUI();
  return (
    <Button
      variant="ghost"
      size="icon"
      aria-label={t("common.copy")}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          notify.success(t("common.copied"));
        } catch (error) {
          notify.error(asError(error));
        }
      }}
    >
      <Copy />
    </Button>
  );
}
export function ArtifactTable({ artifacts }: { artifacts: Artifact[] }) {
  const { t } = useUI();
  const [error, setError] = useState<Error>();
  if (!artifacts.length) return <Empty title={t("builds.noArtifacts")} detail={t("builds.noArtifactsDetail")} />;
  return (
    <div>
      <ErrorBox message={error} />
      <div className="table-scroll delivery-artifact-table" tabIndex={0} role="region" aria-label={t("artifacts.files")}>
        <table>
          <thead>
            <tr>
              <th>{t("artifacts.target")}</th>
              <th>{t("common.size")}</th>
              <th>SHA-256</th>
              <th>{t("signing.status")}</th>
              <th className="text-right">{t("common.download")}</th>
            </tr>
          </thead>
          <tbody>
            {artifacts.map((artifact) => {
              const filename = (artifact.delivery_filename || artifact.path).split(/[\\/]/).pop() || "—";
              return <tr key={artifact.id}>
                <td>
                  <div className="font-medium break-all">{filename}</div>
                  <div className="mt-1 font-mono text-xs text-muted-foreground">
                    {artifact.target}{artifact.release_version ? ` · ${artifact.release_version}` : ""}
                  </div>
                </td>
                <td className="font-mono text-xs">{bytes(artifact.size)}</td>
                <td>
                  <span
                    title={artifact.sha256}
                    className="font-mono text-xs text-muted-foreground"
                  >
                    {short(artifact.sha256)}…{artifact.sha256.slice(-8)}
                  </span>
                </td>
                <td>
                  {artifact.signing ? (
                    <div className="space-y-1 text-xs">
                      <strong>{t("signing.verified")}</strong>
                      <p>{artifact.signing.profile}</p>
                      <p
                        className="break-all font-mono"
                        title={artifact.signing.fingerprint_sha256}
                      >
                        {artifact.signing.fingerprint_sha256.slice(0, 16)}…
                      </p>
                      <p>
                        {t(
                          artifact.signing.notarized
                            ? "signing.notarized"
                            : "signing.notarizationNotRequired",
                        )}
                      </p>
                    </div>
                  ) : (
                    <span className="text-xs text-muted-foreground">
                      {t("signing.noEvidence")}
                    </span>
                  )}
                </td>
                <td className="text-right">
                  <Button variant="ghost" size="icon" asChild>
                    <a
                      href="#"
                      onClick={(event) => {
                        event.preventDefault();
                        setError(undefined);
                        void api<{ url: string }>(
                          `/artifacts/${artifact.id}/download`,
                        )
                          .then((result) => {
                            notify.success(t("common.downloadStarted"));
                            window.location.assign(result.url);
                          })
                          .catch((error) => setError(asError(error)));
                      }}
                      aria-label={t("artifacts.download", {
                        path: filename,
                      })}
                    >
                      <Download />
                    </a>
                  </Button>
                </td>
              </tr>;
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
export function StepIcon({ status }: { status?: string }) {
  return status === "succeeded" ? (
    <CheckCircle2 className="size-4 text-success" />
  ) : status === "failed" ? (
    <XCircle className="size-4 text-destructive" />
  ) : status === "running" ? (
    <LoaderCircle className="size-4 animate-spin text-primary" />
  ) : (
    <Circle className="size-4 text-muted-foreground" />
  );
}
