/**
 * [INPUT]: 依赖临时只读源码工具与文件系统夹具。
 * [OUTPUT]: 验证路径、符号链接、二进制和查询大小边界。
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
