/**
 * [INPUT]: 依赖项目授权设备平台、构建目标合同与共享控件。
 * [OUTPUT]: 提供目标类型与平台卡片，以及可折叠的精细配置。
 * [POS]: 构建引导第三步；稳定标识保留删除与改名时的输入焦点。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { Plus, Trash2 } from "lucide-react";
import { Field } from "@/components/common";
import { useUI } from "@/components/providers";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { defaultBuildPlatform, hasBuildPlatform, type BuildPlatform } from "@/lib/build-platforms";
import type { ApplicationPlatform, BuildTarget } from "@/lib/types";

export type TargetChoice = {id:number; target:BuildTarget; customKey:boolean};
export const initialTarget: TargetChoice = {id:0,customKey:false,target:{kind:"native",key:"linux-amd64",os:"linux",arch:"amd64",application_platform:"linux",artifact_key:"app",format_suffix:".tar.gz"}};
export function nativeTarget(platform:BuildPlatform,key=`${platform.os}-${platform.arch}`):Extract<BuildTarget,{kind:"native"}> {
  return {kind:"native",key,os:platform.os,arch:platform.arch,application_platform:platform.os==="darwin"?"macos":platform.os,artifact_key:key,format_suffix:".tar.gz"};
}
const presets = [
  {os:"linux",arch:"amd64",label:"platformLinuxIntel"},
  {os:"linux",arch:"arm64",label:"platformLinuxArm"},
  {os:"darwin",arch:"arm64",label:"platformMacArm"},
  {os:"darwin",arch:"amd64",label:"platformMacIntel"},
  {os:"windows",arch:"amd64",label:"platformWindows"},
] as const;

function uniqueKey(base:string, choices:TargetChoice[], id:number) {
  const used=new Set(choices.filter(choice=>choice.id!==id).map(choice=>choice.target.key));
  let key=base;
  for(let suffix=2;used.has(key);suffix++) key=`${base}-${suffix}`;
  return key;
}

export function BuildTargetPicker({choices,onChange,platforms}:{choices:TargetChoice[];onChange:(value:TargetChoice[])=>void;platforms?:BuildPlatform[]}) {
  const {t}=useUI();
  const preferred=defaultBuildPlatform(platforms);
  function update(id:number,target:BuildTarget,customKey?:boolean) {
    onChange(choices.map(choice=>choice.id===id?{...choice,target,customKey:customKey??choice.customKey}:choice));
  }
  function platform(choice:TargetChoice,os:"linux"|"darwin"|"windows",arch:"amd64"|"arm64",application:ApplicationPlatform|null=os==="darwin"?"macos":os) {
    if(choice.target.kind!=="native"||!hasBuildPlatform(platforms,os,arch)) return;
    const key=choice.customKey?choice.target.key:uniqueKey(`${os}-${arch}`,choices,choice.id);
    update(choice.id,{...choice.target,key,os,arch,application_platform:application,
      artifact_key:choice.target.artifact_key===choice.target.key?key:choice.target.artifact_key});
  }
  return <div className="space-y-5">
    {choices.map((choice,index)=>{
      const target=choice.target;
      return <section key={choice.id} className="guide-target" aria-label={t("releaseFlow.targetNumber",{number:index+1})}>
        <div className="mb-4 flex items-center justify-between gap-3"><h3 className="text-sm font-medium">{t("releaseFlow.targetNumber",{number:index+1})}</h3>
          <Button type="button" variant="ghost" size="sm" disabled={choices.length===1} onClick={()=>onChange(choices.filter(item=>item.id!==choice.id))}><Trash2 />{t("releaseFlow.removeTarget")}</Button></div>
        <fieldset className="guide-options"><legend className="sr-only">{t("releaseFlow.targetKind")}</legend>
          {(["native","image"] as const).map(kind=><label key={kind} className="wizard-choice" data-selected={target.kind===kind} data-disabled={kind==="native"&&!preferred}>
            <input type="radio" name={`target-${choice.id}-kind`} checked={target.kind===kind} disabled={kind==="native"&&!preferred} onChange={()=>{
              if(target.kind===kind)return;
              if(kind==="native"&&!preferred)return;
              const key=choice.customKey?target.key:uniqueKey(kind==="image"?"image":`${preferred!.os}-${preferred!.arch}`,choices,choice.id);
              update(choice.id,kind==="image"?{kind,key,platforms:["linux/amd64","linux/arm64"]}:nativeTarget(preferred!,key));
            }} />
            <span><span className="block font-medium">{t(`releaseFlow.${kind}`)}</span><span className="wizard-choice-detail">{t(`buildGuide.${kind}Hint`)}</span></span>
          </label>)}
        </fieldset>
        {target.kind==="native"?<fieldset className="guide-platforms"><legend className="mb-3 mt-5 text-xs text-muted-foreground">{t("buildGuide.platform")}</legend>
          {presets.map(preset=><label key={`${preset.os}-${preset.arch}`} className="wizard-choice" data-selected={target.os===preset.os&&target.arch===preset.arch&&hasBuildPlatform(platforms,preset.os,preset.arch)} data-disabled={!hasBuildPlatform(platforms,preset.os,preset.arch)} title={!hasBuildPlatform(platforms,preset.os,preset.arch)?t("buildGuide.devicesMissing"):undefined}>
            <input type="radio" name={`target-${choice.id}-platform`} disabled={!hasBuildPlatform(platforms,preset.os,preset.arch)} checked={target.os===preset.os&&target.arch===preset.arch&&hasBuildPlatform(platforms,preset.os,preset.arch)} onChange={()=>platform(choice,preset.os,preset.arch)} />
            <span>{t(`wizard.${preset.label}`)}</span>
          </label>)}
        </fieldset>:<fieldset className="guide-options mt-5"><legend className="mb-3 text-xs text-muted-foreground">{t("buildGuide.imagePlatforms")}</legend>
          {(["linux/amd64","linux/arm64"] as const).map(value=><label key={value} className="wizard-choice" data-selected={target.platforms.includes(value)}>
            <input type="checkbox" checked={target.platforms.includes(value)} onChange={event=>update(choice.id,{...target,platforms:event.target.checked?[...target.platforms,value]:target.platforms.filter(item=>item!==value)})} /><span>{value}</span>
          </label>)}
        </fieldset>}
        <details className="wizard-advanced mt-5"><summary>{t("buildGuide.targetAdvanced")}</summary><div className="mt-4 grid gap-4 sm:grid-cols-2">
          <Field label={t("releaseFlow.targetKey")}><Input value={target.key} maxLength={63} onChange={event=>update(choice.id,{...target,key:event.target.value},true)} /></Field>
          {target.kind==="native"&&<>
            <Field label={t("releaseFlow.hostOS")}><Select value={target.os} disabled={!preferred} onChange={event=>{
              const os=event.target.value as typeof target.os;
              const match=platforms?.find(item=>item.os===os&&item.device_count>0&&item.arch===target.arch)||platforms?.find(item=>item.os===os&&item.device_count>0);
              if(match)platform(choice,match.os,match.arch,null);
            }}>{([['linux','Linux'],['darwin','macOS'],['windows','Windows']] as const).map(([os,label])=><option key={os} value={os} disabled={!platforms?.some(item=>item.os===os&&item.device_count>0)}>{label}</option>)}</Select></Field>
            <Field label={t("releaseFlow.arch")}><Select value={target.arch} disabled={!preferred} onChange={event=>platform(choice,target.os,event.target.value as typeof target.arch,target.application_platform??null)}>{(["amd64","arm64"] as const).map(arch=><option key={arch} value={arch} disabled={!hasBuildPlatform(platforms,target.os,arch)}>{arch}</option>)}</Select></Field>
            <Field label={t("releaseFlow.application")}><Select value={target.application_platform||""} onChange={event=>update(choice.id,{...target,application_platform:(event.target.value||null) as ApplicationPlatform|null})}><option value="">{t("builds.nonApplication")}</option>{(target.os==="darwin"?["macos","ios","android"]:target.os==="linux"?["linux","android"]:["windows"]).map(value=><option key={value} value={value}>{value}</option>)}</Select></Field>
            <Field label={t("releaseFlow.artifactKey")}><Input value={target.artifact_key||""} maxLength={63} placeholder={target.key} onChange={event=>update(choice.id,{...target,artifact_key:event.target.value||undefined})} /></Field>
            <Field label={t("releaseFlow.suffix")}><Input value={target.format_suffix||""} maxLength={32} placeholder=".tar.gz" onChange={event=>update(choice.id,{...target,format_suffix:event.target.value||undefined})} /></Field>
          </>}
        </div></details>
      </section>;
    })}
    <Button type="button" variant="secondary" disabled={choices.length>=32} onClick={()=>{
      const id=Math.max(...choices.map(choice=>choice.id))+1;
      const key=uniqueKey(preferred?`${preferred.os}-${preferred.arch}`:"image",choices,id);
      onChange([...choices,{id,customKey:false,target:preferred?nativeTarget(preferred,key):{kind:"image",key,platforms:["linux/amd64","linux/arm64"]}}]);
    }}><Plus />{t("releaseFlow.addTarget")}</Button>
  </div>;
}
