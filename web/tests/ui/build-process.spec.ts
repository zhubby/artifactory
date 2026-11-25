/**
 * [INPUT]: 依赖真实构建页、阶段夹具和受权过程接口。
 * [OUTPUT]: 验证本代准备围栏、源码失败与响应式无障碍。
 * [POS]: 观察者过程视图的浏览器回归，不创建真实构建。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";
import { buildStages, sourceSteps, type BuildActivity } from "../../src/lib/build-activity";

const failure = { ...build, status: "failed", commit_sha: null, source_manifest_id: null, current_plan_id: null,
  jobs: [], artifacts: [], created_at:"2026-11-24T08:25:09Z", finished_at: "2026-11-24T08:27:10Z", error: "Git operation timed out" };
const activity = {
  build_id: build.id, generated_at: failure.finished_at, truncated: false,
  source: { status: "failed", created_at: failure.created_at, finished_at: failure.finished_at, attempt: 1, pod: "source-build-a1",
    history_available: true, diagnostic: { stage: "resolve_ref", code: "timeout" } },
  events: [{ id: "source/1/2", stage: "source", action: "source_resolve_ref", status: "running", created_at: "2026-11-24T08:25:10Z", details: { actor: "source-build-a1", attempt: 1 } },
    { id: "source/finished", stage: "source", action: "source_finished", status: "failed", created_at: failure.finished_at, details: { actor: "source", code: "timeout" } }],
  planning: { plans: [], runs: [], events: [] },
};

const preparation = { run_id: "run-1", generation: 1, pod_uid: "pod-1", pod_name: "planning-1",
  current_stage: "lfs", current_state: "started" as const, events: [
    { execution: { generation: 1, uid: "pod-1", name: "planning-1" }, progress: { sequence: 1, stage: "fetch", state: "completed" as const }, created_at: "2026-11-25T12:00:00Z" },
    { execution: { generation: 1, uid: "pod-1", name: "planning-1" }, progress: { sequence: 2, stage: "lfs", state: "started" as const }, created_at: "2026-11-25T12:00:01Z" },
  ] };
const pending = { ...build, source_preparation_version: 2, status: "planning", current_plan_id: null, jobs: [], artifacts: [], finished_at: null };
const pendingActivity: BuildActivity = { ...activity, events: [], source: { ...activity.source, status: "succeeded", diagnostic: null, protocol_version: 2, preparation },
  planning: { plans: [], events: [], runs: [{ id: "run-1", state: "leased", purpose: "initial", calls: 0, usage: {}, error: null,
    runtime: { status: "provisioning", generation: 1, calls: 0, usage: {}, preparation } }] } };

test("v2 stage evidence requires current Pod readiness and AI start records", () => {
  expect(buildStages(pending, pendingActivity).slice(0, 2)).toEqual([{ key: "source", state: "running" }, { key: "planning", state: "not_started" }]);
  const ready = structuredClone(pendingActivity);
  ready.planning.runs[0].runtime!.preparation = { ...preparation, ready_at: "2026-11-25T12:01:00Z" };
  expect(buildStages(pending, ready).slice(0, 2)).toEqual([{ key: "source", state: "succeeded" }, { key: "planning", state: "queued" }]);
  ready.planning.runs[0].runtime!.preparation!.started_at = "2026-11-25T12:01:01Z";
  expect(buildStages(pending, ready)[1].state).toBe("running");
  ready.planning.runs[0].runtime!.generation = 2;
  expect(buildStages(pending, ready).slice(0, 2)).toEqual([{ key: "source", state: "running" }, { key: "planning", state: "not_started" }]);
  expect(sourceSteps(pending, ready).every(step => step.state === "not_started")).toBe(true);
  delete ready.planning.runs[0].runtime;
  ready.planning.runs[0].runtime_unavailable = true;
  expect(buildStages(pending, ready)[0].state).toBe("running");
  const terminal = structuredClone(pendingActivity);
  terminal.planning.runs[0].state = "failed";
  terminal.source!.preparation = { run_id: "run-1" };
  expect(buildStages({ ...pending, status: "failed" }, terminal)[0].state).toBe("unknown");
  terminal.source!.preparation.diagnostic = { stage: "fetch", code: "timeout" };
  expect(buildStages({ ...pending, status: "failed" }, terminal)[0].state).toBe("failed");
});

test("v2 observer sees local preparation without treating a pinned commit as ready", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: pending }));
  await page.route(`**/api/v1/builds/${build.id}/activity`, route => route.fulfill({ json: pendingActivity }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  const process = page.getByRole("region", { name: "构建过程", exact: true });
  await expect(process.getByRole("heading", { name: "正在准备源码", exact: true })).toBeVisible();
  const source = page.getByRole("region", { name: "规划 Pod 源码准备", exact: true });
  await expect(source.getByText("planning-1", { exact: true })).toBeVisible();
  await expect(source.locator("li").filter({ hasText: "下载固定提交" })).toContainText("已完成");
  await expect(source.locator("li").filter({ hasText: "下载与校验 LFS" })).toContainText("进行中");
  await expect(source.locator("li").filter({ hasText: "准备嵌套子模块" })).toContainText("暂无记录");
  await expect(source.getByText("提交已固定，目录就绪须单独确认。", { exact: true })).toBeVisible();
  await expect(process.getByRole("button", { name: /AI 规划/ })).toContainText("尚未开始");
});

test("source timeout identifies the failed action and explicitly leaves later stages unstarted", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: failure }));
  await page.route(`**/api/v1/builds/${build.id}/activity`, route => route.fulfill({ json: activity }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await expect(page.getByRole("tab", { name: "过程", exact: true })).toHaveAttribute("aria-selected", "true");
  const process = page.getByRole("region", { name: "构建过程", exact: true });
  await expect(process.getByRole("heading", { name: "源码准备失败", exact: true })).toBeVisible();
  await expect(process.locator(".build-process-downstream").getByText("AI 规划尚未开始", { exact: true })).toBeVisible();
  await expect(process.getByText("构建设备尚未启动", { exact: true })).toBeVisible();
  await expect(process.getByText(/检查源码池的出口网络和代理配置/)).toBeVisible();
  await expect(page.locator(".build-detail-meta")).toContainText("未固定提交");
  await process.getByRole("button", { name: /源码准备/ }).click();
  await expect(process.locator(".build-process-events li")).toHaveCount(2);
  await process.locator(".build-process-events summary").filter({ hasText: "解析源码引用" }).click();
  await expect(process.getByText("source-build-a1", { exact: true }).last()).toBeVisible();
});

test("legacy source failure does not manufacture a historical diagnostic", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: failure }));
  await page.route(`**/api/v1/builds/${build.id}/activity`, route => route.fulfill({ json: { ...activity, source: { ...activity.source, history_available: false, diagnostic: null }, events: [] } }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await expect(page.getByText("此构建没有保存详细源码记录，无法还原当时的底层错误。", { exact: true })).toBeVisible();
  await expect(page.locator(".build-process-events li")).toHaveCount(0);
});

test("background activity preserves an expanded event and the observer's selected tab", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  const snapshot = { ...build, status:"planning",jobs:[],artifacts:[],current_plan_id:null,finished_at:null };
  let reads = 0;
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json:snapshot }));
  await page.route(`**/api/v1/builds/${build.id}/events`, route => route.fulfill({ contentType:"text/event-stream",body:`retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(snapshot)}\n\n` }));
  await page.route(`**/api/v1/builds/${build.id}/activity`, route => {
    reads++;
    return route.fulfill({json:{...activity,source:{...activity.source,status:"succeeded",diagnostic:null},
      events:activity.events.slice(0,1),planning:{plans:[],events:[],runs:[{id:"run-1",state:"leased",purpose:"initial",calls:reads,usage:{},error:null}]}}});
  });
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  const row = page.locator(".build-process-events details").first();
  await row.locator("summary").click();
  await page.clock.fastForward(3100);
  await expect.poll(() => reads).toBeGreaterThan(1);
  await expect(row).toHaveAttribute("open","");
  await expect(page.getByRole("progressbar")).toBeHidden();
  await page.getByRole("tab",{name:"计划",exact:true}).click();
  await page.clock.fastForward(3100);
  await expect.poll(() => reads).toBeGreaterThan(2);
  await expect(page.getByRole("tab",{name:"计划",exact:true})).toHaveAttribute("aria-selected","true");
  await page.getByRole("tab",{name:"过程",exact:true}).click();
  await expect(row).toHaveAttribute("open","");
});

for (const locale of ["zh-CN", "en"] as const) for (const theme of ["light", "dark"] as const) {
  test(`${locale} ${theme}: process is readable, responsive and accessible`, async ({ page }, info) => {
    await fixtureAPI(page);
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: String(info.project.use.baseURL) }]);
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: pending }));
    await page.route(`**/api/v1/builds/${build.id}/activity`, route => route.fulfill({ json: pendingActivity }));
    await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
    await expect(page.locator(".build-process-summary h2")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    const summary = await page.locator(".build-process-summary").boundingBox();
    const duration = await page.locator(".build-process-duration").boundingBox();
    expect(summary).not.toBeNull();
    expect(duration).not.toBeNull();
    expect(duration!.x + duration!.width).toBeLessThanOrEqual(summary!.x + summary!.width + 1);
    const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(result.violations.map(item => ({ id: item.id, nodes: item.nodes.map(node => node.target) }))).toEqual([]);
    await page.screenshot({ path: info.outputPath(`${locale}-${theme}-build-process.png`), fullPage: true });
  });
}

test("v2 preparation stays inside a 320px viewport", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: pending }));
  await page.route(`**/api/v1/builds/${build.id}/activity`, route => route.fulfill({ json: pendingActivity }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await expect(page.getByRole("region", { name: "规划 Pod 源码准备", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
});
