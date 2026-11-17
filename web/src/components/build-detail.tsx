/**
 * [INPUT]: 依赖构建 API、事件订阅和共享控件。
 * [OUTPUT]: 提供 BuildDetail 构建详情。
 * [POS]: 活跃构建订阅事件；终态展示静态分页日志。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { subscribeBuild } from "@/lib/events";

import { useUI } from "@/components/providers";

import { Link } from "@/lib/tenant";
import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  Clock3,
  GitBranch,
  GitCommitHorizontal,
  Pause,
  Play,
  RefreshCw,
  Server,
  ShieldCheck,
  Square,
  Terminal,
} from "lucide-react";
import {
  ArtifactTable,
  CopyButton,
  Empty,
  ErrorBox,
  Loading,
  PageTitle,
  SectionTitle,
  Status,
  StepIcon,
} from "@/components/common";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Modal } from "@/components/ui/dialog";
import { Tabs } from "@/components/ui/tabs";
import { SubmitForm } from "@/components/forms";
import { ImageBuilds } from "@/components/image-builds";
import { BuildPlanning } from "@/components/build-planning";
import { BuildLogHistory } from "@/components/build-log-history";
import { isBuildTerminal, type BuildLog } from "@/lib/build-state";
import { Field } from "@/components/common";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { Build, User } from "@/lib/types";
import { cn, elapsed, short } from "@/lib/utils";

export function BuildDetail({
  projectSlug,
  number,
  user,
  readOnly = false,
}: {
  projectSlug: string;
  number: number;
  user: User;
  readOnly?: boolean;
}) {
  const { t, date } = useUI();
  const remote = useRemote<Build>(
    `/projects/by-slug/${encodeURIComponent(projectSlug)}/builds/${number}`,
    data => isBuildTerminal(data?.status) && data?.publish_status === "pending" ? 3000 : false,
  );
  const build = remote.data;
  const id = build?.id;
  const role =
    user.role === "admin"
      ? "admin"
      : user.project_roles?.[build?.project_id || ""] || "viewer";
  const [logs, setLogs] = useState<BuildLog[]>([]);
  const [selected, setSelected] = useState<string>();
  const [filter, setFilter] = useState<string>();
  const [follow, setFollow] = useState(true);
  const [connected, setConnected] = useState(false);
  const [watching, setWatching] = useState<string>();
  const live = !!id && (!isBuildTerminal(build?.status) || watching === id);
  const [error, setError] = useState<Error>();
  const [publish, setPublish] = useState(false);
  const [tab, setTab] = useState("execution");
  const [clipped, setClipped] = useState(false);
  const terminal = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setLogs([]);
    setSelected(undefined);
    setFilter(undefined);
    setClipped(false);
  }, [id]);
  useEffect(() => {
    if (!id || !live) return;
    setWatching(id);
    setConnected(false);
    const controller = new AbortController();
    void subscribeBuild(
      id,
      controller.signal,
      (kind, data) => {
        if (kind === "snapshot" || kind === "complete") {
          remote.setData(JSON.parse(data));
          setConnected(true);
        }
        if (kind === "complete") {
          setWatching(undefined);
          setConnected(false);
        }
        if (kind === "log") {
          const log: BuildLog = JSON.parse(data);
          setLogs((previous) => {
            if (previous.some((entry) => entry.id === log.id)) return previous;
            if (previous.length >= 3000) setClipped(true);
            return [...previous, log].slice(-3000);
          });
        }
      },
      () => setConnected(false),
    );
    return () => controller.abort();
  }, [id, live, remote.setData]);
  useEffect(() => {
    if (tab === "execution" && follow && terminal.current)
      terminal.current.scrollTop = terminal.current.scrollHeight;
  }, [logs, follow, filter, selected, tab]);
  async function action(path: string, body: unknown = {}) {
    setError(undefined);
    try {
      await mutation(path, body);
      await remote.reload();
    } catch (error) {
      setError(asError(error));
    }
  }
  if (!build || !id)
    return (
      <>
        <ErrorBox message={remote.error} />
        <Loading />
      </>
    );
  const job = build.jobs?.find((job) => job.id === selected) || build.jobs?.[0];
  const attempt = job?.attempts.at(-1);
  const displayed = logs.filter(
    (log) =>
      (!attempt || log.attempt_id === attempt.id) &&
      (!filter || log.step === filter),
  );
  const active = [
    "resolving",
    "planning",
    "replanning",
    "needs_input",
    "queued",
    "running",
    "canceling",
  ].includes(build.status);
  return (
    <>
      <Link
        href={`/projects/${projectSlug}/builds`}
        className="mb-5 inline-flex items-center gap-2 text-xs text-muted-foreground hover:text-primary max-md:min-h-11"
      >
        <ArrowLeft size={13} />
        {t("builds.all")}
      </Link>
      {build.jobs?.some((job) => job.blocked_reason) && (
        <p className="notice" role="status">
          {build.jobs
            .filter((job) => job.blocked_reason)
            .map((job) => `${job.target}: ${job.blocked_reason}`)
            .join(" · ")}
        </p>
      )}
      <PageTitle
        eyebrow={t("builds.eyebrowId", { id: build.number })}
        title={build.project_name}
        description={t("builds.resultDescription", {
          date: date(build.created_at),
        })}
      >
        <Status value={build.status} />
        {role !== "viewer" && active && (
          <Button
            variant="destructive"
            onClick={() => void action(`/builds/${id}/cancel`)}
          >
            <Square />
            {t("builds.cancel")}
          </Button>
        )}
        {!readOnly &&
          role !== "viewer" &&
          build.status === "succeeded" &&
          build.publish_status !== "succeeded" && (
            <Button onClick={() => setPublish(true)}>
              <ShieldCheck />
              {t("builds.publish")}
            </Button>
          )}
      </PageTitle>
      <ErrorBox message={error || remote.error || build.error} />
      <div className="build-meta">
        <span className="meta-item">
          <GitBranch size={14} />
          {build.requested_ref}
        </span>
        <span className="meta-item">
          <GitCommitHorizontal size={14} />
          {build.commit_sha || t("builds.resolving")}
        </span>
        <span className="meta-item">
          <Clock3 size={14} />
          {elapsed(build.created_at, build.finished_at)}
        </span>
        <span className="meta-item">
          {t("builds.publication")}{" "}
          <Status small value={build.publish_status} />
        </span>
        {build.release_version && (
          <span className="meta-item text-primary">
            {build.release_version}
          </span>
        )}
      </div>
      {build.publish_error && <ErrorBox message={t("builds.publishError", { detail: build.publish_error })} />}
      {build.status === "needs_input" && <div className="notice build-attention" role="status">
        <span>{t("redesign.needsInput")}</span><Button size="sm" variant="secondary" onClick={() => setTab("plan")}>{t("redesign.viewPlan")}</Button>
      </div>}
      <Tabs value={tab} onChange={setTab} label={t("redesign.buildTabs")} items={[
        { value: "execution", label: t("redesign.execution") }, { value: "plan", label: t("redesign.plan") }, { value: "artifacts", label: t("redesign.output") },
      ]}>{activeTab => <>
        <div hidden={activeTab !== "execution"}>
          <div className="build-layout">
            <div className="min-w-0 space-y-4">
        <section className="panel self-start">
          <SectionTitle
            title={t("builds.targets")}
            detail={t("common.targetCount", { count: build.jobs?.length || 0 })}
          />
          <div className="build-targets">
            {build.jobs?.length ? (
              build.jobs.map((target) => (
                <button
                  key={target.id}
                  className={cn(
                    "target-button",
                    job?.id === target.id && "selected",
                  )}
                  onClick={() => {
                    setSelected(target.id);
                    setFilter(undefined);
                  }}
                >
                  <StepIcon status={target.status} />
                  <div className="min-w-0 flex-1">
                    <div className="font-mono text-xs">{target.target}</div>
                    <div className="mt-1.5 font-mono text-[10px] text-muted-foreground">
                      {target.target_config.os} / {target.target_config.arch}
                    </div>
                  </div>
                  <Status small value={target.status} />
                </button>
              ))
            ) : (
              <div className="p-5 text-xs leading-6 text-muted-foreground">
                {build.status === "failed"
                  ? t("builds.configFailed")
                  : t("builds.readingConfig")}
              </div>
            )}
          </div>
          <div className="border-t border-border p-4 text-[11px] leading-6 text-muted-foreground">
            {attempt ? (
              <>
                <div className="flex items-center gap-2">
                  <Server size={13} />
                  {attempt.node_name}
                </div>
                <div className="mt-2 font-mono">
                  attempt {short(attempt.id)}
                </div>
                <div>
                  {t("builds.lease")} {date(attempt.lease_expires_at)}
                </div>
              </>
            ) : (
              t("builds.matching")
            )}
          </div>
        </section>
          <section className="panel">
            <SectionTitle
              title={t("builds.steps")}
              detail={t("builds.pipeline")}
            />
            {job?.target_config.steps.map((step, index) => {
              const state = attempt?.steps.find(
                (entry) => entry.step_index === index,
              );
              return (
                <button
                  key={index}
                  className={cn(
                    "flex w-full items-center gap-3 border-b border-border px-5 py-3.5 text-left last:border-0 hover:bg-secondary",
                    filter === step.name && "bg-secondary",
                  )}
                  onClick={() =>
                    setFilter(filter === step.name ? undefined : step.name)
                  }
                >
                  <StepIcon status={state?.status} />
                  <span className="min-w-0 flex-1 text-xs">{step.name}</span>
                  <span className="tag">{step.phase}</span>
                  <span className="w-16 text-right font-mono text-[10px] text-muted-foreground">
                    {state ? elapsed(state.started_at, state.finished_at) : "—"}
                  </span>
                </button>
              );
            })}
            {job?.placement && (
              <div className="space-y-2 border-t border-border p-4 text-xs text-muted-foreground">
                <p>
                  {t("builds.placement")} ·{" "}
                  {job.placement.selected_reason ||
                    job.placement.reason ||
                    t("builds.placementQueued")}
                </p>
                <ol className="list-inside list-decimal space-y-1">
                  {job.placement.placement.candidates.map((candidate) => (
                    <li key={candidate.node_id} className="break-all">
                      <code>{short(candidate.node_id)}</code> ·{" "}
                      {candidate.reason}
                    </li>
                  ))}
                </ol>
              </div>
            )}
            {attempt?.error && (
              <div className="p-4">
                <ErrorBox message={attempt.error} />
              </div>
            )}
            {job &&
              ["failed", "lost", "canceled"].includes(job.status) &&
              !["planning", "replanning", "needs_input"].includes(
                build.status,
              ) &&
              !readOnly &&
              role !== "viewer" && (
                <div className="p-4">
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => void action(`/jobs/${job.id}/retry`)}
                  >
                    <RefreshCw />
                    {t("builds.retry")}
                  </Button>
                </div>
              )}
          </section>
            </div>
          {!live ? <BuildLogHistory key={`${id}/${attempt?.id || ""}/${filter || ""}`} buildId={id} attemptId={attempt?.id} step={filter} /> : <section className="panel">
            <SectionTitle
              live
              title={
                filter
                  ? t("builds.filteredLogs", { step: filter })
                  : t("builds.logs")
              }
              detail={
                connected ? t("builds.connected") : t("builds.reconnecting")
              }
            >
              <div className="flex items-center gap-1">
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={
                    follow ? t("builds.pauseFollow") : t("builds.follow")
                  }
                  onClick={() => setFollow(!follow)}
                >
                  {follow ? <Pause /> : <Play />}
                </Button>
                <CopyButton value={displayed.map((log) => log.text).join("")} />
              </div>
            </SectionTitle>
            {clipped && (
              <div className="border-b border-border px-5 py-2 text-[10px] text-primary">
                {t("builds.clipped")}
              </div>
            )}
            <div
              className="log-console"
              ref={terminal}
              role="log"
              aria-label={t("builds.logLabel")}
              aria-live="off"
            >
              {displayed.length ? (
                displayed.map((log) => (
                  <span key={log.id}>
                    {!filter && (
                      <span className="mr-3 select-none text-log-muted">
                        [{log.step}]
                      </span>
                    )}
                    {log.text}
                  </span>
                ))
              ) : (
                <span className="flex items-center gap-2 text-muted-foreground">
                  <Terminal size={13} />
                  {active ? t("builds.waitingLogs") : t("builds.noLogs")}
                </span>
              )}
            </div>
          </section>}
          </div>
        </div>
        <div hidden={activeTab !== "plan"}>
          {build.goal ? <BuildPlanning build={build} user={{ ...user, role }} refresh={remote.reload} />
            : <Empty title={t("redesign.plan")} detail={t("builds.noConfig")} />}
      <details className="panel mt-5">
        <summary className="cursor-pointer px-5 py-4 text-xs text-muted-foreground">
          {t("builds.snapshot")}
        </summary>
        <pre className="log-console min-h-0">
          {build.config_yaml ||
            (build.config_snapshot
              ? JSON.stringify(build.config_snapshot, null, 2)
              : t("builds.noConfig"))}
        </pre>
      </details>
        </div>
      <section className="panel" hidden={activeTab !== "artifacts"}>
        <SectionTitle
          title={t("builds.verified")}
          detail={t("artifacts.shaVerified")}
        />
        {!!build.image_executions?.length && <ImageBuilds executions={build.image_executions} projectId={build.project_id} registry={build.image_registry} editable={role !== "viewer" && !readOnly} reload={remote.reload} />}
        {build.artifacts?.length ? (
          <ArtifactTable artifacts={build.artifacts} />
        ) : !build.image_executions?.length ? (
          <Empty
            title={t("builds.noArtifacts")}
            detail={t("builds.noArtifactsDetail")}
          />
        ) : null}
      </section>
      </>}</Tabs>

      <Modal
        open={publish}
        onOpenChange={setPublish}
        title={t("builds.publishTitle")}
        description={t("builds.publishDetail")}
      >
        <SubmitForm
          label={t("builds.createRelease")}
          onSubmit={async (form) => {
            await mutation(`/builds/${id}/publish`, {
              version: String(form.get("version")),
            });
            setPublish(false);
            await remote.reload();
          }}
        >
          <Field label={t("builds.version")}>
            <Input
              name="version"
              defaultValue={build.release_version || ""}
              readOnly={!!build.release_version}
              required
              maxLength={100}
              placeholder="v1.0.0"
            />
          </Field>
        </SubmitForm>
      </Modal>
    </>
  );
}
