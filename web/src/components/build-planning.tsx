/**
 * [INPUT]: 依赖构建状态、规划实时摘要、历史和澄清接口。
 * [OUTPUT]: 展示实时或已确认统计、计划和澄清入口。
 * [POS]: 复用规划轮询；取消与终态优先采用持久结果。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useRef } from "react";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading, SectionTitle, Status, TargetPlatformLabel } from "@/components/common";
import { SubmitForm } from "@/components/forms";
import { Textarea } from "@/components/ui/input";
import { mutation, useRemote } from "@/lib/api";
import { isBuildTerminal } from "@/lib/build-state";
import { PlanningConversations } from "@/components/planning-conversations";
import type { Build, PlanningHistory, User } from "@/lib/types";

export function BuildPlanning({build, user, refresh}: {build: Build; user: User; refresh: () => Promise<void>}) {
  const { t, number, date } = useUI();
  const terminal = isBuildTerminal(build.status);
  const history = useRemote<PlanningHistory>(`/builds/${build.id}/plans`, terminal ? false : 3000);
  const previous = useRef({ id: build.id, terminal });
  useEffect(() => {
    if (terminal && previous.current.id === build.id && !previous.current.terminal) void history.reload({progress:"background"});
    previous.current = { id: build.id, terminal };
  }, [build.id, terminal, history.reload]);
  const current = history.data?.plans.at(-1);
  const run = history.data?.runs.at(-1);
  const activeRun = run && !terminal && !["canceling", "needs_input"].includes(build.status)
    && ["queued", "leased"].includes(run.state);
  const runtime = activeRun ? run.runtime : undefined;
  const runtimeUnavailable = activeRun && !runtime && run.runtime_unavailable;
  const state = runtime?.status === "provisioning" ? "preparing" : runtime?.status || run?.state;
  const emptyHint = build.status === "needs_input" ? "ai.clarificationHint"
    : build.status === "failed" ? "ai.noPlanFailed"
    : build.status === "canceled" ? "ai.noPlanCanceled"
    : terminal ? "ai.noPlan" : "ai.planningHint";
  return <section className="panel mb-6 min-w-0">
    <SectionTitle title={t("ai.plan")} detail={t("ai.revisionCount", {count: build.revision_count || 0})}>{run && <span className="tag">{t(runtimeUnavailable ? "ai.confirmedCallCount" : "ai.callCount", {count: runtime?.calls ?? run.calls})}</span>}</SectionTitle>
    <div className="space-y-4 p-5">
      <PlanningConversations buildId={build.id} terminal={terminal} />
      <p className="whitespace-pre-wrap text-sm leading-6">{build.goal}</p>
      <ErrorBox message={history.error} />
      <p className="text-xs leading-5 text-muted-foreground">{t("ai.zeroIntrusion")}</p>
      {build.source_manifest_id && <p className="break-all font-mono text-xs text-muted-foreground">{t("ai.sourceManifest")} {build.source_manifest_id}</p>}
      {build.status === "needs_input" && build.clarification && <div className="notice space-y-4">
        <p className="whitespace-pre-wrap text-sm">{build.clarification.question}</p>
        {user.role !== "viewer" && <SubmitForm label={t("ai.continue")} onSubmit={async (form) => {await mutation(`/builds/${build.id}/clarifications`, {answer: String(form.get("answer") || "")}); await refresh(); await history.reload();}}><Field label={t("ai.answer")}><Textarea name="answer" required maxLength={8000} /></Field></SubmitForm>}
      </div>}
      {runtimeUnavailable && <p className="text-xs text-muted-foreground" role="status">{t("ai.runtimeUnavailable")}</p>}
      {run && <div className="flex flex-wrap gap-4 text-xs text-muted-foreground"><Status value={state || run.state} small /><span>{t("ai.tokens", {count: number((runtime?.usage ?? run.usage)?.totalTokens || 0)})}</span>{run.error && <span>{run.error}</span>}</div>}
      {current ? <>
        <p className="whitespace-pre-wrap text-sm leading-6">{current.contents.summary}</p>
        <ul className="space-y-2 text-xs text-muted-foreground">{current.contents.evidence.map((item, index) => <li key={index}><code className="break-all text-primary">{item.path}</code><span className="ml-2">{item.reason}</span></li>)}</ul>
        <details><summary className="cursor-pointer text-sm text-primary">{t("ai.executionPlan")}</summary><div className="mt-4 space-y-4">{Object.entries(current.contents.recipe.targets).map(([name, target]) => <div key={name}><h3 className="mb-2 text-sm font-medium">{name} · <TargetPlatformLabel target={target} /></h3>{target.steps.map((step, index) => <div className="mb-3" key={index}><p className="mb-1 text-xs text-muted-foreground">{step.name} · {step.phase}</p><pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-md bg-background p-3 text-xs">{step.run}</pre></div>)}</div>)}{current.contents.scripts.map((script) => <div key={script.name}><p className="mb-2 font-mono text-xs">{script.name}</p><pre className="overflow-x-auto whitespace-pre-wrap break-all rounded-md bg-background p-3 text-xs">{script.content}</pre></div>)}</div></details>
        <details><summary className="cursor-pointer text-sm text-muted-foreground">{t("ai.planHistory")}</summary><div className="mt-3 space-y-2">{history.data?.plans.map((entry) => <details key={entry.id}><summary className="cursor-pointer text-xs">{t("ai.version", {version: entry.revision})} · {date(entry.created_at)}</summary><p className="mt-2 whitespace-pre-wrap text-xs leading-5">{entry.contents.summary}</p><pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-all text-xs">{JSON.stringify(entry.contents, null, 2)}</pre></details>)}</div></details>
      </> : !history.data ? !history.error && <Loading variant="detail" /> : <p className="text-sm text-muted-foreground">{t(emptyHint)}</p>}
    </div>
  </section>;
}
