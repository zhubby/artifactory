/**
 * [INPUT]: 依赖当前计划身份、历史、实时摘要和澄清接口。
 * [OUTPUT]: 展示结构化方案、修订切换、统计与澄清草稿。
 * [POS]: 计划工作台保留阅读状态；审计按需读取，终态静默补读。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useEffect, useId, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { useUI } from "@/components/providers";
import { ErrorBox, Field, Loading, Status, TargetPlatformLabel } from "@/components/common";
import { BuildPlanTarget, PlanCode } from "@/components/build-plan-target";
import { PlanningConversations } from "@/components/planning-conversations";
import { SubmitForm } from "@/components/forms";
import { Textarea } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { mutation, useRemote } from "@/lib/api";
import { isBuildTerminal } from "@/lib/build-state";
import type { Build, PlanRevision, PlanningHistory, User } from "@/lib/types";

function PlanSummary({ summary }: { summary: string }) {
  const { t } = useUI();
  const id = useId();
  const text = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflow, setOverflow] = useState(false);
  useEffect(() => {
    const element = text.current;
    if (!element || expanded) return;
    const measure = () => setOverflow(element.scrollHeight > element.clientHeight + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [summary, expanded]);
  return <div className="build-plan-summary">
    <p id={id} ref={text} className="build-plan-summary-text" data-expanded={expanded}>{summary}</p>
    {(overflow || expanded) && <Button variant="ghost" size="sm" className="build-plan-summary-toggle" aria-expanded={expanded} aria-controls={id} onClick={() => setExpanded(value => !value)}>{t(expanded ? "buildDetail.collapseSummary" : "buildDetail.expandSummary")}</Button>}
  </div>;
}

function PlanContents({ plan, build, plans, currentView, selectedTarget, selectTarget }: {
  plan: PlanRevision; build: Build; plans: PlanRevision[]; currentView: boolean;
  selectedTarget: string | null; selectTarget: (target: string) => void;
}) {
  const { t } = useUI();
  const targets = Object.entries(plan.contents.recipe.targets);
  const active = targets.some(([name]) => name === selectedTarget) ? selectedTarget : targets[0]?.[0];
  return <>
    <PlanSummary summary={plan.contents.summary} />
    <div className="build-plan-workspace" role="region" aria-label={t("buildDetail.scheme")}>
      <nav className="build-plan-nav" aria-label={t("buildDetail.target")}>
        <h3 className="build-plan-nav-label">{t("buildDetail.targets", { count: targets.length })}</h3>
        {targets.map(([name, target]) => <button type="button" className="build-plan-nav-item" aria-pressed={active === name} key={name} onClick={() => selectTarget(name)}><div>
          <span className="build-plan-nav-kind">{t(target.executor === "buildkit" ? "buildDetail.imageTarget" : "buildDetail.nativeTarget")}</span>
          <span className="build-plan-nav-name">{name}</span>
          <span className="build-plan-nav-meta">{target.executor === "buildkit" ? target.image?.platforms.join(" · ") : <TargetPlatformLabel target={target} />}</span>
        </div></button>)}
      </nav>
      <div className="build-plan-mobile-target">
        <label>{t("buildDetail.target")}</label>
        <Select aria-label={t("buildDetail.target")} value={active || ""} onChange={event => selectTarget(event.target.value)}>
          {targets.map(([name, target]) => <option key={name} value={name}>{name} · {t(target.executor === "buildkit" ? "buildDetail.imageTarget" : "buildDetail.nativeTarget")}</option>)}
        </Select>
      </div>
      <div className="build-plan-targets">{targets.map(([name, target]) => <section key={name} hidden={active !== name} data-target={name} aria-label={name}>
        <BuildPlanTarget name={name} target={target} plan={plan} build={build} plans={plans} currentView={currentView} />
      </section>)}</div>
    </div>
    <div className="build-plan-secondary">
      {!!plan.contents.evidence.length && <details className="build-plan-fold">
        <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.evidence", { count: plan.contents.evidence.length })}</summary>
        <ul className="space-y-3">{plan.contents.evidence.map((item, index) => <li key={`${item.path}/${index}`} className="text-sm"><code className="break-all">{item.path}</code><p className="mt-1 text-muted-foreground">{item.reason}</p></li>)}</ul>
      </details>}
      {!!plan.contents.scripts.length && <details className="build-plan-fold">
        <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.scripts", { count: plan.contents.scripts.length })}</summary>
        <div className="space-y-3">{plan.contents.scripts.map(script => <PlanCode key={script.name} value={script.content} label={script.name} />)}</div>
      </details>}
      <details className="build-plan-fold">
        <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.rawPlan")}</summary>
        <PlanCode value={JSON.stringify(plan.contents, null, 2)} label={t("buildDetail.rawPlan")} />
      </details>
    </div>
  </>;
}

function mergePlans(previous: PlanRevision[], incoming: PlanRevision[]) {
  return [...new Map([...previous, ...incoming].map(plan => [plan.id, plan])).values()].sort((a, b) => b.revision - a.revision);
}

export function BuildPlanning({ build, user, refresh }: { build: Build; user: User; refresh: () => Promise<void> }) {
  const { t, number, date } = useUI();
  const terminal = isBuildTerminal(build.status);
  const history = useRemote<PlanningHistory>(`/builds/${build.id}/plans`, data => {
    const synchronized = !build.current_plan_id || data?.plans.some(plan => plan.id === build.current_plan_id);
    return terminal && synchronized ? false : 3000;
  });
  const previous = useRef({ id: build.id, terminal });
  const [known, setKnown] = useState<{ buildId: string; plans: PlanRevision[] }>({ buildId: build.id, plans: [] });
  const [choice, setChoice] = useState<{ buildId: string; id: string | null }>({ buildId: build.id, id: null });
  const [retained, setRetained] = useState<{ buildId: string; plan?: PlanRevision }>({ buildId: build.id });
  const [selectedTarget, setSelectedTarget] = useState<string | null>(null);
  const [answer, setAnswer] = useState("");
  const explicitId = choice.buildId === build.id ? choice.id : null;
  const plans = mergePlans(known.buildId === build.id ? known.plans : [], history.data?.plans || []);
  const selectedId = explicitId ?? build.current_plan_id;
  const selected = plans.find(plan => plan.id === selectedId);
  const plan = selected ?? (retained.buildId === build.id ? retained.plan : undefined) ?? plans[0];
  const current = plans.find(entry => entry.id === build.current_plan_id);
  const syncing = Boolean(history.data && (
    (build.current_plan_id && !history.data.plans.some(entry => entry.id === build.current_plan_id))
    || (!build.current_plan_id && history.data.plans.length > 0)
    || (current && history.data.plans.some(entry => entry.revision > current.revision))
  ));
  useEffect(() => {
    if (history.data) setKnown(value => ({ buildId: build.id, plans: mergePlans(value.buildId === build.id ? value.plans : [], history.data!.plans) }));
  }, [build.id, history.data]);
  useEffect(() => {
    if (selected) setRetained({ buildId: build.id, plan: selected });
  }, [build.id, selected]);
  useEffect(() => {
    if (terminal && previous.current.id === build.id && !previous.current.terminal) void history.reload({ progress: "background" });
    previous.current = { id: build.id, terminal };
  }, [build.id, terminal, history.reload]);

  const run = history.data?.runs.at(-1);
  const activeRun = run && !terminal && !["canceling", "needs_input"].includes(build.status) && ["queued", "leased"].includes(run.state);
  const runtime = activeRun ? run.runtime : undefined;
  const runtimeUnavailable = activeRun && !runtime && run.runtime_unavailable;
  const state = runtime?.status === "provisioning" ? "preparing" : runtime?.status || run?.state || build.status;
  const emptyHint = build.status === "needs_input" ? "ai.clarificationHint"
    : build.status === "failed" ? "ai.noPlanFailed"
    : build.status === "canceled" ? "ai.noPlanCanceled"
    : terminal ? "ai.noPlan" : "ai.planningHint";
  const revisionLabel = (entry: PlanRevision) => `${t("ai.version", { version: entry.revision })} · ${date(entry.created_at)}${entry.id === build.current_plan_id ? ` · ${t("buildDetail.currentRevision")}` : ""}`;
  const currentOption = build.current_plan_id || "current";
  return <section className="panel mb-6 min-w-0 build-plan">
    <header className="build-plan-header">
      <div className="build-plan-heading"><h2>{t("ai.plan")}</h2><Status value={state} small /></div>
      <div className="build-plan-stats">
        <span>{t("ai.revisionCount", { count: build.revision_count ?? plans.length })}</span>
        {run && <><span>{t(runtimeUnavailable ? "ai.confirmedCallCount" : "ai.callCount", { count: runtime?.calls ?? run.calls })}</span><span>{t("ai.tokens", { count: number((runtime?.usage ?? run.usage)?.totalTokens || 0) })}</span></>}
      </div>
      {!!plans.length && <div className="build-plan-revision">
        <label>{t("buildDetail.planRevision")}</label>
        <Select aria-label={t("buildDetail.planRevision")} value={explicitId ?? currentOption} onChange={event => setChoice({ buildId: build.id, id: event.target.value === currentOption ? null : event.target.value })}>
          {!current && <option value={currentOption}>{t("buildDetail.currentRevision")} · {t("buildDetail.syncing")}</option>}
          {plans.map(entry => <option key={entry.id} value={entry.id}>{revisionLabel(entry)}</option>)}
        </Select>
      </div>}
    </header>
    <div className="build-plan-body">
      <ErrorBox message={history.error} />
      {build.status === "needs_input" && build.clarification && <div className="build-plan-clarification">
        <div><h3>{t("buildDetail.clarification")}</h3><p className="whitespace-pre-wrap">{build.clarification.question}</p></div>
        {user.role !== "viewer" && <SubmitForm label={t("ai.continue")} onSubmit={async form => {
          await mutation(`/builds/${build.id}/clarifications`, { answer: String(form.get("answer") || "") });
          setAnswer("");
          await refresh();
          await history.reload();
        }}><Field label={t("ai.answer")}><Textarea name="answer" required maxLength={8000} value={answer} onChange={event => setAnswer(event.target.value)} /></Field></SubmitForm>}
      </div>}
      {runtimeUnavailable && <p className="text-xs text-muted-foreground" role="status">{t("ai.runtimeUnavailable")}</p>}
      {run?.error && <p className="text-sm text-muted-foreground">{run.error}</p>}
      {syncing && <p className="build-plan-sync" role="status"><span>{t("buildDetail.syncing")}</span>{plan && plan.id !== build.current_plan_id && <span className="ml-2">{t("buildDetail.displayedRevision", { revision: plan.revision })}</span>}</p>}
      {explicitId && <p className="build-plan-historical">{t("buildDetail.historicalRevision")}</p>}
      {plan ? <PlanContents key={plan.id} plan={plan} build={build} plans={plans} currentView={!explicitId && plan.id === build.current_plan_id} selectedTarget={selectedTarget} selectTarget={setSelectedTarget} /> : !history.data ? !history.error && <Loading variant="detail" /> : <p className="text-sm text-muted-foreground">{t(emptyHint)}</p>}
      <div className="build-plan-secondary">
        {build.goal && <details className="build-plan-fold"><summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.originalGoal")}</summary><p className="whitespace-pre-wrap text-sm leading-6">{build.goal}</p></details>}
        <details className="build-plan-fold">
          <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.sourceDetails")}</summary>
          <div className="space-y-3"><p className="text-xs leading-5 text-muted-foreground">{t("ai.zeroIntrusion")}</p>{build.source_manifest_id && <p className="break-all font-mono text-xs text-muted-foreground">{t("ai.sourceManifest")} {build.source_manifest_id}</p>}</div>
        </details>
        <PlanningConversations buildId={build.id} terminal={terminal} />
      </div>
    </div>
  </section>;
}
