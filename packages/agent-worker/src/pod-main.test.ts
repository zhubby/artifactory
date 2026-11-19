/**
 * [INPUT]: 依赖 Runtime 租约传输和失败报告。
 * [OUTPUT]: 验证提案、审计幂等和响应字节上限。
 * [POS]: Kubernetes Worker 传输回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import test from "node:test";
import assert from "node:assert/strict";
import { httpPost, runtimeRpc, reportTerminalFailure } from "./transport/runtime.js";
import { type Task } from "./runtime.js";
import { createServer } from "node:http";
import { once } from "node:events";
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

test("the real Pod process signals an unconfirmed interruption after acquiring its lease", async () => {
  const dir = await mkdtemp(join(tmpdir(), "artifactory-pod-test-"));
  const token = join(dir, "token");
  await writeFile(token, "test");
  const server = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ lease: { run_id: "run", generation: 1, uid: "pod", token: "grant" }, task: { model: { api: "openai-completions", base_url: "http://127.0.0.1", model: "test", api_key: "grant", parameters: {} } } }));
  });
  server.listen(0, "127.0.0.1"); await once(server, "listening");
  const address = server.address(); assert.ok(address && typeof address === "object");
  try {
    const child = spawn(process.execPath, [fileURLToPath(new URL("./pod-main.js", import.meta.url))], { env: { ...process.env, ARTIFACTORY_RUNTIME_URL: `http://127.0.0.1:${address.port}`, ARTIFACTORY_RUN_ID: "run", ARTIFACTORY_RUN_GENERATION: "1", POD_UID: "pod", POD_NAME: "pod", POD_NAMESPACE: "test", ARTIFACTORY_POD_TOKEN: token }, stdio: "ignore" });
    const [code, signal] = await once(child, "exit");
    assert.equal(signal, null); assert.equal(code, 75);
  } finally {
    await new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
    await rm(dir, { recursive: true, force: true });
  }
});

test("a lost submission acknowledgement queries the same proposal without new inference", async () => {
  const requests: string[] = [];
  let proposal = "";
  const post = async (path: string, body: Record<string, unknown>) => {
    requests.push(path);
    if (path === "/v1/tool") { proposal = String(body.request_id); throw new TypeError("connection lost after commit"); }
    assert.equal(body.proposal_id, proposal);
    return { accepted: true, plan_revision_id: "original" };
  };
  const rpc = runtimeRpc({ run_id: "run", generation: 1, uid: "pod", token: "grant" }, post, new AbortController().signal);
  const result = await rpc("tool", { name: "submit_plan", arguments_json: '{"plan":{}}' });
  assert.equal(result.plan_revision_id, "original");
  assert.deepEqual(requests, ["/v1/tool", "/v1/proposal"]);
});

test("a disconnected Pod leaves recovery to Runtime but explicit budget failure is terminal", async () => {
  const task: Task = { context: {}, model: { api: "openai-completions", base_url: "https://runtime.invalid", model: "test", api_key: "private-grant", parameters: {} }, max_calls: 20, timeout_seconds: 600 };
  const lease = { run_id: "run", generation: 1, uid: "pod", token: "private-grant" };
  const events: Record<string, unknown>[] = [];
  const post = async (_path: string, body: Record<string, unknown>) => { events.push(body); return { ok: true }; };
  const signal = new AbortController().signal;
  for (const error of [new TypeError("fetch failed"), new Error("runtime request rejected (503)"), new Error("request timed out"), new Error("socket connection lost")]) {
    assert.equal(await reportTerminalFailure(error, task, lease, post, signal), false);
  }
  assert.equal(events.length, 0, "transport failure must not permanently fail the logical run");
  assert.equal(await reportTerminalFailure(new Error("model call limit reached"), task, lease, post, signal), true);
  assert.equal(events.length, 1);
  assert.equal(events[0]?.kind, "failed");
  assert.deepEqual(events[0]?.payload, { error: "model call limit reached" });
  await reportTerminalFailure(new Error("planning deadline reached"), task, lease, post, signal);
  assert.deepEqual(events[1]?.payload, { error: "planning deadline reached" });
  await reportTerminalFailure(new Error("401 private-grant"), task, lease, post, signal);
  assert.deepEqual(events[2]?.payload, { error: "model authentication rejected; check the platform model profile" });
  assert.equal(await reportTerminalFailure(new Error("401"), task, lease, async () => { throw new Error("connection lost"); }, signal), false);
  const aborted = new AbortController(); aborted.abort();
  assert.equal(await reportTerminalFailure(new Error("401"), task, lease, post, aborted.signal), false);
});

test("audit reports retry the same durable event identity",async()=>{
 const events:Record<string,unknown>[]=[];
 const rpc=runtimeRpc({run_id:"run",generation:2,uid:"pod",token:"grant"},async(_path,body)=>{events.push(body);if(events.length===1)throw new Error("ack lost");return {ok:true};},new AbortController().signal);
 await rpc("report",{kind:"conversation",payload_json:JSON.stringify({kind:"message",content:{role:"user",content:"source excerpt"}})});
 assert.equal(events.length,2);assert.deepEqual(events[0],events[1]);
});

test("Runtime replies accept large manifests and enforce the UTF-8 byte limit",async()=>{
  let content="源".repeat(1_000_000);
  const server=createServer((_request,response)=>{
    response.writeHead(200,{"content-type":"application/json"});
    response.end(JSON.stringify({source_manifest:{content}}));
  });
  server.listen(0,"127.0.0.1");
  await once(server,"listening");
  const address=server.address();assert.ok(address&&typeof address==="object");
  const post=httpPost(`http://127.0.0.1:${address.port}`,new AbortController().signal);
  try {
    assert.deepEqual(await post("/v1/acquire",{}),{source_manifest:{content}});
    content="源".repeat(3_000_000);
    await assert.rejects(post("/v1/acquire",{}),/runtime reply exceeds limit/);
  }finally{await new Promise<void>((resolve,reject)=>server.close(error=>error?reject(error):resolve()));}
});
