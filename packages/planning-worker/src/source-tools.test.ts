/**
 * [INPUT]: 依赖临时只读源码工具与文件系统夹具。
 * [OUTPUT]: 验证行范围、搜索续页与路径、符号链接、二进制和字节预算。
 * [POS]: 本地源码读取的安全回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,mkdir,writeFile,symlink,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {sourceTools} from "./source-tools.js";
test("source tools read local files and reject host traversal, symlinks and oversized data",async()=>{
 const root=await mkdtemp(join(tmpdir(),"source-tools-"));
 try{await mkdir(join(root,"src"));await mkdir(join(root,".git"));await writeFile(join(root,"src/main.rs"),'fn main() { println!("hello"); }');await writeFile(join(root,".git/config"),"secret-config");await writeFile(join(root,"big.txt"),"x".repeat(128*1024+1));await writeFile(join(root,"binary"),Buffer.from([0,1]));await symlink("/etc/passwd",join(root,"outside"));
 const tool=await sourceTools(root,{id:"fixed",digest:"a".repeat(64),repositories:[{path:"",upstream_url:"https://git.example/repo",commit_sha:"b".repeat(40),tags:{},lfs_objects:[]}]});
 const read=await tool("read_file",{path:"src/main.rs"});assert.match(String(read.content),/hello/);
 for(const path of ["../etc/passwd","/etc/passwd",".git/config","outside","big.txt","binary","src/../src/main.rs"])await assert.rejects(tool("read_file",{path}));
 const tree=await tool("list_tree",{});assert.equal(JSON.stringify(tree).includes(".git"),false);assert.equal(JSON.stringify(tree).includes("outside"),false);
 const result=await tool("search_source",{query:"hello"});assert.equal((result.matches as unknown[]).length,1);assert.equal((await tool("git_metadata",{})).commit,"b".repeat(40));
 }finally{await rm(root,{recursive:true,force:true});}
});

const manifest={id:"fixed",digest:"a".repeat(64),repositories:[{path:"",upstream_url:"https://git.example/repo",commit_sha:"b".repeat(40),tags:{},lfs_objects:[]}]};
async function fixture(run:(root:string)=>Promise<void>) {
 const root=await mkdtemp(join(tmpdir(),"source-tools-page-"));
 try{await run(root);}finally{await rm(root,{recursive:true,force:true});}
}

test("read_file ranges preserve full-read compatibility and exact line boundaries",async()=>fixture(async root=>{
 await writeFile(join(root,"lines.txt"),"first\r\nsecond\r\nthird\nfourth\n");
 const tool=await sourceTools(root,manifest);
 assert.deepEqual(await tool("read_file",{path:"lines.txt"}),{path:"lines.txt",content:"first\r\nsecond\r\nthird\nfourth\n"});
 assert.deepEqual(await tool("read_file",{path:"lines.txt",offset:1,limit:2}),{
  path:"lines.txt",content:"second\r\nthird\n",offset:1,total_lines:4,next_offset:3,truncated:true,
 });
 assert.deepEqual(await tool("read_file",{path:"lines.txt",offset:3,limit:2}),{
  path:"lines.txt",content:"fourth\n",offset:3,total_lines:4,next_offset:null,truncated:false,
 });
 assert.deepEqual(await tool("read_file",{path:"lines.txt",offset:20,limit:2}),{
  path:"lines.txt",content:"",offset:20,total_lines:4,next_offset:null,truncated:false,
 });
 assert.equal((await tool("read_file",{path:"lines.txt",limit:1})).content,"first\r\n");
}));

test("read_file ranges reject invalid numbers and cannot bypass file safety",async()=>fixture(async root=>{
 await writeFile(join(root,"valid.txt"),"line\n");
 await writeFile(join(root,"large.txt"),"x".repeat(128*1024+1));
 await writeFile(join(root,"binary"),Buffer.from([0,1]));
 await symlink("/etc/passwd",join(root,"outside"));
 const tool=await sourceTools(root,manifest);
 for(const args of [{offset:-1},{offset:0.5},{offset:"0"},{offset:Number.MAX_SAFE_INTEGER+1},{limit:0},{limit:-1},{limit:1.5},{limit:"1"}])
  await assert.rejects(tool("read_file",{path:"valid.txt",...args}));
 for(const path of ["../etc/passwd","/etc/passwd","outside","large.txt","binary"])
  await assert.rejects(tool("read_file",{path,offset:0,limit:1}));
}));

test("read_file handles empty files and defaults ranged reads to a bounded page",async()=>fixture(async root=>{
 await writeFile(join(root,"empty"),"");
 await writeFile(join(root,"many"),Array.from({length:202},(_,i)=>String(i+1)).join("\n"));
 const tool=await sourceTools(root,manifest);
 assert.deepEqual(await tool("read_file",{path:"empty",offset:0,limit:1}),{
  path:"empty",content:"",offset:0,total_lines:0,next_offset:null,truncated:false,
 });
 const result=await tool("read_file",{path:"many",offset:1});
 assert.equal(String(result.content).split("\n")[0],"2");
 assert.equal(result.next_offset,201);assert.equal(result.truncated,true);
 assert.equal((await tool("read_file",{path:"many",offset:201})).content,"202");
}));

test("search_source scopes exact files and directory prefixes without traversal",async()=>fixture(async root=>{
 await mkdir(join(root,"src"));await mkdir(join(root,"src-old"));await mkdir(join(root,".git"));
 await writeFile(join(root,"src/main.ts"),"needle\n");await writeFile(join(root,"src-old/main.ts"),"needle\n");
 await writeFile(join(root,".git/config"),"needle secret");await symlink("/etc",join(root,"outside"));
 const tool=await sourceTools(root,manifest);
 for(const path of ["src","src/main.ts"]){
  const result=await tool("search_source",{query:"needle",path});
  assert.deepEqual(result,{matches:[{path:"src/main.ts",line:1,text:"needle"}],truncated:false,next_offset:null});
 }
 for(const path of ["../src","/etc",".git/config","src/../src"])
  await assert.rejects(tool("search_source",{query:"needle",path}));
 assert.deepEqual((await tool("search_source",{query:"needle",path:"outside"})).matches,[]);
}));

test("search_source resumes within one file without repeating or losing matches",async()=>fixture(async root=>{
 await writeFile(join(root,"hits.txt"),Array.from({length:202},(_,i)=>`needle ${i+1}`).join("\n"));
 const tool=await sourceTools(root,manifest);
 const first=await tool("search_source",{query:"needle"});
 assert.equal((first.matches as unknown[]).length,200);assert.equal(first.truncated,true);
 assert.equal(typeof first.next_offset,"number");
 const second=await tool("search_source",{query:"needle",offset:first.next_offset});
 assert.deepEqual(second,{matches:[{path:"hits.txt",line:201,text:"needle 201"},{path:"hits.txt",line:202,text:"needle 202"}],truncated:false,next_offset:null});
}));

test("search_source exposes a continuation when the byte budget ends before any match",async()=>fixture(async root=>{
 const content="a".repeat(128*1024);
 for(let i=0;i<129;i++)await writeFile(join(root,`a-${String(i).padStart(3,"0")}.txt`),content);
 await writeFile(join(root,"z-target.txt"),"needle\n");
 const tool=await sourceTools(root,manifest);
 const first=await tool("search_source",{query:"needle"});
 assert.deepEqual(first.matches,[]);assert.equal(first.truncated,true);
 assert.equal(typeof first.next_offset,"number");assert.ok(Number(first.next_offset)>0);
 const second=await tool("search_source",{query:"needle",offset:first.next_offset});
 assert.deepEqual(second,{matches:[{path:"z-target.txt",line:1,text:"needle"}],truncated:false,next_offset:null});
 assert.deepEqual(await tool("search_source",{query:"needle",path:"z-target.txt"}),second);
}));

test("search_source rejects invalid continuation values",async()=>fixture(async root=>{
 await writeFile(join(root,"main.txt"),"needle");const tool=await sourceTools(root,manifest);
 for(const offset of [-1,0.5,"0",Number.MAX_SAFE_INTEGER+1,NaN])
  await assert.rejects(tool("search_source",{query:"needle",offset}));
}));
