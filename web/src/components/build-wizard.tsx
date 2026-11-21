/**
 * [INPUT]: 依赖项目引用、授权设备平台、目标卡片和构建创建接口。
 * [OUTPUT]: 提供四步构建引导、可编辑摘要和最终确认提交。
 * [POS]: Release 内构建入口；只读版本，独立保存本次源码、要求和目标。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useId, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Check, Pencil, Play } from "lucide-react";
import { ErrorBox, Field } from "@/components/common";
import { BuildTargetPicker, initialTarget, nativeTarget, type TargetChoice } from "@/components/build-target-picker";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Combobox } from "@/components/ui/combobox";
import { Modal } from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/input";
import { mutation } from "@/lib/api";
import { asError, LocalizedError } from "@/lib/errors";
import { useRepositoryRefs } from "@/lib/repository-refs";
import { defaultBuildPlatform, targetsHaveDevices, useBuildPlatforms } from "@/lib/build-platforms";
import { buildTargetSummary, validBuildTargets } from "@/lib/build-targets";
import { notify } from "@/lib/notify";
import { buildPath } from "@/lib/routes";
import { useRouter } from "@/lib/tenant";
import type { Build, Project, Release } from "@/lib/types";

type Step=0|1|2|3;
const stepNames=["source","goal","targets","review"] as const;
const questions=["wizard.sourceQuestion","wizard.goalQuestion","buildGuide.targetsQuestion","wizard.reviewQuestion"] as const;
const bytes=(value:string)=>new TextEncoder().encode(value).length;

export function BuildWizard({release,project,onClose,canBuild=true}:{release:Release;project:Project;onClose:()=>void;canBuild?:boolean}) {
  const {t}=useUI();
  const router=useRouter();
  const formID=useId();
  const question=useRef<HTMLHeadingElement>(null);
  const busy=useRef(false);
  const refs=useRepositoryRefs(project.id);
  const platforms=useBuildPlatforms(project.id);
  const targetsTouched=useRef(false);
  const defaultApplied=useRef(false);
  const [step,setStep]=useState<Step>(0);
  const [returnToReview,setReturnToReview]=useState(false);
  const [source,setSource]=useState("");
  const sourceTouched=useRef(false);
  const [choices,setChoices]=useState<TargetChoice[]>([initialTarget]);
  const [goal,setGoal]=useState("");
  const [goalTemplate,setGoalTemplate]=useState("");
  const [parameters,setParameters]=useState("{}");
  const [dirty,setDirty]=useState(false);
  const [pending,setPending]=useState(false);
  const [error,setError]=useState<Error>();
  const [showParameters,setShowParameters]=useState(false);
  const available=release.status==="draft"&&canBuild;
  const targets=choices.map(choice=>choice.target);

  useEffect(()=>{if(!sourceTouched.current&&refs.data?.default_ref)setSource(refs.data.default_ref);},[refs.data]);
  useEffect(()=>{
    if(!platforms.data||defaultApplied.current||targetsTouched.current)return;
    defaultApplied.current=true;
    const preferred=defaultBuildPlatform(platforms.data);
    if(preferred)setChoices([{id:0,customKey:false,target:{...nativeTarget(preferred),artifact_key:"app"}}]);
    else setChoices([{id:0,customKey:false,target:{kind:"image",key:"image",platforms:["linux/amd64","linux/arm64"]}}]);
  },[platforms.data]);
  useEffect(()=>{
    question.current?.focus({preventScroll:true});
    question.current?.closest('[data-modal-scroll]')?.scrollTo({top:0});
  },[step]);

  function valid(current:Step):unknown {
    if(current===0&&(!source.trim()||source!==source.trim()||bytes(source)>255||source.startsWith("-")||/[\0\r\n]/.test(source)))throw new LocalizedError("wizard.invalidRef");
    if(current===1&&(!goal.trim()||bytes(goal)>16000))throw new LocalizedError("wizard.invalidGoal");
    if(current===2&&!validBuildTargets(targets))throw new LocalizedError("releaseFlow.invalidTargets");
    if(current===2&&targets.some(target=>target.kind==="native")){
      if(!platforms.data)throw new LocalizedError(platforms.loading?"buildGuide.devicesLoading":"buildGuide.devicesLoadFailed");
      if(!targetsHaveDevices(targets,platforms.data))throw new LocalizedError("buildGuide.devicesInvalid");
    }
    if(current===3){
      let value:unknown;
      try{value=JSON.parse(parameters);}catch{throw new LocalizedError("common.invalidJson");}
      if(!value||typeof value!=="object"||Array.isArray(value)||bytes(JSON.stringify(value))>16384||Object.entries(value).some(([key,item])=>key.startsWith("ARTIFACTORY_RELEASE_")||typeof item!=="string"))throw new LocalizedError("wizard.invalidParameters");
      return value;
    }
  }
  function advance(){
    if(busy.current)return;
    try{valid(step);setError(undefined);setStep(returnToReview?3:(step+1) as Step);setReturnToReview(false);}
    catch(error){setError(asError(error));}
  }
  function edit(next:Step){setReturnToReview(true);setError(undefined);setStep(next);}
  async function submit(){
    if(busy.current||!available)return;
    for(const current of [0,1,2,3] as const){
      try{valid(current);}catch(error){setStep(current);setReturnToReview(current!==3);if(current===3)setShowParameters(true);setError(asError(error));return;}
    }
    busy.current=true;setPending(true);setError(undefined);
    try{
      const build=await mutation<Build>(`/releases/${release.id}/builds`,{ref:source,goal,target_requirements:targets,targets:targets.map(target=>target.key),parameters:valid(3)});
      document.getElementById(formID)?.removeAttribute("data-dirty");setDirty(false);
      notify.success(t("common.operationSucceeded"));
      onClose();router.push(buildPath(release.project_slug,build.number,release.id));
    }catch(error){setError(asError(error));}
    finally{busy.current=false;setPending(false);}
  }
  const footer=<div className="guide-actions">
    <Button type="button" variant="ghost" disabled={pending} onClick={()=>{if(step===0){if(!dirty||window.confirm(t("workspace.unsaved")))onClose();}else{setReturnToReview(false);setError(undefined);setStep((step-1) as Step);}}}><ArrowLeft />{t(step===0?"common.cancel":"wizard.back")}</Button>
    <span className="text-xs text-muted-foreground">{t("wizard.progress",{current:step+1,total:4})}</span>
    <Button type="submit" form={formID} disabled={pending||(step===3&&!available)}>{pending?t("common.processing"):t(step===3?"buildGuide.confirm":returnToReview?"buildGuide.returnToReview":"wizard.continue")}{step===3?<Play />:<ArrowRight />}</Button>
  </div>;
  return <Modal open onOpenChange={open=>{if(!open)onClose();}} dismissible={!pending} initialFocus={question} size="wide" title={t("builds.start")} description={t("wizard.description")} footer={footer} navigation={
    <div className="guide-navigation"><div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs"><strong>{project.name}</strong><span className="text-muted-foreground">Release {release.version}</span></div>
      <ol className="wizard-steps" aria-label={t("redesign.wizardSteps")}>{stepNames.map((name,index)=><li key={name} aria-current={step===index?"step":undefined}><span className="wizard-step-dot">{index<step?<Check size={12} aria-hidden="true" />:index+1}</span>{t(`buildGuide.steps.${name}`)}</li>)}</ol>
    </div>
  }>
    <ErrorBox message={error} />
    {step===0&&<ErrorBox message={refs.error} />}
    {!available&&<p className="notice mb-5" role="status">{t("buildGuide.unavailable")}</p>}
    <form id={formID} noValidate data-dirty={dirty?"true":undefined} onSubmit={event=>{event.preventDefault();if(step===3)void submit();else advance();}}>
      <h2 ref={question} tabIndex={-1} className="wizard-question">{t(questions[step])}</h2>
      <p className="mb-6 mt-3 text-sm leading-6 text-muted-foreground">{t(["releaseFlow.sourceHint","wizard.goalInputHint","buildGuide.targetsHint","wizard.reviewHint"][step])}</p>
      <fieldset disabled={pending} className="min-w-0 border-0 p-0">
      {step===0&&<Field label={t("releaseFlow.source")}><Combobox aria-label={t("releaseFlow.source")} name="ref" value={source} required maxLength={255} onValueChange={value=>{sourceTouched.current=true;setSource(value);setDirty(true);}}
        options={[...(refs.data?.branches||[]),...(refs.data?.tags||[])].map(ref=>({value:ref.ref,label:ref.name}))}
        loading={refs.loading} error={!!refs.error} emptyLabel={t("wizard.noRefs")} loadingLabel={t("common.loading")} errorLabel={t("common.requestFailed")} placeholder="refs/heads/main" />
        {refs.error&&<Button type="button" variant="ghost" className="mt-3" onClick={refs.reload}>{t("common.retry")}</Button>}
      </Field>}
      {step===1&&<div className="space-y-5"><fieldset className="guide-goals"><legend className="sr-only">{t("releaseFlow.buildGoal")}</legend>
        {["Build","Test","Custom"].map(template=><label key={template} className="wizard-choice" data-selected={goalTemplate===template}><input type="radio" name="goal-template" checked={goalTemplate===template} onChange={()=>{setGoalTemplate(template);if(template!=="Custom")setGoal(t(`wizard.goal${template}Text`));setDirty(true);}} /><span>{t(`wizard.goal${template}`)}</span></label>)}
      </fieldset><Field label={t("releaseFlow.buildGoal")}><Textarea name="goal" aria-label={t("releaseFlow.buildGoal")} required value={goal} maxLength={16000} onChange={event=>{setGoal(event.target.value);setGoalTemplate("Custom");setDirty(true);}} placeholder={t("releaseFlow.goalPlaceholder")} /></Field></div>}
      {step===2&&<>
        <p className="mb-4 text-xs leading-5 text-muted-foreground">{t("buildGuide.devicesHint")}</p>
        {(platforms.loading||platforms.error||platforms.data?.length===0)&&<div className="notice mb-4" role="status">{t(platforms.loading?"buildGuide.devicesLoading":platforms.error?"buildGuide.devicesLoadFailed":"buildGuide.devicesEmpty")}
          {platforms.error&&<Button type="button" variant="ghost" className="ml-2" onClick={platforms.reload}>{t("common.retry")}</Button>}
        </div>}
        <BuildTargetPicker choices={choices} platforms={platforms.data} onChange={value=>{targetsTouched.current=true;setChoices(value);setDirty(true);}} />
      </>}
      {step===3&&<>
        <dl className="wizard-review">
          <div><dt>{t("projects.project")} / Release</dt><dd>{project.name} · {release.version}</dd></div>
          {[
            {label:t("releaseFlow.source"),value:source,edit:"wizard.editSource",step:0 as Step},
            {label:t("releaseFlow.buildGoal"),value:goal,edit:"buildGuide.editGoal",step:1 as Step},
            {label:t("releaseFlow.targets"),value:buildTargetSummary({selected_targets:targets.map(target=>target.key),target_requirements:targets}),edit:"buildGuide.editTargets",step:2 as Step},
          ].map(item=><div key={item.step}><dt>{item.label}</dt><dd><span>{item.value}</span><Button type="button" size="icon" variant="ghost" aria-label={t(item.edit)} onClick={()=>edit(item.step)}><Pencil /></Button></dd></div>)}
        </dl>
        <details className="wizard-advanced mt-5" open={showParameters} onToggle={event=>setShowParameters(event.currentTarget.open)}><summary>{t("wizard.advanced")}</summary><div className="mt-4"><Field label={t("releaseFlow.parameters")}><Textarea name="parameters" aria-label={t("releaseFlow.parameters")} value={parameters} onChange={event=>{setParameters(event.target.value);setDirty(true);}} spellCheck={false} /><p className="mt-2 text-xs text-muted-foreground">{t("releaseFlow.parametersHint")}</p></Field></div></details>
        <p className="mt-5 text-xs leading-6 text-muted-foreground">{t("wizard.submitHint")}</p>
      </>}
      </fieldset>
    </form>
  </Modal>;
}
