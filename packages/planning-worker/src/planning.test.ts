/**
 * [INPUT]: 依赖规划核心与本地 HTTP 模型桩。
 * [OUTPUT]: 验证协议能力、应用分类、工具权限和凭据隔离。
 * [POS]: 规划核心的行为回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { test, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import { runTask, safeError, toolDefinitions, type Task } from "./planning.js";
import type { ModelConfiguration } from "./model-configuration.js";

import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
const manifest={id:"test-manifest",digest:"a".repeat(64),repositories:[]};
let sourceDirectory="";
beforeEach(async()=>{sourceDirectory=await mkdtemp(join(tmpdir(),"planning-"));await mkdir(join(sourceDirectory,"source"));await writeFile(join(sourceDirectory,"manifest.json"),JSON.stringify(manifest));process.env.ARTIFACTORY_SOURCE_ROOT=join(sourceDirectory,"source");process.env.ARTIFACTORY_SOURCE_METADATA=join(sourceDirectory,"manifest.json");});
afterEach(async()=>{await rm(sourceDirectory,{recursive:true,force:true});delete process.env.ARTIFACTORY_SOURCE_ROOT;delete process.env.ARTIFACTORY_SOURCE_METADATA;});

test("worker exposes only read-only analysis and structured completion tools", () => {
  assert.deepEqual(toolDefinitions.map((tool) => tool.name), ["list_tree", "read_file", "search_source", "git_metadata", "node_capabilities", "read_logs", "submit_plan", "submit_placements", "request_input"]);
  assert.equal(safeError(new Error("401 secret-value"), "secret-value"), "model authentication rejected; check the platform model profile");
  assert.equal(safeError(new Error('429: organization org-c401 request reached max RPM'), "secret-value"), "model provider quota or rate limit reached");
  assert.equal(safeError(new Error("request with key secret-value failed"), "secret-value").includes("secret-value"), false);
});

test("plan submission separates application platforms from build hosts", () => {
  const submit = toolDefinitions.find((tool) => tool.name === "submit_plan");
  assert.ok(submit);
  const schema = (value: unknown, ...keys: string[]): unknown => {
    for (const key of keys) {
      assert.ok(value && typeof value === "object");
      value = (value as Record<string, unknown>)[key];
    }
    return value;
  };
  const target = schema(submit.parameters, "properties", "plan", "properties", "recipe", "properties", "targets", "additionalProperties");
  assert.deepEqual(schema(target, "properties", "application_platform", "enum"), ["macos", "ios", "android", "linux", "windows", null]);
  assert.deepEqual(schema(target, "properties", "application_platform", "type"), ["string", "null"]);
  const required = schema(target, "required");
  assert.ok(Array.isArray(required) && required.includes("application_platform"));
});

test("provider timeouts remain distinct from the planning deadline", () => {
  assert.equal(safeError(new Error("planning deadline reached"), "task-secret"), "planning deadline reached");
  for (const message of ["Request timed out.", "UND_ERR_CONNECT_TIMEOUT", "connect ETIMEDOUT", "model provider deadline exceeded"]) {
    assert.equal(safeError(new Error(message), "task-secret"), "model provider request timed out; check worker connectivity and the model endpoint");
  }
});

test("Pi uses task key only for authentication and terminates after clarification", async () => {
  let requests = 0;
  const server = createServer(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk;
    assert.equal(body.includes("task-secret"), false);
    assert.equal(request.headers["x-api-key"], "task-secret");
    requests++;
    response.writeHead(200, { "content-type": "text/event-stream" });
    const event = (name: string, data: unknown) => response.write(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
    event("message_start", { type: "message_start", message: { id: "test", type: "message", role: "assistant", model: "test-model", content: [], usage: { input_tokens: 10, output_tokens: 0 } } });
    event("content_block_start", { type: "content_block_start", index: 0, content_block: { type: "tool_use", id: "question-1", name: "request_input", input: {} } });
    event("content_block_delta", { type: "content_block_delta", index: 0, delta: { type: "input_json_delta", partial_json: JSON.stringify({ question: "Which product?" }) } });
    event("content_block_stop", { type: "content_block_stop", index: 0 });
    event("message_delta", { type: "message_delta", delta: { stop_reason: "tool_use" }, usage: { output_tokens: 15 } });
    event("message_stop", { type: "message_stop" });
    response.end();
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address(); assert.ok(address && typeof address === "object");
  const task: Task = { context: { source_manifest:manifest, goal: "Build the selected product", usage: {totalTokens: 100} }, model: { api: "anthropic-messages", base_url: `http://127.0.0.1:${address.port}`, model: "test-model", api_key: "task-secret", parameters: { max_tokens: 1000 } }, timeout_seconds: 10, max_calls: 2 };
  let questions = 0; let totalTokens = 0; const messages:Record<string,unknown>[]=[];
  try {
    await runTask(task, async (method, payload) => {
      assert.equal(JSON.stringify(payload).includes("task-secret"), false);
      if (method === "report" && payload.kind === "conversation") messages.push(JSON.parse(String(payload.payload_json)));
      if (method === "report" && payload.kind === "usage") totalTokens = JSON.parse(String(payload.payload_json)).totalTokens;
      if (method === "tool") { assert.equal(payload.name, "request_input"); questions++; return { accepted: true, needs_input: true }; }
      return { ok: true };
    }, new AbortController().signal);
    assert.equal(requests, 1); assert.equal(questions, 1);
    assert.equal(messages[0]?.kind,"system");
    assert.ok(messages.some(value=>(value.content as Record<string,unknown>)?.role==="user"));
    assert.ok(messages.some(value=>(value.content as Record<string,unknown>)?.role==="assistant"));
    assert.ok(totalTokens > 100, "resumed usage must include the previous worker's tokens");
  } finally { server.close(); server.closeAllConnections(); }
});

test("Chat Completions request fields follow explicit capabilities for any model name", async (t) => {
  const fixtures: {
    model: string; level: string;
    compat?: NonNullable<ModelConfiguration["parameters"]["compat"]>;
    thinking?: boolean; nestedThinking?: string; effort?: string; role?: string;
  }[] = [
    { model: "qwen3.8-max", level: "off" },
    { model: "qwen4-max", level: "medium" },
    { model: "arbitrary-private-model", level: "high" },
    { model: "qwen4-max", level: "medium", compat: { thinking_format: "enable_thinking" }, thinking: true },
    { model: "renamed-model", level: "off", compat: { thinking_format: "enable_thinking" }, thinking: false },
    { model: "any-model", level: "medium", compat: { thinking_format: "thinking" }, nestedThinking: "enabled" },
    { model: "any-model", level: "off", compat: { thinking_format: "thinking" }, nestedThinking: "disabled" },
    { model: "any-model", level: "medium", compat: { system_role: "system", thinking_format: "reasoning_effort", max_tokens_field: "max_completion_tokens" }, effort: "medium" },
    { model: "any-model", level: "off", compat: { system_role: "developer" }, role: "developer" },
    { model: "any-model", level: "off", compat: { thinking_format: "reasoning_effort" } },
  ];
  for (const [index,fixture] of fixtures.entries()) await t.test(`${index}: ${fixture.model} ${fixture.level}`, async () => {
    let sent: Record<string, unknown> | undefined;
    const server = createServer(async (request, response) => {
      let body = "";
      for await (const chunk of request) body += chunk;
      sent = JSON.parse(body);
      response.writeHead(200, { "content-type": "text/event-stream" });
      const chunk = (delta: unknown, finish_reason: string | null = null) => response.write(`data: ${JSON.stringify({ id: "compat-test", object: "chat.completion.chunk", created: 1, model: fixture.model, choices: [{ index: 0, delta, finish_reason }] })}\n\n`);
      chunk({ role: "assistant", tool_calls: [{ index: 0, id: "question-1", type: "function", function: { name: "request_input", arguments: JSON.stringify({ question: "Which product?" }) } }] });
      chunk({}, "tool_calls");
      response.end("data: [DONE]\n\n");
    });
    server.listen(0, "127.0.0.1"); await once(server, "listening");
    const address = server.address(); assert.ok(address && typeof address === "object");
    const task: Task = { context: { source_manifest:manifest, goal: "Build the selected product" }, model: { api: "openai-completions", base_url: `http://127.0.0.1:${address.port}/v1`, model: fixture.model, api_key: "task-secret", parameters: { thinking_level: fixture.level, ...(fixture.compat ? {compat:fixture.compat} : {}) } }, timeout_seconds: 10, max_calls: 1 };
    try {
      await runTask(task, async (method, payload) => {
        if (method === "tool") { assert.equal(payload.name, "request_input"); return { accepted: true, needs_input: true }; }
        return { ok: true };
      }, new AbortController().signal);
      assert.ok(sent);
      assert.equal(sent.model,fixture.model);
      assert.equal(sent.enable_thinking, fixture.thinking);
      assert.deepEqual(sent.thinking, fixture.nestedThinking ? {type:fixture.nestedThinking} : undefined);
      assert.equal(sent.reasoning_effort, fixture.effort);
      assert.equal((sent.messages as { role: string }[])[0]?.role, fixture.role || "system");
      const tokenField=fixture.compat?.max_tokens_field || "max_tokens";
      assert.equal(sent[tokenField],8192);
      assert.equal(sent[tokenField === "max_tokens" ? "max_completion_tokens" : "max_tokens"],undefined);
      assert.equal(sent.store,undefined);
      assert.equal(JSON.stringify(sent).includes("task-secret"), false);
    } finally { server.close(); server.closeAllConnections(); }
  });
});

test("Chat Completions does not infer second-turn fields from endpoint names", async (t) => {
  for (const endpoint of ["generic", "deepseek.com", "openrouter.ai"]) await t.test(endpoint, async () => {
    const requests: Record<string, unknown>[] = [];
    const server = createServer(async (request, response) => {
      let body = "";
      for await (const chunk of request) body += chunk;
      requests.push(JSON.parse(body));
      response.writeHead(200, { "content-type": "text/event-stream" });
      const first = requests.length === 1;
      const delta = first ? { role: "assistant", content: "I will inspect this source." } : {
        role: "assistant", tool_calls: [{ index: 0, id: "question", type: "function", function: { name: "request_input", arguments: JSON.stringify({ question: "Which product?" }) } }],
      };
      for (const [value, finish] of [[delta, null], [{}, first ? "stop" : "tool_calls"]]) {
        response.write(`data: ${JSON.stringify({ id: "endpoint-test", object: "chat.completion.chunk", created: 1, model: "private-alias", choices: [{ index: 0, delta: value, finish_reason: finish }] })}\n\n`);
      }
      response.end("data: [DONE]\n\n");
    });
    server.listen(0, "127.0.0.1"); await once(server, "listening");
    const address = server.address(); assert.ok(address && typeof address === "object");
    const task: Task = { context: { source_manifest: manifest, goal: "Build this product" }, model: { api: "openai-completions", base_url: `http://127.0.0.1:${address.port}/${endpoint}/v1`, model: "private-alias", api_key: "task-secret", parameters: { thinking_level: "medium", compat: { thinking_format: "reasoning_effort" } } }, timeout_seconds: 10, max_calls: 2 };
    try {
      await runTask(task, async (method) => method === "tool" ? { accepted: true, needs_input: true } : { ok: true }, new AbortController().signal);
      assert.equal(requests.length, 2);
      const next = requests[1]; assert.ok(next);
      const history = (next.messages as Record<string, unknown>[]).find(message => message.role === "assistant");
      assert.ok(history);
      assert.equal(history.reasoning_content, undefined);
      assert.equal(next.reasoning_effort, "medium");
      assert.equal(next.prompt_cache_key, undefined);
      assert.equal(next.prompt_cache_retention, undefined);
    } finally { server.close(); server.closeAllConnections(); }
  });
});

test("planning deadline interrupts an awaited rate-limit delay", async () => {
  let requests = 0;
  const server = createServer(async (request, response) => {
    for await (const _ of request) { /* consume request */ }
    requests++;
    response.writeHead(200, {"content-type": "text/event-stream"});
    const event = (name: string, data: unknown) => response.write(`event: ${name}\ndata: ${JSON.stringify(data)}\n\n`);
    event("message_start", {type:"message_start", message:{id:"loop", type:"message",role:"assistant",model:"test-model",content:[],usage:{input_tokens:1,output_tokens:0}}});
    event("content_block_start", {type:"content_block_start",index:0,content_block:{type:"text",text:""}});
    event("content_block_delta", {type:"content_block_delta",index:0,delta:{type:"text_delta",text:"I will inspect the repository."}});
    event("content_block_stop", {type:"content_block_stop",index:0});
    event("message_delta", {type:"message_delta",delta:{stop_reason:"end_turn"},usage:{output_tokens:5}});
    event("message_stop", {type:"message_stop"});response.end();
  });
  server.listen(0,"127.0.0.1");await once(server,"listening");
  const address=server.address();assert.ok(address && typeof address === "object");
  const task:Task={context:{source_manifest:manifest,goal:"Inspect and build"},model:{api:"anthropic-messages",base_url:`http://127.0.0.1:${address.port}`,model:"test-model",api_key:"test-secret",parameters:{requests_per_minute:1}},timeout_seconds:1,max_calls:20};
  const started=Date.now();
  try {
    await assert.rejects(runTask(task,async()=>({ok:true}),new AbortController().signal),/deadline/);
    assert.ok(Date.now()-started<3000,"deadline must not wait for the 60-second call throttle");
    assert.equal(requests,1);
  } finally {server.close();server.closeAllConnections();}
});

test("placement inference resumes and corrects prose using only placement completion tools", async () => {
  const node = "00000000-0000-0000-0000-000000000001";
  const placements = { app: { candidates: [{ node_id: node, metadata_digest: "a".repeat(64), reason: "native toolchain and available resources" }], resources: { min_available_memory_bytes: 4096 } } };
  let requests = 0;
  const reminders: string[] = [];
  const server = createServer(async (request, response) => {
    let body = "";
    for await (const chunk of request) body += chunk;
    const sent = JSON.parse(body);
    const names = sent.tools.map((tool: { function: { name: string } }) => tool.function.name);
    assert.ok(names.includes("query_nodes") && names.includes("submit_placements"));
    for (const forbidden of ["submit_plan", "request_input", "read_file"]) assert.ok(!names.includes(forbidden));
    requests++;
    if (requests <= 2) reminders.push(JSON.stringify(sent.messages.findLast((message: { role: string }) => message.role === "user").content));
    response.writeHead(200, { "content-type": "text/event-stream" });
    const chunk = (delta: unknown, finish_reason: string | null = null) => response.write(`data: ${JSON.stringify({ id: "placement-test", object: "chat.completion.chunk", created: 1, model: "gpt-4.1", choices: [{ index: 0, delta, finish_reason }] })}\n\n`);
    if (requests === 1) {
      chunk({ role: "assistant", content: "I will find suitable replacement devices." });
      chunk({}, "stop");
      response.end("data: [DONE]\n\n");
      return;
    }
    const name = requests === 2 ? "query_nodes" : "submit_placements";
    const args = requests === 2 ? { os: "linux", tools: ["cargo"] } : { placements };
    chunk({ role: "assistant", tool_calls: [{ index: 0, id: `tool-${requests}`, type: "function", function: { name, arguments: JSON.stringify(args) } }] });
    chunk({}, "tool_calls");
    response.end("data: [DONE]\n\n");
  });
  server.listen(0, "127.0.0.1"); await once(server, "listening");
  const address = server.address(); assert.ok(address && typeof address === "object");
  const task: Task = { context: { purpose: "placement", placement_task: { targets: { app: { version: 1 } }, current_placements: placements } }, model: { api: "openai-completions", base_url: `http://127.0.0.1:${address.port}/v1`, model: "gpt-4.1", api_key: "test-secret", parameters: {} }, timeout_seconds: 10, max_calls: 5 };
  const calls: string[] = [];
  try {
    await runTask(task, async (method, payload) => {
      if (method !== "tool") return { ok: true };
      calls.push(String(payload.name));
      if (payload.name === "query_nodes") return { nodes: [{ node_id: node, metadata_digest: "a".repeat(64), resources_fresh: true }] };
      assert.equal(payload.name, "submit_placements");
      assert.deepEqual(JSON.parse(String(payload.arguments_json)), { placements });
      return { accepted: true };
    }, new AbortController().signal);
    assert.deepEqual(calls, ["query_nodes", "submit_placements"]);
    assert.equal(requests, 3);
    assert.equal(reminders.length, 2);
    for (const reminder of reminders.slice(1)) {
      assert.match(reminder, /submit_placements/);
      assert.doesNotMatch(reminder, /submit_plan\b|request_input|source read/);
    }
  } finally { server.close(); server.closeAllConnections(); }
});
