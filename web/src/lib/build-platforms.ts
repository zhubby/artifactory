/**
 * [INPUT]: 依赖项目授权设备平台接口与请求取消。
 * [OUTPUT]: 提供项目平台查询、默认平台及原生目标准入判断。
 * [POS]: 构建引导的数据边界；平台只表示设备授权，不表示即时空闲。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import { useEffect, useState } from "react";
import { api } from "@/lib/api";
import { asError } from "@/lib/errors";
import type { BuildTarget } from "@/lib/types";

export type BuildPlatform = {
  os: "linux"|"darwin"|"windows";
  arch: "amd64"|"arm64";
  device_count: number;
  online_count: number;
};
export function hasBuildPlatform(platforms:BuildPlatform[]|undefined,os:string,arch:string) {
  return !!platforms?.some(platform=>platform.os===os&&platform.arch===arch&&platform.device_count>0);
}
export function defaultBuildPlatform(platforms:BuildPlatform[]|undefined) {
  return platforms?.filter(platform=>platform.device_count>0).sort((a,b)=>Number(b.online_count>0)-Number(a.online_count>0)||`${a.os}-${a.arch}`.localeCompare(`${b.os}-${b.arch}`))[0];
}
export function targetsHaveDevices(targets:BuildTarget[],platforms:BuildPlatform[]|undefined) {
  return targets.every(target=>target.kind!=="native"||hasBuildPlatform(platforms,target.os,target.arch));
}
export function useBuildPlatforms(projectID:string) {
  const [revision,setRevision]=useState(0);
  const [snapshot,setSnapshot]=useState<{projectID:string;data?:BuildPlatform[];error?:Error;loading:boolean}>({projectID:"",loading:false});
  useEffect(()=>{
    if(!projectID)return;
    let active=true;const controller=new AbortController();
    setSnapshot({projectID,loading:true});
    void api<BuildPlatform[]>(`/projects/${encodeURIComponent(projectID)}/build-platforms`,{signal:controller.signal})
      .then(data=>{if(active)setSnapshot({projectID,data,loading:false});})
      .catch(error=>{if(active)setSnapshot({projectID,error:asError(error),loading:false});});
    return ()=>{active=false;controller.abort();};
  },[projectID,revision]);
  const current=snapshot.projectID===projectID?snapshot:undefined;
  return {data:current?.data,error:current?.error,loading:current?.loading??Boolean(projectID),reload:()=>setRevision(value=>value+1)};
}
