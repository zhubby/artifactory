/**
 * [INPUT]: 依赖构建鉴权、分页审计消息、骨架和后台轮询。
 * [OUTPUT]: 按 run 与 generation 展示对话，首读骨架与终态补读。
 * [POS]: 对话按需读取，不从审计历史恢复执行。列表错误桥独立于折叠状态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
"use client";
import {useCallback,useEffect,useRef,useState} from "react";
import {api,useRemote} from "@/lib/api";
import {asError,sameError} from "@/lib/errors";
import {useUI} from "@/components/providers";
import {ErrorBox,Loading} from "@/components/common";
import {Button} from "@/components/ui/button";
import type {PlanningConversation,ConversationMessage,ConversationPage} from "@/lib/types";

export function PlanningConversations({buildId,terminal}:{buildId:string;terminal:boolean}) {
 const {t,date}=useUI();const [open,setOpen]=useState(false);
 const sessions=useRemote<{conversations:PlanningConversation[]}>(open?`/builds/${buildId}/planning/conversations`:null,terminal?false:3000);
 const wasTerminal=useRef(terminal);
 useEffect(()=>{if(terminal&&!wasTerminal.current)void sessions.reload({progress:"background"});wasTerminal.current=terminal;},[terminal,sessions.reload]);
 return <details onToggle={event=>setOpen(event.currentTarget.open)} className="rounded-md border border-border p-3">
  <summary className="cursor-pointer text-sm font-medium">{t("ai.conversations")}</summary>
  <ErrorBox message={sessions.error}/>
  {open&&<div className="mt-4 space-y-3"><p className="text-xs text-muted-foreground">{t("ai.conversationHint")}</p>{!sessions.data&&!sessions.error&&<Loading/>}{sessions.data?.conversations.length===0&&<p className="text-sm text-muted-foreground">{t("ai.noConversations")}</p>}{sessions.data?.conversations.map(session=><details key={`${session.run_id}/${session.generation}`} className="rounded border border-border p-3"><summary className="cursor-pointer break-all text-xs">{session.run_id.slice(0,8)} · {t("ai.generation",{generation:session.generation})} · {session.status} · {date(session.started_at)}</summary><Conversation buildId={buildId} session={session}/></details>)}</div>}
 </details>;
}
function combinedMessages(messages:ConversationMessage[]):ConversationMessage[] {
 const result:ConversationMessage[]=[];const chunks=new Map<string,{event:ConversationMessage;parts:Map<number,string>;total:number;kind:string}>();
 for(const message of messages){if(message.kind!=="message_chunk"){result.push(message);continue;}const p=message.payload;const id=String(p.message_id);let entry=chunks.get(id);if(!entry){entry={event:message,parts:new Map(),total:Number(p.total),kind:String(p.kind)};chunks.set(id,entry);result.push(entry.event);}entry.parts.set(Number(p.index),String(p.text));}
 return result.map(message=>{if(message.kind!=="message_chunk")return message;const entry=chunks.get(String(message.payload.message_id))!;const text=[...entry.parts.entries()].sort((a,b)=>a[0]-b[0]).map(([,part])=>part).join("");let payload:Record<string,unknown>={partial:text};if(entry.parts.size===entry.total){try{payload=JSON.parse(text) as Record<string,unknown>;}catch{payload={text};}}return {...message,kind:entry.kind,payload};});
}
function Conversation({buildId,session}:{buildId:string;session:PlanningConversation}) {
 const {t}=useUI();const root=useRef<HTMLDivElement>(null);const [open,setOpen]=useState(false);
 const [messages,setMessages]=useState<ConversationMessage[]>([]);const [next,setNext]=useState<number|null>(null);const [loaded,setLoaded]=useState(false);const [busy,setBusy]=useState(false);const [error,setError]=useState<Error>();const latest=useRef(0);const requests=useRef(new Set<AbortController>());
 const priorStatus=useRef(session.status);
 const path=`/builds/${buildId}/planning/conversations/${session.run_id}/${session.generation}/messages`;
 const load=useCallback(async(after:number,background=false)=>{const controller=new AbortController();requests.current.add(controller);setBusy(true);try{const page=await api<ConversationPage>(`${path}?after=${after}&limit=50`,{signal:controller.signal,progress:background?"background":"foreground"});if(controller.signal.aborted)return;setMessages(previous=>{const merged=new Map(previous.map(message=>[message.sequence,message]));for(const message of page.messages)merged.set(message.sequence,message);return [...merged.values()].sort((a,b)=>a.sequence-b.sequence);});latest.current=Math.max(latest.current,...page.messages.map(message=>message.sequence));setNext(page.next_cursor);setLoaded(true);setError(undefined);}catch(error){if(!controller.signal.aborted)setError(previous=>{const failure=asError(error);return sameError(previous,failure)?previous:failure;});}finally{requests.current.delete(controller);if(!controller.signal.aborted)setBusy(false);}},[path]);
 useEffect(()=>{const details=root.current?.parentElement;if(!details)return;const toggle=()=>setOpen((details as HTMLDetailsElement).open);details.addEventListener("toggle",toggle);return()=>details.removeEventListener("toggle",toggle);},[]);
 useEffect(()=>{if(open&&!loaded)void load(0);},[open,loaded,load]);
 useEffect(()=>{if(!open||!loaded)return;if(priorStatus.current==="running"&&session.status!=="running")void load(latest.current,true);priorStatus.current=session.status;},[open,loaded,session.status,load]);
 useEffect(()=>{if(!open||!loaded||next!==null||session.status!=="running")return;const timer=setInterval(()=>void load(latest.current,true),3000);return()=>clearInterval(timer);},[open,loaded,next,session.status,load]);
 useEffect(()=>{const pending=requests.current;return()=>{for(const request of pending)request.abort();pending.clear();};},[]);
 return <div ref={root} className="mt-3 space-y-3"><p className="break-all font-mono text-xs text-muted-foreground">{session.model} · {session.pod_uid}{session.source_digest&&` · ${session.source_digest}`}</p>{session.status==="interrupted"&&<p className="text-xs text-muted-foreground">{t("ai.conversationInterrupted")}</p>}<ErrorBox message={error}/>{open&&!loaded&&!error&&<Loading/>}{combinedMessages(messages).map(message=><details key={message.sequence} className="rounded bg-background p-3"><summary className="cursor-pointer text-xs">{message.sequence} · {String(message.payload.role||message.kind)}</summary><pre className="mt-2 max-h-96 overflow-auto whitespace-pre-wrap break-all text-xs">{JSON.stringify(message.payload,null,2)}</pre></details>)}{next!==null&&<Button variant="secondary" size="sm" disabled={busy} onClick={()=>void load(next)}>{t("ai.moreMessages")}</Button>}{error&&<Button variant="secondary" size="sm" disabled={busy} onClick={()=>{setError(undefined);void load(latest.current);}}>{t("common.retry")}</Button>}</div>;
}
