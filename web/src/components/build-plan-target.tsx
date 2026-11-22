/**
 * [INPUT]: 依赖已选计划、配方目标和带修订身份的任务与尝试。
 * [OUTPUT]: 提供原生顺序步骤、镜像参数和同修订执行记录。
 * [POS]: 计划工作台的目标阅读器；旧成功只标注来源。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";

import { useState, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";
import { CopyButton, Status, TargetPlatformLabel } from "@/components/common";
import { useUI } from "@/components/providers";
import type { Build, PlanRevision, RecipeStep, RecipeTarget, StepRun } from "@/lib/types";

export function PlanCode({ value, label }: { value: string; label: string }) {
  return <div className="build-plan-code">
    <div className="build-plan-code-toolbar"><span>{label}</span><CopyButton value={value} /></div>
    <pre tabIndex={0} aria-label={label}><code>{value}</code></pre>
  </div>;
}

function PlanField({ label, children }: { label: string; children: ReactNode }) {
  return <div><dt>{label}</dt><dd>{children}</dd></div>;
}

function PlanStep({ step, index, record }: { step: RecipeStep; index: number; record?: StepRun }) {
  const { t } = useUI();
  const [open, setOpen] = useState(index === 0);
  return <li className="build-plan-step">
    <details open={open} onToggle={event => setOpen(event.currentTarget.open)}>
      <summary className="build-plan-step-heading">
        <span className="build-plan-step-number">{index + 1}</span>
        <span className="build-plan-step-title">{step.name}</span>
        <span className="tag">{t(`buildDetail.phases.${step.phase}`)}</span>
        {record && <Status value={record.status} small />}
        <ChevronRight className="build-plan-step-chevron" size={16} aria-hidden="true" />
      </summary>
      <div className="build-plan-step-body">
        <dl className="build-plan-fields">
          <PlanField label={t("buildDetail.shell")}><code>{step.shell || t("buildDetail.defaultShell")}</code></PlanField>
          {step.timeout_seconds !== undefined && <PlanField label={t("buildDetail.stepTimeout")}>{t("buildDetail.seconds", { count: step.timeout_seconds })}</PlanField>}
          {record?.exit_code !== undefined && record.exit_code !== null && <PlanField label={t("buildDetail.exitCode")}>{record.exit_code}</PlanField>}
        </dl>
        <PlanCode value={step.run} label={t("buildDetail.command")} />
      </div>
    </details>
  </li>;
}

function ImageTarget({ target }: { target: RecipeTarget }) {
  const { t } = useUI();
  const image = target.image;
  if (!image) return null;
  const args = Object.entries(image.build_args || {});
  const secrets = Object.entries(image.secrets || {});
  return <div className="build-plan-image">
    <dl className="build-plan-fields">
      <PlanField label={t("buildDetail.imageName")}><code>{image.name}</code></PlanField>
      <PlanField label={t("buildDetail.imagePlatforms")}><div className="flex flex-wrap gap-2">{image.platforms.map(platform => <code className="tag" key={platform}>{platform}</code>)}</div></PlanField>
      <PlanField label={t("buildDetail.dockerfile")}><code>{image.dockerfile ?? "Dockerfile"}</code></PlanField>
      <PlanField label={t("buildDetail.context")}><code>{image.context ?? "."}</code></PlanField>
      <PlanField label={t("buildDetail.builderPool")}><code>{image.pool ?? "default"}</code></PlanField>
      {target.timeout_seconds !== undefined && <PlanField label={t("buildDetail.targetTimeout")}>{t("buildDetail.seconds", { count: target.timeout_seconds })}</PlanField>}
    </dl>
    <details className="build-plan-fold">
      <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.imageOptions")}</summary>
      <dl className="build-plan-fields">
        <PlanField label={t("buildDetail.buildArgs")}>{args.length ? args.map(([key, value]) => <div key={key}><code>{key}={value}</code></div>) : t("buildDetail.none")}</PlanField>
        <PlanField label={t("buildDetail.emulation")}>{t(image.allow_emulation ? "buildDetail.enabled" : "buildDetail.disabled")}</PlanField>
        <PlanField label={t("buildDetail.imageTags")}>{image.tags?.length ? image.tags.join(", ") : t("buildDetail.none")}</PlanField>
        <PlanField label={t("buildDetail.secretRefs")}>{secrets.length ? secrets.map(([key, value]) => <div key={key}><code>{key}: {value}</code></div>) : t("buildDetail.none")}</PlanField>
        <PlanField label={t("buildDetail.pullCredentials")}>{image.pull_credentials?.length ? image.pull_credentials.map(value => <div key={value}><code>{value}</code></div>) : t("buildDetail.none")}</PlanField>
      </dl>
    </details>
  </div>;
}

export function BuildPlanTarget({ name, target, plan, build, plans, currentView }: {
  name: string; target: RecipeTarget; plan: PlanRevision; build: Build; plans: PlanRevision[]; currentView: boolean;
}) {
  const { t } = useUI();
  const jobs = currentView ? (build.jobs || []).filter(job => job.target === name) : [];
  const job = jobs.find(entry => entry.plan_revision_id === plan.id);
  const attempt = jobs.flatMap(entry => entry.attempts).filter(entry => entry.plan_revision_id === plan.id).at(-1);
  const retained = currentView && !job ? jobs.find(entry => entry.status === "succeeded" && entry.plan_revision_id && entry.plan_revision_id !== plan.id) : undefined;
  const origin = plans.find(entry => entry.id === retained?.plan_revision_id);
  const status = job?.status ?? attempt?.status;
  const image = target.executor === "buildkit";
  return <>
    <div className="build-plan-target-header">
      <div><span className="build-plan-nav-kind">{t(image ? "buildDetail.imageTarget" : "buildDetail.nativeTarget")}</span><h3>{name}</h3></div>
      {status && <Status value={status} small />}
    </div>
    {!image && <p className="build-plan-target-meta"><TargetPlatformLabel target={target} /></p>}
    {retained && <p className="build-plan-retained" role="status"><Status value="succeeded" small />{t("buildDetail.retainedSuccess", { revision: origin?.revision ?? retained.plan_revision_id?.slice(0, 8) ?? "—" })}</p>}
    {image ? <ImageTarget target={target} /> : <>
      <dl className="build-plan-fields">
        {target.timeout_seconds !== undefined && <PlanField label={t("buildDetail.targetTimeout")}>{t("buildDetail.seconds", { count: target.timeout_seconds })}</PlanField>}
        <PlanField label={t("buildDetail.artifactPatterns")}>{target.artifacts?.length ? target.artifacts.map(path => <div key={path}><code>{path}</code></div>) : t("buildDetail.none")}</PlanField>
        <PlanField label={t("buildDetail.signing")}>{target.signing_profiles?.length ? target.signing_profiles.join(", ") : t("buildDetail.none")}</PlanField>
        {!!target.signing_outputs?.length && <PlanField label={t("buildDetail.signingOutputs")}>{target.signing_outputs.map(output => <div key={output.path}><code>{output.path}{output.bundle ? ` → ${output.bundle}` : ""}</code></div>)}</PlanField>}
      </dl>
      <h4 className="build-plan-section-label">{t("buildDetail.sequentialSteps", { count: target.steps.length })}</h4>
      <ol className="build-plan-steps">{target.steps.map((step, index) => <PlanStep key={`${index}/${step.name}`} step={step} index={index} record={attempt?.steps.find(record => record.step_index === index && record.name === step.name && record.phase === step.phase)} />)}</ol>
      {(Object.keys(target.labels || {}).length > 0 || !!target.capabilities?.length) && <details className="build-plan-fold">
        <summary><ChevronRight size={16} aria-hidden="true" />{t("buildDetail.deviceRequirements")}</summary>
        <dl className="build-plan-fields">
          {!!target.capabilities?.length && <PlanField label={t("buildDetail.capabilities")}>{target.capabilities.join(", ")}</PlanField>}
          {Object.entries(target.labels || {}).map(([key, value]) => <PlanField key={key} label={key}><code>{value}</code></PlanField>)}
        </dl>
      </details>}
    </>}
  </>;
}
