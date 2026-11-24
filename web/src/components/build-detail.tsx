/**
 * [INPUT]: 依赖 Release 归属、构建快照、规划工作台与日志事件。
 * [OUTPUT]: 提供过程总览、目标合同和保留内容的详情标签。
 * [POS]: Release 内的构建详情；首次展示过程，轮询保留用户选择。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { buildTargetSummary } from "@/lib/build-targets";
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
  Square,
  Terminal,
} from "lucide-react";
import {
  ArtifactTable,
  CopyButton,
  Empty,
  ErrorBox,
  Loading,
  SectionTitle,
  Status,
  StepIcon,
  TargetPlatformLabel,
} from "@/components/common";
import { Button } from "@/components/ui/button";
import { Tabs } from "@/components/ui/tabs";
import { ImageBuilds } from "@/components/image-builds";
import { BuildPlanning } from "@/components/build-planning";
import { BuildProcess } from "@/components/build-process";
import { BuildLogHistory } from "@/components/build-log-history";
import { isBuildTerminal, type BuildLog } from "@/lib/build-state";
import { mutation, useRemote } from "@/lib/api";
import { asError } from "@/lib/errors";
import { notify } from "@/lib/notify";
import type { Build, Release, User } from "@/lib/types";
import { releasePath } from "@/lib/routes";
import { cn, elapsed, short } from "@/lib/utils";

function initialTab() {
  return "process";
}

export function BuildDetail({
  projectSlug,
  number,
  user,
  readOnly = false,
  releaseId,
}: {
  projectSlug: string;
  number: number;
  user: User;
  readOnly?: boolean;
  releaseId?: string;
}) {
  const { t, date } = useUI();
  const remote = useRemote<Build>(
    `/projects/by-slug/${encodeURIComponent(projectSlug)}/builds/${number}`,
    data => isBuildTerminal(data?.status) && data?.publish_status === "pending" ? 3000 : false,
  );
  const build = remote.data;
  const id = build?.id;
  const release = useRemote<Release>(build?.release_id ? `/releases/${build.release_id}` : null);
  const canMutate = !readOnly && release.data?.status === "draft";
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
  const [tabSelection, setTabSelection] = useState<{ buildId: string; value: string }>();
  const tab = tabSelection && tabSelection.buildId === id ? tabSelection.value : initialTab();
  const activeTab = useRef(tab);
  activeTab.current = tab;
  function setTab(value: string) {
    if (id) setTabSelection({ buildId: id, value });
  }
  const [clipped, setClipped] = useState(false);
  const terminal = useRef<HTMLDivElement>(null);
  const pendingFollow = useRef(false);
  useEffect(() => {
    if (build) setTabSelection(previous => previous?.buildId === build.id
      ? previous : { buildId: build.id, value: initialTab() });
  }, [build]);
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
    pendingFollow.current = follow;
    if (activeTab.current === "execution" && follow && terminal.current) {
      terminal.current.scrollTop = terminal.current.scrollHeight;
      pendingFollow.current = false;
    }
  }, [logs, follow, filter, selected]);
  useEffect(() => {
    if (tab === "execution" && follow && pendingFollow.current && terminal.current) {
      terminal.current.scrollTop = terminal.current.scrollHeight;
      pendingFollow.current = false;
    }
  }, [tab, follow]);
  async function action(path: string, body: unknown = {}) {
    setError(undefined);
    try {
      await mutation(path, body);
      notify.success(t("common.operationSucceeded"));
      await remote.reload();
    } catch (error) {
      setError(asError(error));
    }
  }
  if (!build || !id)
    return (
      <>
        <ErrorBox key="build-feedback" message={remote.error} />
        {!remote.error && <Loading variant="detail" />}
      </>
    );
  if (releaseId && releaseId !== build.release_id)
    return <><ErrorBox key="build-feedback" message={t("common.notFound")} /></>;
  const job = build.jobs?.find((job) => job.id === selected) || build.jobs?.[0];
  const attempt = job?.attempts.at(-1);
  const imageJob = job?.target_config.executor === "buildkit";
  const imageExecutions = build.image_executions?.filter(execution => execution.job_id === job?.id && execution.operation === "build") || [];
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
  const canRetry = !!job && ["failed", "lost", "canceled"].includes(job.status)
    && !["planning", "replanning", "needs_input"].includes(build.status)
    && canMutate && role !== "viewer";
  const noJobsDetail = t(build.status === "failed" ? "builds.configFailed"
    : build.status === "needs_input" ? "redesign.needsInput"
    : active ? "builds.readingConfig" : "builds.noJobs");
  return (
    <div className="build-detail">
      <Link
        href={releasePath(projectSlug, build.release_id)}
        className="build-detail-back"
      >
        <ArrowLeft size={13} />
        {t("releaseFlow.returnRelease", {version:build.release_version})}
      </Link>
      {build.jobs?.some((job) => job.blocked_reason) && (
        <p className="notice" role="status">
          {build.jobs
            .filter((job) => job.blocked_reason)
            .map((job) => `${job.target}: ${job.blocked_reason}`)
            .join(" · ")}
        </p>
      )}
      <header className="build-detail-header">
        <div className="build-detail-heading">
          <h1>{t("buildDetail.title", {number: build.number})}</h1>
          <Link className="tag font-mono" href={releasePath(projectSlug, build.release_id)}>{build.release_version}</Link>
          <Status value={build.status} />
        </div>
        <div className="build-detail-actions">
        {!readOnly && role !== "viewer" && active && (
          <Button
            variant="destructive"
            onClick={() => void action(`/builds/${id}/cancel`)}
          >
            <Square />
            {t("builds.cancel")}
          </Button>
        )}
        </div>
      </header>
      <ErrorBox key="build-feedback" message={error || remote.error || release.error} />
      {build.error && tab !== "process" && <p className="mb-5 whitespace-pre-wrap break-words text-sm text-destructive" role="status" aria-atomic="true">{build.error}</p>}
      <div className="build-detail-meta">
        <span className="meta-item" title={t("buildDetail.project")}>{build.project_name}</span>
        <span className="meta-item">
          <GitBranch size={14} />
          {build.requested_ref}
        </span>
        <span className="meta-item" title={t("buildDetail.commit")}>
          <GitCommitHorizontal size={14} />
          <code title={build.commit_sha || undefined}>{build.commit_sha ? short(build.commit_sha) : t(isBuildTerminal(build.status) ? "process.commitNotPinned" : "builds.resolving")}</code>
          {build.commit_sha && <CopyButton value={build.commit_sha} />}
        </span>
        <span className="meta-item" title={t("buildDetail.duration")}>
          <Clock3 size={14} />
          {elapsed(build.created_at, build.finished_at)}
        </span>
        <span className="meta-item">{buildTargetSummary(build)}</span>
        <span className="meta-item" title={t("buildDetail.created")}>{date(build.created_at)}</span>
      </div>
      <details className="build-detail-contract"><summary>{t("buildDetail.targetContract")}<span>{t("common.targetCount", {count:build.target_requirements.length})}</span></summary>
        <div className="table-scroll"><table><thead><tr><th>{t("releaseFlow.targetKey")}</th><th>{t("releaseFlow.targetKind")}</th><th>{t("releaseFlow.platforms")}</th><th>{t("releaseFlow.deliveryName")}</th></tr></thead><tbody>{build.target_requirements.map(target => <tr key={target.key}><td className="font-mono">{target.key}</td><td>{t(target.kind === "native" ? "releaseFlow.native" : "releaseFlow.image")}</td><td>{target.kind === "image" ? target.platforms.join(" · ") : `${target.application_platform || t("builds.nonApplication")} · ${target.os}/${target.arch}`}</td><td>{target.kind === "image" ? build.release_version : `${target.artifact_key || target.key} · ${target.format_suffix || t("releaseFlow.detectSuffix")}`}</td></tr>)}</tbody></table></div>
      </details>
      {build.status === "needs_input" && <div className="notice build-attention" role="status">
        <span>{t("redesign.needsInput")}</span><Button size="sm" variant="secondary" onClick={() => setTab("plan")}>{t("redesign.viewPlan")}</Button>
      </div>}
      <Tabs value={tab} onChange={setTab} label={t("redesign.buildTabs")} items={[
        { value: "process", label: t("process.tab") },
        { value: "execution", label: t("redesign.execution") }, { value: "plan", label: t("redesign.plan") }, { value: "artifacts", label: t("redesign.output") },
      ]}>{activeTab => <>
        <div hidden={activeTab !== "process"}><BuildProcess key={id} build={build} navigate={setTab} /></div>
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
                  type="button"
                  aria-pressed={job?.id === target.id}
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
                      <TargetPlatformLabel target={target.target_config} />
                    </div>
                  </div>
                  <Status small value={target.status} />
                </button>
              ))
            ) : (
              <div className="p-5 text-xs leading-6 text-muted-foreground">
                {noJobsDetail}
              </div>
            )}
          </div>
          {job && !imageJob && <div className="border-t border-border p-4 text-[11px] leading-6 text-muted-foreground">
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
              t(["queued", "leased", "running"].includes(job.status) ? "builds.matching" : "builds.noAttempts")
            )}
          </div>}
        </section>
          {job && (!imageJob || canRetry) && <section className="panel">
            {!imageJob && <>
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
                  type="button"
                  aria-pressed={filter === step.name}
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
                <p className="whitespace-pre-wrap break-words text-sm text-destructive" role="status" aria-atomic="true">{attempt.error}</p>
              </div>
            )}
            </>}
            {canRetry && (
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
          </section>}
            </div>
          {imageJob ? <section className="panel"><SectionTitle title={t("images.build")} />
            {imageExecutions.length ? <ImageBuilds executions={imageExecutions} registry={build.image_registry} /> : <div className="space-y-3 p-5"><Status value={job.status} /><p className="text-sm text-muted-foreground">{t("images.noExecution")}</p></div>}
          </section> : !job ? <section className="panel"><Empty title={t("builds.noJobs")} detail={noJobsDetail} /></section> : !live ? <BuildLogHistory key={`${id}/${attempt?.id || ""}/${filter || ""}`} buildId={id} attemptId={attempt?.id} step={filter} /> : <section className="panel">
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
          {build.goal ? <BuildPlanning key={build.id} build={build} user={{ ...user, role: canMutate ? role : "viewer" }} refresh={remote.reload} />
            : <Empty title={t("redesign.plan")} detail={t("builds.noConfig")} />}
      <details className="build-detail-contract mt-5">
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
      <section className="panel build-detail-output" hidden={activeTab !== "artifacts"}>
        <SectionTitle
          title={t("builds.verified")}
          detail={t("artifacts.shaVerified")}
        />
        {!!build.image_executions?.length && <ImageBuilds executions={build.image_executions} registry={build.image_registry} />}
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

    </div>
  );
}
