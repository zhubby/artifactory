/**
 * [INPUT]: 依赖节点HTTP夹具、真实组件和浏览器交互。
 * [OUTPUT]: 验证队列进展、清单冲突、采样裁剪、未知与零值、OTA身份和请求恢复。
 * [POS]: Emissary演进的Web回归；不代替设备和服务验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, nodes, tenantURL } from "./fixtures";
import type { Node } from "../../src/lib/types";
import type { NodeOperation } from "../../src/lib/node-operation-types";
async function setup(page: Page, node: Node, baseURL?: string, role: "admin" | "viewer" = "admin") {
  await fixtureAPI(page, role);
  await page.context().addCookies([{ name: "artifactory-locale", value: "en", url: baseURL || "http://127.0.0.1:3100" }]);
  await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [node] }));
}
async function operations(page: Page) {
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "View details for Mac Studio", exact: true }).click();
  await page.getByRole("tab", { name: "Operations", exact: true }).click();
  return page.getByRole("dialog");
}
function operation(requestId: string, overrides: Partial<NodeOperation> = {}): NodeOperation {
  return {
    id: "operation-1", request_id: requestId, kind: "refresh_inventory", state: "succeeded", reason: null,
    delivery_state: "confirmed", cancel_requested_at: null, created_at: new Date().toISOString(), updated_at: new Date().toISOString(), finished_at: new Date().toISOString(), result_expires_at: new Date(Date.now()+86400000).toISOString(),
    result: { probes: {}, skipped: {}, inventory: { observation_id: "observation-1", revision: 2, delivery: "pending" }, truncated: false, omitted: 0 }, ...overrides,
  };
}

test("old device preserves unknown telemetry and does not request unsupported operations", async ({ page, baseURL }) => {
  await setup(page, nodes[0], baseURL);
  const calls: string[]=[];
  page.on("request", request => { if (request.url().includes("/operations?")) calls.push(request.url()); });
  const dialog=await operations(page);
  const telemetry=dialog.getByTestId("node-telemetry-status");
  await expect(telemetry.getByRole("status")).toHaveText("Unknown");
  await expect(telemetry.locator(".node-facts > div").filter({hasText:"Historical records"}).locator("dd")).toHaveText("Unknown");
  await expect(telemetry.locator(".node-facts > div").filter({hasText:"Latest host records"}).locator("dd")).toHaveText("Unknown");
  await expect(telemetry.locator(".node-facts > div").filter({hasText:"Local storage errors"}).locator("dd")).toHaveText("Unknown");
  await expect(dialog.getByText("This device version does not support finite operations.")).toBeVisible();
  await expect(dialog.getByRole("button",{name:"Collect diagnostics",exact:true})).toHaveCount(0);
  await expect(dialog.getByTestId("emissary-update-progress")).toContainText("maintenance upgrade");
  expect(calls).toEqual([]);
});

test("reports backlog and actual OTA bytes only for the current frozen update", async ({ page, baseURL }) => {
  const now=new Date().toISOString();
  const node:Node={...nodes[0],diagnostics:{...nodes[0].diagnostics,metrics:{telemetry:{state:"schema_unsupported",inventory_state:"waiting_inventory_receiver",spool_records:9,spool_bytes:8192,inventory_records:1,backoff_seconds:30,dropped_batches:0}}},emissary_ota_status:{update_id:"update-1",generation:3,observability:{schema_version:1,launcher_capabilities:["ota-progress-v1"],last_progress_at:now,stalled:false,progress:{update_id:"update-1",generation:3,subphase:"download",bytes_done:512,bytes_total:1024,download_attempt:2,last_progress_at:now},drain:{active_workers:0,pending_log_records:4,pending_log_bytes:256,terminals:0,operations:1}}}};
  await setup(page,node,baseURL);
  const dialog=await operations(page);
  await expect(dialog.getByTestId("node-telemetry-status")).toContainText("Waiting for a compatible data schema");
  await expect(dialog.getByTestId("node-telemetry-status").locator(".node-facts > div").filter({has:page.locator("dt",{hasText:/^Inventory delivery$/})}).locator("dd")).toHaveText("Waiting for a receiver that supports independent inventory");
  await expect(dialog.getByRole("progressbar")).toHaveAttribute("value","50");
  await expect(dialog.getByTestId("emissary-update-progress")).toContainText("512 B / 1.0 KiB · 50%");
  await expect(dialog.getByTestId("emissary-update-progress").locator(".node-facts > div").filter({hasText:"Active workers"}).locator("dd")).toHaveText("0");
  await page.route("**/api/v1/nodes?*",route=>route.fulfill({json:[{...node,emissary_ota_status:{...node.emissary_ota_status,observability:{schema_version:1,launcher_capabilities:["ota-progress-v1"],progress:{update_id:"old-update",generation:2,bytes_done:1024,bytes_total:1024}}}}]}));
  await page.reload();
  const changed=await operations(page);
  await expect(changed.getByRole("progressbar")).toHaveCount(0);
  await expect(changed.getByText("Download bytes are not reported.")).toBeVisible();
});

test("uncertain inventory submission recovers its request without a second POST", async ({ page, baseURL }) => {
  await setup(page,{...nodes[0],capabilities:[...nodes[0].capabilities,"node-operations-v1"]},baseURL);
  let stored:NodeOperation|undefined;let posts=0;let requestId="";
  await page.route(`**/api/v1/nodes/${nodes[0].id}/operations**`,async route=>{
    if(route.request().method()==="POST") {
      posts++;const input=route.request().postDataJSON();requestId=input.request_id;stored=operation(requestId);
      expect(input).toMatchObject({kind:"refresh_inventory",schema:1,input:{mode:"full"}});
      return route.abort("failed");
    }
    return route.fulfill({json:{items:stored?[stored]:[],next_cursor:null}});
  });
  const dialog=await operations(page);
  await dialog.getByRole("button",{name:"Refresh inventory",exact:true}).click();
  await expect(dialog.getByText("The submit response is unknown.",{exact:false})).toBeVisible();
  await page.reload();
  await operations(page);
  await expect(dialog.getByText("The submit response is unknown.",{exact:false})).toBeVisible();
  await dialog.getByRole("button",{name:"Find this request",exact:true}).click();
  await expect(dialog.getByText("The submit response is unknown.",{exact:false})).toHaveCount(0);
  expect(posts).toBe(1);expect(requestId).toMatch(/^[a-f0-9-]{36}$/);
  await dialog.locator("summary").filter({hasText:"Refresh inventory · Succeeded"}).click();
  await expect(dialog.getByText("Saved locally; awaiting Telemetry confirmation")).toBeVisible();
  await expect(dialog.getByText("Scheduling continues to use the immutable startup snapshot.",{exact:false})).toBeVisible();
  expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
});

test("cancellation remains requested until device terminal and members cannot read results", async ({ page, baseURL }) => {
  const node={...nodes[0],capabilities:[...nodes[0].capabilities,"node-operations-v1"]};
  await setup(page,node,baseURL);
  let current=operation("request-1",{state:"running",delivery_state:"awaiting_result",finished_at:null,result:null});let cancels=0;
  await page.route(`**/api/v1/nodes/${node.id}/operations**`,route=>route.fulfill({json:{items:[current],next_cursor:null}}));
  await page.route("**/api/v1/node-operations/operation-1/cancel",route=>{cancels++;current={...current,cancel_requested_at:new Date().toISOString()};return route.fulfill({json:current});});
  const dialog=await operations(page);
  await dialog.locator("summary").filter({hasText:"Refresh inventory · Running"}).click();
  await dialog.getByRole("button",{name:"Request cancellation",exact:true}).click();
  await expect(dialog.getByRole("button",{name:"Cancellation requested",exact:true})).toBeDisabled();
  await expect(dialog.locator("summary").filter({hasText:"Refresh inventory · Running"})).toBeVisible();
  expect(cancels).toBe(1);
  await setup(page,node,baseURL,"viewer");
  await page.reload();
  const member=await operations(page);
  await expect(member.getByTestId("finite-node-operations")).toHaveCount(0);
});

for (const locale of ["en", "zh-CN"]) {
  test(`${locale}: durable rejection and separate delivery lanes preserve disk unknown values`, async ({ page, baseURL }, info) => {
    await page.setViewportSize({ width: 320, height: 900 });
    const delivery = {
      state: "ready", negotiation_state: "retrying_safe_format", last_rejection_reason: "id_content_conflict", last_rejection_at: "2026-11-26T05:03:00Z",
      bounds_exceeded_batches: 0, id_content_conflict_batches: 2, identity_retired_batches: 3, omitted_host_devices: 4, truncated_host_fields: 5,
      fresh_state: "ready", fresh_last_ack_at: "2026-11-26T05:01:00Z", fresh_last_progress_at: "2026-11-26T05:01:01Z", fresh_upload_latency_ms: 12, fresh_backoff_seconds: 0,
      history_state: "overloaded", history_last_ack_at: "2026-11-26T05:02:00Z", history_last_progress_at: "2026-11-26T05:02:01Z", history_upload_latency_ms: 34, history_backoff_seconds: 8,
      inventory_conflict_records: 1, inventory_conflicts_total: 2, inventory_last_conflict_at: "2026-11-26T05:03:30Z",
      inventory_state: "connection_unavailable", inventory_last_ack_at: "2026-11-26T05:04:00Z", inventory_last_progress_at: "2026-11-26T05:04:01Z", inventory_upload_latency_ms: 56, inventory_backoff_seconds: 16,
      disk_state: "ready", disk_observed_at: "2026-11-26T05:05:00Z", disk_file_bytes: 6144, disk_record_bytes: 4096, disk_allocated_bytes: null, disk_block_overhead_bytes: null,
      disk_temporary_bytes: 0, disk_index_bytes: 1024, disk_corrupt_bytes: 512, disk_unknown_bytes: 256, disk_files: 7,
    };
    let node: Node = { ...nodes[0], diagnostics: { ...nodes[0].diagnostics, metrics: { telemetry: delivery } } };
    await setup(page, node, baseURL);
    await page.route("**/api/v1/nodes?*", route => route.fulfill({ json: [node] }));
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: baseURL || "http://127.0.0.1:3100" }]);
    const open = async () => {
      await page.goto(tenantURL("/nodes"));
      await page.getByRole("button", { name: /^(View details for Mac Studio|查看 Mac Studio 的详情)$/ }).click();
      await page.getByRole("tab", { name: locale === "en" ? "Operations" : "运维", exact: true }).click();
      return page.getByTestId("node-telemetry-status");
    };
    const telemetry = await open();
    await expect(telemetry.getByRole("status")).toHaveText(locale === "en" ? "Delivery is progressing" : "正在正常交付");
    await expect(telemetry.getByText(locale === "en" ? "Record identity and content conflict" : "记录身份与内容冲突", { exact: true })).toBeVisible();
    await expect(telemetry.getByText(locale === "en" ? "Retrying negotiation with the established format" : "沿用安全格式并重试协商", { exact: true })).toBeVisible();
    for (const [label, count] of locale === "en" ? [["Quarantined inventories", "1"], ["Inventory conflicts recorded", "2"]] : [["已隔离的冲突清单", "1"], ["清单冲突累计次数", "2"]]) {
      await expect(telemetry.locator(".node-facts > div").filter({has: page.locator("dt", {hasText: new RegExp(`^${label}$`)})}).locator("dd")).toHaveText(count);
    }
    await expect(telemetry.locator(`time[datetime="${delivery.inventory_last_conflict_at}"]`)).toHaveCount(1);
    const lanes = telemetry.getByTestId("telemetry-delivery-lanes");
    await lanes.locator("summary").click();
    for (const [lane, ack, progress, backoff] of [["fresh", delivery.fresh_last_ack_at, delivery.fresh_last_progress_at, "0"], ["history", delivery.history_last_ack_at, delivery.history_last_progress_at, "8"], ["inventory", delivery.inventory_last_ack_at, delivery.inventory_last_progress_at, "16"]]) {
      const section = lanes.getByTestId(`telemetry-lane-${lane}`);
      await expect(section.locator(`time[datetime="${ack}"]`)).toHaveCount(1);
      await expect(section.locator(`time[datetime="${progress}"]`)).toHaveCount(1);
      await expect(section.locator(".node-facts > div").filter({ has: page.locator("dt", { hasText: locale === "en" ? /^Retry delay$/ : /^重试等待$/ }) }).locator("dd")).toHaveText(`${backoff}${locale === "en" ? "s" : "秒"}`);
    }
    const disk = telemetry.getByTestId("telemetry-disk-observation");
    await disk.locator("summary").click();
    const facts = locale === "en" ? [
      ["File contents", "6.0 KiB"], ["Allocated on disk", "Unknown"], ["Block allocation overhead", "Unknown"], ["Temporary files", "0 B"], ["Indexes", "1.0 KiB"], ["Preserved unreadable files", "512 B"], ["Unclassified files", "256 B"], ["Observed files", "7"],
    ] : [
      ["文件内容占用", "6.0 KiB"], ["磁盘实际分配", "未知"], ["磁盘块分配开销", "未知"], ["临时文件占用", "0 B"], ["索引占用", "1.0 KiB"], ["保留的损坏文件占用", "512 B"], ["未分类文件占用", "256 B"], ["观测文件数", "7"],
    ];
    for (const [label, value] of facts) {
      await expect(disk.locator(".node-facts > div").filter({ has: page.locator("dt", { hasText: new RegExp(`^${label}$`) }) }).locator("dd")).toHaveText(value);
    }
    await expect(disk.getByText(locale === "en" ? "The queue quota limits record contents. Observed disk use also includes temporary files, indexes and allocation overhead." : "队列额度限制记录正文。磁盘观测还包含临时文件、索引和分配开销。", { exact: true })).toBeVisible();
    const losses = telemetry.getByTestId("telemetry-permanent-losses");
    await losses.locator("summary").click();
    for (const [label, value] of locale === "en" ? [["Samples rejected by size limits", "0"], ["Samples with identity conflicts", "2"], ["Samples with retired identities", "3"], ["Host devices omitted by the sample budget", "4"], ["Host text fields truncated by the sample budget", "5"]] : [["超过大小限制拒收的采样", "0"], ["身份内容冲突的采样", "2"], ["身份退役丢弃的采样", "3"], ["采样额度省略的主机设备数", "4"], ["采样额度截断的主机文本字段数", "5"]]) {
      await expect(losses.locator(".node-facts > div").filter({ has: page.locator("dt", { hasText: new RegExp(`^${label}$`) }) }).locator("dd")).toHaveText(value);
    }
    const dialog = page.getByRole("dialog");
    expect(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth + 1)).toBe(true);
    expect(await telemetry.evaluate(element => [...element.querySelectorAll(".node-facts > div")].every(row => row.scrollWidth <= row.clientWidth + 1))).toBe(true);
    await disk.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/ui/node-telemetry-disk-${locale}-${info.project.name}.png`, fullPage: true });
    node = { ...node, diagnostics: { ...node.diagnostics, metrics: { telemetry: { ...delivery, truncated_host_fields: 0, disk_state: "unsafe_entry", disk_file_bytes: null, disk_temporary_bytes: null } } } };
    await page.reload();
    const failed = await open();
    const failedDisk = failed.getByTestId("telemetry-disk-observation");
    await failedDisk.locator("summary").click();
    await expect(failedDisk).toContainText(locale === "en" ? "Unsafe filesystem entry; totals are incomplete" : "存在不安全文件项，合计不完整");
    await expect(failedDisk.locator(".node-facts > div").filter({ has: page.locator("dt", { hasText: locale === "en" ? /^File contents$/ : /^文件内容占用$/ }) }).locator("dd")).toHaveText(locale === "en" ? "Unknown" : "未知");
    const zeroLosses = failed.getByTestId("telemetry-permanent-losses");
    await zeroLosses.locator("summary").click();
    await expect(zeroLosses.locator(".node-facts > div").filter({ has: page.locator("dt", { hasText: locale === "en" ? /^Host text fields truncated by the sample budget$/ : /^采样额度截断的主机文本字段数$/ }) }).locator("dd")).toHaveText("0");
  });

  test(`${locale}: scalar telemetry preserves queue ages, schema waits and zero counters`, async ({ page, baseURL }, info) => {
    await page.setViewportSize({width:320,height:900});
    let node: Node={...nodes[0],diagnostics:{...nodes[0].diagnostics,metrics:{telemetry:{
      state:"schema_wait",inventory_state:"schema_wait",spool_records:9,spool_bytes:8192,oldest_age_seconds:23,schema_wait_records:2,
      fresh_records:1,fresh_bytes:1024,fresh_oldest_age_seconds:7,inventory_records:3,inventory_bytes:2048,inventory_oldest_age_seconds:11,inventory_schema_wait_records:1,
      quota_eviction_batches:0,expired_batches:4,malformed_batches:5,disk_errors:6,collector_duration_ms:2,collector_error:"collector_unavailable",
    }}}};
    await setup(page,node,baseURL);
    await page.route("**/api/v1/nodes?*",route=>route.fulfill({json:[node]}));
    await page.context().addCookies([{name:"artifactory-locale",value:locale,url:baseURL || "http://127.0.0.1:3100"}]);
    const open=async()=>{
      await page.goto(tenantURL("/nodes"));
      await page.getByRole("button",{name:/^(View details for Mac Studio|查看 Mac Studio 的详情)$/}).click();
      await page.getByRole("tab",{name:locale==="en"?"Operations":"运维",exact:true}).click();
      return page.getByTestId("node-telemetry-status");
    };
    let telemetry=await open();
    await expect(telemetry.getByRole("status")).toHaveText(locale==="en"?"Queued records await a receiver that supports their schema or size":"队列记录等待接收端支持其格式或大小");
    const facts: [string,string][] = locale==="en" ? [
      ["Samples awaiting compatible receiver","2"],["Latest host records","1"],["Oldest host snapshot age","7s"],
      ["Oldest inventory age","11s"],["Inventories awaiting compatible receiver","1"],["Samples evicted by quota","0"],
      ["Expired samples discarded","4"],["Malformed samples discarded","5"],["Local storage errors","6"],["Collector status","Collection unavailable"],
    ] : [
      ["等待接收端兼容的采样","2"],["最新主机记录数","1"],["最旧主机快照年龄","7秒"],
      ["最旧清单年龄","11秒"],["等待接收端兼容的清单","1"],["容量不足淘汰的采样","0"],
      ["过期丢弃的采样","4"],["格式错误丢弃的采样","5"],["本地存储错误次数","6"],["采集器状态","采集不可用"],
    ];
    for(const [label,value] of facts) {
      await expect(telemetry.locator(".node-facts > div").filter({has:page.locator("dt",{hasText:new RegExp(`^${label}$`)})}).locator("dd")).toHaveText(value);
    }
    const dialog=page.getByRole("dialog");
    expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
    await telemetry.scrollIntoViewIfNeeded();
    await page.screenshot({path:`test-results/ui/node-telemetry-scalars-${locale}-${info.project.name}.png`,fullPage:true});
    node={...node,online:false,diagnostics:{...node.diagnostics,metrics:{telemetry:{state:"ready",spool_records:1,schema_wait_records:null,inventory_records:null,inventory_bytes:null,inventory_schema_wait_records:null,inventory_superseded:null,inventory_conflict_records:null,inventory_conflicts_total:null,inventory_last_conflict_at:null,fresh_records:null,fresh_bytes:null,fresh_oldest_age_seconds:null,inventory_oldest_age_seconds:-1}}}};
    await page.reload();telemetry=await open();
    await expect(telemetry.getByRole("status")).toHaveText(locale==="en"?"Device is offline with pending delivery":"设备离线，仍有待交付数据");
    for(const label of locale==="en"?["Oldest host snapshot age","Oldest inventory age","Collector status","Samples awaiting compatible receiver","Latest host records","Latest host data","Pending inventory","Pending inventory bytes","Inventories awaiting compatible receiver","Superseded inventories","Quarantined inventories","Inventory conflicts recorded","Last inventory conflict"]:["最旧主机快照年龄","最旧清单年龄","采集器状态","等待接收端兼容的采样","最新主机记录数","最新主机数据","待确认清单","待确认清单字节","等待接收端兼容的清单","已替代清单","已隔离的冲突清单","清单冲突累计次数","最近清单冲突时间"]) {
      await expect(telemetry.locator(".node-facts > div").filter({has:page.locator("dt",{hasText:new RegExp(`^${label}$`)})}).locator("dd")).toHaveText(locale==="en"?"Unknown":"未知");
    }
  });

  test(`${locale}: narrow detail keeps outcome unknown distinct from expired result`, async ({ page, baseURL }, info) => {
    await page.setViewportSize({width:320,height:900});
    await setup(page,{...nodes[0],capabilities:[...nodes[0].capabilities,"node-operations-v1"]},baseURL);
    await page.context().addCookies([{name:"artifactory-locale",value:locale,url:baseURL || "http://127.0.0.1:3100"}]);
    const uncertain=operation("pending",{state:"running",delivery_state:"outcome_unknown",finished_at:null,result:null});
    const expired=operation("expired",{id:"operation-2",kind:"collect_diagnostics",state:"failed",reason:"cleanup_failed",result:null,result_expires_at:"2026-01-01T00:00:00Z"});
    await page.route(`**/api/v1/nodes/${nodes[0].id}/operations**`,route=>route.fulfill({json:{items:[uncertain,expired],next_cursor:null}}));
    await page.goto(tenantURL("/nodes"));
    await page.getByRole("button",{name:/^(View details for Mac Studio|查看 Mac Studio 的详情)$/}).click();
    await page.getByRole("tab",{name:locale==="en"?"Operations":"运维",exact:true}).click();
    const dialog=page.getByRole("dialog");
    const list=dialog.getByTestId("finite-node-operations");
    const summaries=list.locator("summary");
    await expect(summaries).toHaveCount(2);
    await summaries.nth(0).click();await summaries.nth(1).click();
    await expect(summaries.nth(0)).toContainText(locale==="en"?"Outcome unknown":"结果未知");
    await expect(list.getByText(locale==="en"?"Result body is no longer available.":"结果正文已不可用。",{exact:false})).toBeVisible();
    await expect(list.getByText(locale==="en"?"A probe process could not be stopped; OTA drain remains blocked":"探测进程未能停止；升级排空仍被阻止",{exact:true})).toBeVisible();
    await expect(list.getByRole("button",{name:locale==="en"?"Request cancellation":"请求取消",exact:true})).toHaveCount(0);
    expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
    await page.screenshot({path:`test-results/ui/node-evolution-${locale}-${info.project.name}.png`,fullPage:true});
  });
}

test("revoked diagnostics read hides previously received probe results", async ({ page, baseURL }) => {
  await setup(page,{...nodes[0],capabilities:[...nodes[0].capabilities,"node-operations-v1"]},baseURL);
  let denied=false;
  const item=operation("request-1",{result:{inventory:null,probes:{git:{status:"available",observed_at:new Date().toISOString(),duration_ms:2,probe_version:1,version:"2.40.1",error_code:null}},skipped:{},truncated:false,omitted:0}});
  await page.route(`**/api/v1/nodes/${nodes[0].id}/operations**`,route=>denied?route.fulfill({status:403,json:{error:"Forbidden"}}):route.fulfill({json:{items:[item],next_cursor:null}}));
  const dialog=await operations(page);
  const list=dialog.getByTestId("finite-node-operations");
  await list.locator("summary").click();
  await expect(list.getByText("2.40.1",{exact:true})).toBeVisible();
  denied=true;
  await expect(list.locator("summary")).toHaveCount(0);
  await expect(list.getByText("2.40.1",{exact:true})).toHaveCount(0);
  await expect(list.getByRole("button",{name:"Refresh inventory",exact:true})).toBeDisabled();
});
