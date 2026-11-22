/**
 * [INPUT]: 依赖构建、版本计划、SSE 与澄清接口夹具。
 * [OUTPUT]: 验证计划身份、执行归属、草稿和标签生命周期。
 * [POS]: 构建详情工作台的响应式、双语和主题回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { Build } from "../../src/lib/types";
import { themeStorage } from "../../src/lib/preferences";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

const native = {
  os: "darwin" as const, arch: "arm64" as const, application_platform: "macos" as const,
  artifacts: ["dist/DesktopClient.zip"], capabilities: ["xcode", "codesign"],
  signing_profiles: ["apple-internal"], timeout_seconds: 1800,
  steps: [
    { name: "Compile application", phase: "build", shell: "bash", timeout_seconds: 600, run: "if [ -d src ]; then\n  make build\nfi\n" },
    { name: "Verify package", phase: "verify", shell: "bash", timeout_seconds: 120, run: "test -s dist/DesktopClient.zip" },
  ],
};
const container = {
  os: "linux" as const, arch: "amd64" as const, application_platform: null, executor: "buildkit" as const,
  steps: [], artifacts: [], timeout_seconds: 1200,
  image: {
    name: "desktop-service", context: "services/api", dockerfile: "docker/production.Dockerfile",
    platforms: ["linux/amd64" as const, "linux/arm64" as const], pool: "release-builders", allow_emulation: false,
    build_args: { CHANNEL: "stable" }, secrets: { package_token: "secret-reference-only" },
    pull_credentials: ["registry-reference"], tags: ["candidate"],
  },
};
const summary = "Build the pinned desktop application.\nKeep the native installer signed.\nVerify the collected package.\nBuild the service image from its Dockerfile.\nThis additional explanation belongs in the expanded summary.";
function plan(revision: number, targets: Record<string, typeof native | typeof container> = { macos: native }) {
  return {
    id: `plan-v${revision}`, revision, sha256: String(revision).repeat(64), created_at: build.created_at,
    contents: {
      summary: revision === 2 ? summary : `Plan revision ${revision} implementation`,
      evidence: [{ path: "src/main.c", reason: "Native application entry point" }],
      scripts: [{ name: "verify.sh", content: "test -s dist/DesktopClient.zip" }],
      recipe: { version: 1, targets },
    },
  };
}
const revision = (page: Page) => page.getByRole("combobox", { name: /^(计划版本|Plan revision)$/, exact: true });
const planPanel = (page: Page) => page.locator("section.build-plan");
const scheme = (page: Page) => page.getByRole("region", { name: /^(构建方案|Build scheme)$/, exact: true });
const targetPanel = (page: Page, key: string) => scheme(page).locator(`.build-plan-targets > section[data-target="${key}"]`);
const steps = (page: Page, key = "macos") => targetPanel(page, key).locator("ol.build-plan-steps > li > details");
const tab = (page: Page, key: "plan" | "execution" | "output") => page.getByRole("tab", {
  name: key === "plan" ? /^(计划|Plan)$/ : key === "execution" ? /^(执行|Execution)$/ : /^(制品|Artifacts)$/,
  exact: true,
});

async function selectTarget(page: Page, key: string) {
  const trigger = page.getByRole("combobox", { name: /^(构建目标|Build target)$/, exact: true });
  if (await trigger.isVisible()) await chooseOption(trigger, key);
  else await scheme(page).getByRole("navigation", { name: /^(构建目标|Build target)$/ }).getByRole("button").filter({ has: page.locator(".build-plan-nav-name").getByText(key, { exact: true }) }).click();
  await expect(targetPanel(page, key)).toBeVisible();
}

async function audit(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  expect(result.violations.map(violation => ({ id: violation.id, nodes: violation.nodes.map(node => node.target) }))).toEqual([]);
}

async function setup(page: Page, options: { status?: string; current?: number; plans?: ReturnType<typeof plan>[]; locale?: string; theme?: string; jobs?: Build["jobs"] } = {}) {
  const events = await fixtureAPI(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: options.locale || "zh-CN", url: "http://127.0.0.1" }]);
  await page.addInitScript(({ key, value }) => localStorage.setItem(key, value), { key: themeStorage, value: options.theme || "light" });
  let plans = options.plans || [plan(1), plan(2)];
  const initialPlan = plans.find(entry => entry.revision === (options.current || 2)) || plans[0];
  const requirements: Build["target_requirements"] = Object.entries(initialPlan.contents.recipe.targets).map(([key, target]) => "image" in target
    ? { kind: "image", key, platforms: target.image.platforms }
    : { kind: "native", key, os: target.os, arch: target.arch, application_platform: target.application_platform, artifact_key: "app", format_suffix: ".zip" });
  let snapshot: Build = {
    ...build, status: options.status || "planning", current_plan_id: `plan-v${options.current || 2}`,
    revision_count: options.current || 2, source_manifest_id: "manifest-fixed",
    jobs: options.jobs || [], artifacts: [], target_requirements: requirements, selected_targets: requirements.map(target => target.key),
  };
  let planReads = 0;
  let streamReads = 0;
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: snapshot }));
  await page.route(`**/api/v1/builds/${build.id}/events`, route => {
    streamReads++;
    return route.fulfill({ contentType: "text/event-stream", body: `retry: 1000\nevent: snapshot\ndata: ${JSON.stringify(snapshot)}\n\nevent: log\ndata: ${JSON.stringify({ id: 1, attempt_id: "attempt-main", step: "compile", text: "retained native output\n" })}\n\n` });
  });
  await page.route(`**/api/v1/builds/${build.id}/plans`, route => {
    planReads++;
    return route.fulfill({ json: { plans, runs: [], events: [] } });
  });
  return {
    events,
    get planReads() { return planReads; },
    get streamReads() { return streamReads; },
    updateBuild(value: Partial<Build>) { snapshot = { ...snapshot, ...value }; },
    updatePlans(value: typeof plans) { plans = value; },
    async open() {
      await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
      await expect(page.getByRole("heading", { level: 1, name: /^(构建|Build) #128$/, exact: true })).toBeVisible();
    },
    async poll() {
      const before = planReads;
      await page.clock.fastForward(3100);
      await expect.poll(() => planReads).toBeGreaterThan(before);
    },
  };
}

test("current_plan_id selects the accepted plan regardless of response order and expands only its first command", async ({ page }) => {
  const state = await setup(page, { plans: [plan(2), plan(1)] });
  await state.open();
  await expect(tab(page, "plan")).toHaveAttribute("aria-selected", "true");
  await expect(revision(page)).toHaveAttribute("data-value", "plan-v2");
  await expect(planPanel(page)).toContainText("Build the pinned desktop application.");
  await expect(planPanel(page)).not.toContainText("Plan revision 1 implementation");
  await expect(steps(page).first()).toHaveAttribute("open", "");
  await expect(steps(page).nth(1)).not.toHaveAttribute("open", "");
  await expect(steps(page).first().locator("pre")).toHaveText(native.steps[0].run);
  await expect(steps(page).nth(1).locator("pre")).toBeHidden();
  expect(await steps(page).first().locator("pre").evaluate(element => getComputedStyle(element).whiteSpace)).toMatch(/^pre/);
  const summaryText = planPanel(page).locator(".build-plan-summary-text");
  await expect(summaryText).toHaveAttribute("data-expanded", "false");
  expect(await summaryText.evaluate(element => getComputedStyle(element).webkitLineClamp)).toBe("4");
  await page.getByRole("button", { name: "展开完整摘要", exact: true }).click();
  await expect(summaryText).toHaveAttribute("data-expanded", "true");
  await page.getByRole("button", { name: "收起摘要", exact: true }).click();
  await expect(summaryText).toHaveAttribute("data-expanded", "false");
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
    writeText: async (value: string) => { document.documentElement.dataset.copiedCommand = value; },
  } }));
  await steps(page).first().getByRole("button", { name: "复制", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied-command", native.steps[0].run);
  expect(state.events.writes).toEqual([]);
});

test("an explicit historical revision and expanded command survive polling and a newer current plan", async ({ page }) => {
  await page.clock.install();
  const state = await setup(page);
  await state.open();
  await chooseOption(revision(page), "plan-v1");
  await steps(page).nth(1).locator("summary").click();
  state.updateBuild({ current_plan_id: "plan-v3", revision_count: 3 });
  state.updatePlans([plan(3), plan(2), plan(1)]);
  await state.poll();
  await expect(revision(page)).toHaveAttribute("data-value", "plan-v1");
  await expect(planPanel(page)).toContainText("Plan revision 1 implementation");
  await expect(planPanel(page)).toContainText("历史版本，仅展示此版本的方案。");
  await expect(steps(page).nth(1)).toHaveAttribute("open", "");
  await chooseOption(revision(page), "plan-v3");
  await expect(planPanel(page)).toContainText("Plan revision 3 implementation");
});

test("a temporarily missing current revision retains the accepted scheme until its payload arrives", async ({ page }) => {
  await page.clock.install();
  const state = await setup(page);
  await state.open();
  state.updateBuild({ current_plan_id: "plan-v3", revision_count: 3 });
  await state.poll();
  await expect(page.getByText("正在同步当前计划…", { exact: true })).toBeVisible();
  await expect(planPanel(page)).toContainText("Build the pinned desktop application.");
  state.updatePlans([plan(1), plan(2), plan(3)]);
  await state.poll();
  await expect(revision(page)).toHaveAttribute("data-value", "plan-v3");
  await expect(planPanel(page)).toContainText("Plan revision 3 implementation");
  await expect(page.getByText("正在同步当前计划…", { exact: true })).toHaveCount(0);
});

test("revision identity prevents stale attempt progress and identifies retained successful targets", async ({ page }) => {
  const base = build.jobs![0];
  const previousAttempt = { ...base.attempts[0], plan_revision_id: "plan-v1", status: "succeeded", steps: base.attempts[0].steps.map((step, index) => ({ ...step, name: native.steps[index].name, phase: native.steps[index].phase, status: "succeeded" })) };
  const stale = { ...base, plan_revision_id: "plan-v2", attempts: [previousAttempt] };
  const retained = { ...base, id: "job-retained", target: "retained", status: "succeeded", plan_revision_id: "plan-v1", attempts: [{ ...previousAttempt, id: "attempt-retained" }] };
  const verified = { ...base, id: "job-verified", target: "verified", status: "succeeded", plan_revision_id: "plan-v2", attempts: [{ ...previousAttempt, id: "attempt-verified", plan_revision_id: "plan-v2" }] };
  const targets = { macos: native, retained: native, verified: native };
  const state = await setup(page, { status: "running", jobs: [stale, retained, verified], plans: [plan(1, targets), plan(2, targets)] });
  await state.open();
  await tab(page, "plan").click();
  await expect(steps(page).first().locator(".status-succeeded")).toHaveCount(0);
  await selectTarget(page, "retained");
  await expect(targetPanel(page, "retained")).toContainText("保留成功结果 · 来自修订 1");
  await expect(steps(page, "retained").first().locator(".status-succeeded")).toHaveCount(0);
  await selectTarget(page, "verified");
  await expect(steps(page, "verified").first().locator(".status-succeeded")).toBeVisible();
  await chooseOption(revision(page), "plan-v1");
  await selectTarget(page, "macos");
  await expect(steps(page).first().locator(".status-running")).toHaveCount(0);
  await expect(steps(page).first().locator(".status-succeeded")).toHaveCount(0);
  await expect(targetPanel(page, "macos")).not.toContainText("保留成功结果");
});

test("clarification drafts survive polling, tab switches and a failed submit before continuing", async ({ page }) => {
  await page.clock.install();
  const state = await setup(page, { status: "needs_input" });
  state.updateBuild({ clarification: { question: "Which application should be built?" } });
  let submissions = 0;
  let answer = "";
  await page.route(`**/api/v1/builds/${build.id}/clarifications`, async route => {
    answer = route.request().postDataJSON().answer;
    submissions++;
    if (submissions === 1) return route.fulfill({ status: 503, json: { error: "Clarification service unavailable" } });
    state.updateBuild({ status: "replanning", clarification: null });
    return route.fulfill({ json: { ok: true } });
  });
  await state.open();
  const draft = page.locator('textarea[name="answer"]');
  await draft.fill("The desktop application, with its signed installer.");
  await state.poll();
  await tab(page, "execution").click();
  await tab(page, "plan").click();
  await expect(draft).toHaveValue("The desktop application, with its signed installer.");
  const submit = page.getByRole("button", { name: "提交回答并继续规划", exact: true });
  await submit.click();
  await expect(page.getByText("Clarification service unavailable", { exact: true })).toBeVisible();
  await expect(draft).toHaveValue("The desktop application, with its signed installer.");
  await submit.click();
  await expect.poll(() => submissions).toBe(2);
  expect(answer).toBe("The desktop application, with its signed installer.");
  await expect(draft).toHaveCount(0);
  await expect(tab(page, "plan")).toHaveAttribute("aria-selected", "true");
});

test("the user's tab and mounted native log survive a planning-to-running transition", async ({ page }) => {
  await page.clock.install();
  const state = await setup(page, { jobs: build.jobs });
  await state.open();
  await tab(page, "execution").click();
  const log = page.getByRole("log");
  await expect(log).toContainText("retained native output");
  await log.evaluate(element => { element.setAttribute("data-lifecycle-probe", "original"); });
  await page.getByRole("button", { name: "暂停跟随日志", exact: true }).click();
  await tab(page, "plan").click();
  state.updateBuild({ status: "running" });
  await state.poll();
  await expect(tab(page, "plan")).toHaveAttribute("aria-selected", "true");
  await tab(page, "execution").click();
  await expect(log).toHaveAttribute("data-lifecycle-probe", "original");
  await expect(log).toContainText("retained native output");
  await expect(page.getByRole("button", { name: "跟随日志", exact: true })).toBeVisible();
});

for (const [locale, theme] of [["zh-CN", "light"], ["en", "dark"]]) {
  test(`${locale}/${theme}: native and OCI schemes expose keyboard controls within the viewport`, async ({ page }, info) => {
    const state = await setup(page, { locale, theme, plans: [plan(1), plan(2, { macos: native, container })] });
    await state.open();
    await expect(scheme(page)).toBeVisible();
    const version = revision(page);
    await version.focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("listbox")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(version).toBeFocused();
    await steps(page).nth(1).locator("summary").focus();
    await page.keyboard.press("Enter");
    await expect(steps(page).nth(1).locator("pre")).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await audit(page);
    await page.evaluate(() => { (document.activeElement as HTMLElement)?.blur(); document.scrollingElement!.scrollTop = 0; });
    await page.screenshot({ path: info.outputPath(`build-plan-native-${locale}-${theme}.png`), fullPage: true });
    await page.screenshot({ path: info.outputPath(`build-plan-native-${locale}-${theme}-viewport.png`) });
    await selectTarget(page, "container");
    const image = targetPanel(page, "container");
    for (const value of ["docker/production.Dockerfile", "services/api", "linux/amd64", "linux/arm64", "release-builders"]) await expect(image.getByText(value, { exact: true })).toBeVisible();
    await image.locator("details.build-plan-fold > summary").click();
    await expect(image.getByText("CHANNEL=stable", { exact: true })).toBeVisible();
    await expect(image.getByText("package_token: secret-reference-only", { exact: true })).toBeVisible();
    await expect(image.locator("ol.build-plan-steps")).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
    await audit(page);
    await selectTarget(page, "macos");
    await expect(steps(page).nth(1)).toHaveAttribute("open", "");
    await selectTarget(page, "container");
    await expect(image.locator("details.build-plan-fold")).toHaveAttribute("open", "");
    await page.evaluate(() => { (document.activeElement as HTMLElement)?.blur(); document.scrollingElement!.scrollTop = 0; });
    await page.screenshot({ path: info.outputPath(`build-plan-image-${locale}-${theme}.png`), fullPage: true });
    await page.screenshot({ path: info.outputPath(`build-plan-image-${locale}-${theme}-viewport.png`) });
  });
}

test("320px and a single OCI target keep target switching and metadata readable", async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 800 });
  const state = await setup(page, { plans: [plan(2, { container })] });
  await state.open();
  await expect(targetPanel(page, "container")).toBeVisible();
  await expect(page.getByRole("combobox", { name: "构建目标", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
});
