/**
 * [INPUT]: 依赖 init 源码清单、Pod 身份和 Runtime。
 * [OUTPUT]: 执行单个规划 generation，区分终态与可恢复中断。
 * [POS]: 唯一 Worker 入口；源码准备完成后获取模型时限。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { readFile } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { runTask, type Task } from "./runtime.js";
import { httpPost, runtimeRpc, reportTerminalFailure, type Lease } from "./transport/runtime.js";

const required = (name: string) => { const value = process.env[name]; if (!value) throw new Error(`${name} is required`); return value; };
const controller = new AbortController();
process.on("SIGTERM", () => controller.abort()); process.on("SIGINT", () => controller.abort());
const origin = required("ARTIFACTORY_RUNTIME_URL");
const post = httpPost(origin, controller.signal);
const execution = { run_id: required("ARTIFACTORY_RUN_ID"), generation: Number(required("ARTIFACTORY_RUN_GENERATION")), uid: required("POD_UID"), name: required("POD_NAME"), namespace: required("POD_NAMESPACE") };
let task: Task | undefined;
let lease: Lease | undefined;
let heartbeat: ReturnType<typeof setInterval> | undefined;
try {
  let acquired: Record<string, unknown> | undefined;
  for (let attempt = 0; attempt < 60 && !controller.signal.aborted; attempt++) {
    try { acquired = await post("/v1/acquire", { execution, bootstrap_token: (await readFile(required("ARTIFACTORY_POD_TOKEN"), "utf8")).trim() }); break; }
    catch { await delay(1000, undefined, { signal: controller.signal }); }
  }
  if (!acquired) throw new Error("Pod could not acquire planning lease");
  lease = acquired.lease as Lease;
  task = acquired.task as unknown as Task;
  heartbeat = setInterval(() => { void post("/v1/heartbeat", lease as unknown as Record<string, unknown>).catch(() => controller.abort()); }, 10000);
  await runTask(task, runtimeRpc(lease, post, controller.signal), controller.signal);
} catch (error) {
  const confirmed = task && lease && await reportTerminalFailure(error, task, lease, post, controller.signal);
  // EX_TEMPFAIL 仅用于已领取租约的未知结果；Runtime 对旧身份执行围栏和有限重试。
  console.error("Pi Pod exited before confirmed handoff"); process.exitCode = task && lease && !confirmed ? 75 : 1;
} finally { if (heartbeat) clearInterval(heartbeat); controller.abort(); }
