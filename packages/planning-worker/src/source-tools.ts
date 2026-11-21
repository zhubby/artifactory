/**
 * [INPUT]: 依赖 init 准备的只读源码目录和固定清单。
 * [OUTPUT]: 提供行范围读取、目录范围搜索与有界续页。
 * [POS]: 模型只读取任务目录；源码查询不经过平台 RPC。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { constants } from "node:fs";
import { lstat, open, readdir, readFile } from "node:fs/promises";
import { resolve, join, relative } from "node:path";

export interface SourceManifest {
  id: string; digest: string;
  repositories: { path: string; upstream_url: string; commit_sha: string; tags: Record<string,string>; lfs_objects: {path:string;oid:string;size:number}[] }[];
}
const MAX_FILE = 128 * 1024;
const PAGE = 200;
const MAX_SEARCH_BYTES = 16 * 1024 * 1024;
// 文件最多 MAX_FILE+1 行；整数游标同时保留文件和行，零命中也能续页。
const SEARCH_STRIDE = MAX_FILE + 1;
const SOURCE_NAMES = new Set(["list_tree", "read_file", "search_source", "git_metadata"]);
export const isSourceTool = (name:string) => SOURCE_NAMES.has(name);
function safePath(path:string):string {
  if (path === "") return path;
  if (path.startsWith("/") || path.includes("\\") || path.includes("\0") || path.split("/").some(part=>!part || part === "." || part === ".." || part === ".git")) throw new Error("source path rejected");
  return path;
}
function pageNumber(value:unknown,fallback:number,name:string,minimum=0):number {
  if(value===undefined)return fallback;
  if(typeof value!=="number"||!Number.isSafeInteger(value)||value<minimum)throw new Error(`invalid source ${name}`);
  return value;
}
export async function sourceTools(root:string, manifest:SourceManifest) {
  const base=resolve(root);
  const files:{path:string;size:number}[]=[];
  async function walk(directory:string):Promise<void> {
    for(const entry of await readdir(directory,{withFileTypes:true})) {
      if(entry.name === ".git" || entry.isSymbolicLink()) continue;
      const target=join(directory,entry.name);
      if(entry.isDirectory()) await walk(target);
      else if(entry.isFile()) { const stat=await lstat(target);files.push({path:relative(base,target).split("\\").join("/"),size:stat.size});if(files.length>100000) throw new Error("source tree exceeds inspection limit"); }
    }
  }
  await walk(base);files.sort((a,b)=>a.path.localeCompare(b.path,"en"));
  const known=new Map(files.map(file=>[file.path,file]));
  function scopedFiles(path:string) {
    const prefix=safePath(path);
    return files.filter(file=>!prefix||file.path===prefix||file.path.startsWith(prefix+"/"));
  }
  async function read(path:string):Promise<string> {
    safePath(path);const file=known.get(path);if(!file)throw new Error("source file not found");if(file.size>MAX_FILE)throw new Error("source file exceeds read limit");
    const parts=path.split("/");let directory=base;
    for(const part of parts){directory=join(directory,part);if((await lstat(directory)).isSymbolicLink())throw new Error("source symlink rejected");}
    const handle=await open(join(base,path),constants.O_RDONLY|constants.O_NOFOLLOW);
    try {const stat=await handle.stat();if(!stat.isFile()||stat.size>MAX_FILE)throw new Error("source file exceeds read limit");const bytes=await handle.readFile();if(bytes.includes(0))throw new Error("binary source file rejected");return new TextDecoder("utf-8",{fatal:true}).decode(bytes);} finally {await handle.close();}
  }
  return async(name:string,args:Record<string,unknown>):Promise<Record<string,unknown>>=>{
    if(name === "git_metadata") return {manifest_id:manifest.id,digest:manifest.digest,commit:manifest.repositories.find(repo=>repo.path === "")?.commit_sha,repositories:manifest.repositories};
    if(name === "read_file") {
      const path=String(args.path??"");
      if(args.offset===undefined&&args.limit===undefined)return {path,content:await read(path)};
      const offset=pageNumber(args.offset,0,"offset");
      const limit=pageNumber(args.limit,PAGE,"limit",1);
      const lines=(await read(path)).match(/[^\n]*\n|[^\n]+$/g)??[];
      const end=Math.min(offset+limit,lines.length);
      const next_offset=end<lines.length?end:null;
      return {path,content:lines.slice(offset,end).join(""),offset,total_lines:lines.length,next_offset,truncated:next_offset!==null};
    }
    if(name === "list_tree") {const selected=scopedFiles(String(args.path??""));const offset=Number(args.offset??0);if(!Number.isSafeInteger(offset)||offset<0)throw new Error("invalid source offset");return {files:selected.slice(offset,offset+PAGE),next_offset:offset+PAGE<selected.length?offset+PAGE:null};}
    if(name === "search_source") {
      const query=String(args.query??"");if(!query||query.length>1000)throw new Error("invalid source query");
      const selected=scopedFiles(String(args.path??""));
      const offset=pageNumber(args.offset,0,"offset");
      if(offset>selected.length*SEARCH_STRIDE)throw new Error("invalid source offset");
      const firstFile=Math.floor(offset/SEARCH_STRIDE),firstLine=offset%SEARCH_STRIDE;
      const matches:{path:string;line:number;text:string}[]=[];let bytes=0;
      for(let fileIndex=firstFile;fileIndex<selected.length;fileIndex++) {
        const file=selected[fileIndex]!;if(file.size>MAX_FILE)continue;
        if(bytes+file.size>MAX_SEARCH_BYTES)return {matches,truncated:true,next_offset:fileIndex*SEARCH_STRIDE};
        bytes+=file.size;let content:string;try{content=await read(file.path);}catch{continue;}
        const lines=content.split("\n");
        for(let index=fileIndex===firstFile?firstLine:0;index<lines.length;index++) {
          if(!lines[index]!.includes(query))continue;
          matches.push({path:file.path,line:index+1,text:lines[index]!.slice(0,1000)});
          if(matches.length>=PAGE) {
            const next=index+1<lines.length?fileIndex*SEARCH_STRIDE+index+1:(fileIndex+1)*SEARCH_STRIDE;
            const next_offset=next<selected.length*SEARCH_STRIDE?next:null;
            return {matches,truncated:next_offset!==null,next_offset};
          }
        }
      }
      return {matches,truncated:false,next_offset:null};
    }
    throw new Error("unknown local source tool");
  };
}
export async function taskSource(context:Record<string,unknown>) {
  const root=process.env.ARTIFACTORY_SOURCE_ROOT;
  const path=process.env.ARTIFACTORY_SOURCE_METADATA;
  if(!root||!path)throw new Error("prepared source workspace is required");
  const manifest=JSON.parse(await readFile(path,"utf8")) as SourceManifest;
  const expected=context.source_manifest as SourceManifest|undefined;
  if(!expected||manifest.id!==expected.id||manifest.digest!==expected.digest)throw new Error("prepared source identity mismatch");
  return sourceTools(root,manifest);
}
