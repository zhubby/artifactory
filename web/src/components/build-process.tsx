/**
 * [INPUT]: 依赖受权过程事件、构建快照、共享状态和规划审计入口。
 * [OUTPUT]: 展示当前动作、五阶段状态和可展开的真实事件记录。
 * [POS]: 构建详情的默认观察页；保留筛选、展开和终态补读。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useRef, useState } from "react";
import { ArrowRight, Check, ChevronRight, Clock3, TriangleAlert, X } from "lucide-react";
import { useUI } from "@/components/providers";
import { ErrorBox, Loading, Status } from "@/components/common";
import { PlanningConversations } from "@/components/planning-conversations";
import { Button } from "@/components/ui/button";
import { useRemote } from "@/lib/api";
import { buildStages, type ActivityEvent, type BuildActivity, type ProcessStage } from "@/lib/build-activity";
import { isBuildTerminal } from "@/lib/build-state";
import type { Build } from "@/lib/types";
import { elapsed } from "@/lib/utils";

function EventRow({ event }: { event: ActivityEvent }) {
  const { t, date } = useUI();
  const details = event.details;
  const actor = details.actor && ["control", "planner", "scheduler", "source", "buildkit", "storage", "registry"].includes(details.actor)
    ? t(`process.actors.${details.actor}`) : details.actor;
  const fields = [
    [t("process.actor"), actor], [t("process.target"), details.target],
    [t("process.attempt"), details.attempt], [t("process.model"), details.model],
    [t("process.purpose"), details.purpose ? t(`process.purposes.${details.purpose}`) : null],
    [t("process.revision"), details.revision], [t("process.calls"), details.calls],
    [t("process.step"), details.step], [t("process.phase"), details.phase],
    [t("process.exitCode"), details.exit_code], [t("process.commit"), details.commit_sha],
    [t("process.file"), details.path], ["SHA-256", details.sha256 || details.digest],
    [t("process.executionId"), details.attempt_id || details.execution_id || details.run_id],
    [t("process.planId"), details.plan_id],
  ].filter(([, value]) => value !== undefined && value !== null && value !== "");
  return <li>
    <span className={`build-process-event-dot event-${event.status}`} aria-hidden="true" />
    <details>
      <summary>
        <div><span className="build-process-event-action">{t(`process.actions.${event.action}`)}</span>
          {details.target && <span className="build-process-event-target">{details.target}</span>}
          {details.step && <span className="build-process-event-target">{details.step}</span>}
          <span className="build-process-event-actor">{actor}</span>
        </div>
        {["running", "queued"].includes(event.status)
          ? <span className="build-process-event-state">{t(event.status === "running" ? "process.eventStarted" : "process.eventQueued")}</span>
          : <Status value={event.status === "completed" ? "succeeded" : event.status} small />}
        <time dateTime={event.created_at} title={date(event.created_at)}>{date(event.created_at)}</time>
        <ChevronRight size={14} aria-hidden="true" />
      </summary>
      <dl>{fields.map(([label, value]) => <div key={String(label)}><dt>{label}</dt><dd>{String(value)}</dd></div>)}</dl>
    </details>
  </li>;
}

export function BuildProcess({ build, navigate }: { build: Build; navigate: (tab: string) => void }) {
  const { t, number } = useUI();
  const terminal = isBuildTerminal(build.status);
  const remote = useRemote<BuildActivity>(`/builds/${build.id}/activity`, terminal ? false : 3000);
  const previous = useRef(terminal);
  useEffect(() => {
    if (terminal && !previous.current) void remote.reload({ progress: "background" });
    previous.current = terminal;
  }, [terminal, remote.reload]);
  const [filter, setFilter] = useState<ProcessStage>();
  const [limit, setLimit] = useState(12);
  const activity = remote.data;
  const stages = buildStages(build, activity);
  const sourceFailed = stages[0].state === "failed" && !build.commit_sha;
  const run = activity?.planning.runs.at(-1);
  const calls = run?.runtime?.calls ?? run?.calls ?? 0;
  const failed = stages.find(stage => stage.state === "failed");
  const current = failed || stages.find(stage => ["running", "needs_input", "queued", "canceled"].includes(stage.state));
  const summary = sourceFailed ? "sourceFailed" : build.status === "succeeded" ? "complete"
    : build.status === "canceled" ? "canceled" : build.status === "canceling" ? "canceling"
    : build.status === "needs_input" ? "needsInput" : failed ? "failed"
    : current?.key === "source" ? "preparingSource" : current?.key === "planning" ? build.status === "replanning" ? "replanning" : "planning"
    : current?.key === "dispatch" ? "waiting" : current?.key === "artifacts" ? "archiving" : "executing";
  const events = [...(activity?.events || [])].reverse().filter(event => !filter || event.stage === filter);
  const diagnostic = activity?.source?.diagnostic;
  const historyMissing = activity?.source && !activity.source.history_available && terminal && !build.commit_sha;
  const outputCount = (build.artifacts?.length || 0) + (build.image_executions?.filter(image => image.operation === "build" && image.status === "succeeded" && image.root_digest).length || 0);
  return <section className="build-process" role="region" aria-label={t("process.title")}>
    <ErrorBox message={remote.error} />
    {remote.error && <Button variant="secondary" size="sm" onClick={() => void remote.reload()}>{t("common.retry")}</Button>}
    <header className={`build-process-summary ${failed ? "has-failure" : ""}`}>
      <div className="build-process-summary-icon" aria-hidden="true">{failed ? <TriangleAlert size={22} /> : build.status === "succeeded" ? <Check size={22} /> : <Clock3 size={22} />}</div>
      <div>
        <p className="build-process-eyebrow">{t("process.current")}{current && <span> / {t(`process.stages.${current.key}`)}</span>}</p>
        <h2>{t(`process.summary.${summary}`)}</h2>
        {diagnostic && <p>{t("process.failedAction")}: {t(`process.actions.source_${diagnostic.stage}`)}</p>}
        {diagnostic ? <p>{t(`process.diagnostics.${diagnostic.code}`)}</p>
          : build.status === "succeeded" ? <p>{t("process.outputCount", { count: outputCount })} {t("process.releaseHint")}</p>
          : <p>{t(`process.hints.${summary}`)}</p>}
        {sourceFailed && <div className="build-process-downstream"><span>{t("process.aiNotStarted")}</span><span>{t("process.deviceNotStarted")}</span></div>}
        {build.status === "needs_input" && build.clarification && <p>{build.clarification.question}</p>}
        {!!activity?.planning.runs.length && <p className="build-process-counts">{t(run?.runtime_unavailable ? "ai.confirmedCallCount" : "ai.callCount", { count: calls })} · {t("process.planCount", { count: activity.planning.plans.length })}</p>}
        {build.jobs?.filter(job => job.blocked_reason).map(job => <p key={job.id}>{job.target}: {job.blocked_reason}</p>)}
      </div>
      <span className="build-process-duration"><Clock3 size={13} aria-hidden="true" />{elapsed(build.created_at, build.finished_at)}</span>
    </header>
    <nav className="build-process-stages" aria-label={t("process.stageNavigation")}>
      {stages.map((stage, index) => <button key={stage.key} type="button" aria-pressed={filter === stage.key}
        className={`build-process-stage stage-${stage.state}`} onClick={() => { setFilter(filter === stage.key ? undefined : stage.key); setLimit(12); }}>
        <span className="build-process-stage-number" aria-hidden="true">{stage.state === "succeeded" ? <Check size={14} /> : stage.state === "failed" ? <X size={14} /> : <span>{index + 1}</span>}</span>
        <span><strong>{t(`process.stages.${stage.key}`)}</strong><small>{t(`process.states.${stage.state}`)}</small></span>
        <ChevronRight size={14} aria-hidden="true" />
      </button>)}
    </nav>
    {historyMissing && <p className="build-process-history-note">{t("process.historyMissing")}</p>}
    <div className="build-process-detail-links">
      <Button variant="ghost" size="sm" onClick={() => navigate("plan")}>{t("process.viewPlan")}<ArrowRight size={13} /></Button>
      <Button variant="ghost" size="sm" onClick={() => navigate("execution")}>{t("process.viewLogs")}<ArrowRight size={13} /></Button>
      <Button variant="ghost" size="sm" onClick={() => navigate("artifacts")}>{t("process.viewArtifacts")}<ArrowRight size={13} /></Button>
    </div>
    <section className="build-process-timeline" aria-labelledby={`process-events-${build.id}`}>
      <header><div><h3 id={`process-events-${build.id}`}>{t("process.activity")}</h3><p>{t("process.activityHint")}</p></div>
        {filter && <Button variant="ghost" size="sm" onClick={() => { setFilter(undefined); setLimit(12); }}>{t("process.allStages")}</Button>}
        {activity && <span>{t("process.eventCount", { count: number(events.length) })}</span>}
      </header>
      {!activity ? !remote.error && <Loading variant="list" /> : <>
        {!events.length && <p className="build-process-empty">{t("process.noEvents")}</p>}
        <ol className="build-process-events">{events.slice(0, limit).map(event => <EventRow key={event.id} event={event} />)}</ol>
        {events.length > limit && <Button variant="secondary" size="sm" onClick={() => setLimit(value => value + 25)}>{t("process.moreEvents")}</Button>}
        {activity.truncated && <p className="build-process-history-note">{t("process.truncated")}</p>}
      </>}
    </section>
    {!!activity?.planning.runs.length && <PlanningConversations buildId={build.id} terminal={terminal} />}
    {build.error && <details className="build-process-error"><summary>{t("process.errorDetails")}</summary><p>{build.error}</p></details>}
  </section>;
}
