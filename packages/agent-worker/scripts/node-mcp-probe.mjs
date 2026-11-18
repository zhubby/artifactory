// [INPUT]: 依赖 Runtime 租约和 MCP SDK。
// [OUTPUT]: 输出节点工具查询与过期租约验收结果。
// [POS]: 真实 Runtime 传输的隔离验收脚本。
// [PROTOCOL]: Update this header on change, then check AGENTS.md
import { connectNodeMcp } from "../src/node-mcp.ts";

let input = "";
for await (const chunk of process.stdin) input += chunk;
const task = JSON.parse(input);
const signal = AbortSignal.timeout(20000);
const { runtimeRpc, httpPost } = await import("../dist/transport/runtime.js");
const post = httpPost(task.endpoint, signal);
const rpc = runtimeRpc(task.lease, post, signal);
let session;
try {
  session = await connectNodeMcp(rpc);
  const results = [];
  for (const query of task.queries) {
    const reply = await session.client.callTool(query);
    if (reply.isError) throw new Error(`${query.name} MCP query rejected`);
    results.push(JSON.parse(reply.content[0].text));
  }
  let staleLeaseRejected;
  if (task.stale_lease) {
    const stale = await connectNodeMcp(runtimeRpc(task.stale_lease, post, signal));
    try {
      const result = await stale.client.callTool(task.queries[0]);
      staleLeaseRejected = result.isError === true;
      if (!staleLeaseRejected) throw new Error("stale Runtime lease was accepted");
    } finally { await stale.close(); }
  }
  process.stdout.write(JSON.stringify({ initialized: session.client.getServerVersion()?.name, tools: session.tools.map(tool => tool.name), results, staleLeaseRejected }));
} finally {
  await session?.close();
}
