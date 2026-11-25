/**
 * [INPUT]: 依赖真实构建页面与包含乱序结果、提案和分片的审计响应。
 * [OUTPUT]: 验证独立审计标签、精确调用关联与切换后的阅读状态。
 * [POS]: AI 审计阅读回归；不调用模型或创建真实构建。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";

const session = { run_id: "audit-initial", generation: 1, purpose: "initial", status: "succeeded", model: "custom-model", pod_uid: "pod-audit", source_digest: "a".repeat(64), started_at: build.created_at, ended_at: build.created_at };
const msg = (sequence: number, payload: Record<string, unknown>, kind = "message") => ({ sequence, kind, payload, created_at: build.created_at });
const entries = [
  msg(1, { text: "Inspect pinned source only." }, "system"),
  msg(2, { role: "user", content: JSON.stringify({ build_id: build.id, goal: "Build native application", commit: "a".repeat(40), target_requirements: [{ key: "macos", os: "darwin", arch: "arm64" }] }) }),
  msg(3, { role: "assistant", content: [{ type: "text", text: "I will check the project entry points." }, { type: "toolCall", id: "read-a", name: "read_file", arguments: { path: "README.md" } }, { type: "toolCall", id: "read-b", name: "read_file", arguments: { path: "Dockerfile" } }] }),
  msg(4, { role: "toolResult", toolCallId: "read-b", toolName: "read_file", durationMs: 7, isError: true, content: [{ type: "text", text: "file not found: Dockerfile" }] }),
  msg(5, { role: "user", content: "Planning budget: 19 model calls remain, including the next request; 597 seconds remain. Keep source reads focused and reserve calls to finish with submit_plan with recipe.version=1 or request_input for a necessary question." }),
  msg(6, { role: "toolResult", toolCallId: "read-a", toolName: "read_file", durationMs: 1, isError: false, content: [{ type: "text", text: JSON.stringify({ path: "README.md", content: "# Build demo\n\nRead the lock file.\n\n<script>window.auditInjected=true</script>" }) }] }),
  msg(7, { role: "user", content: "Planning budget: please prioritize the native target." }),
  msg(8, { role: "assistant", content: [{ type: "toolCall", id: "submit-a", name: "submit_plan", arguments: { plan: { summary: "Build with the checked-in scripts", recipe: { version: 1, targets: { macos: { os: "darwin", arch: "arm64", steps: [{ name: "build", run: "sh build.sh" }] } } } } } }] }),
  msg(9, { name: "submit_plan", tool_call_id: "submit-a", proposal_id: "proposal-a", arguments: { plan: { summary: "Build with the checked-in scripts" } } }, "proposal_submitted"),
  msg(10, { name: "submit_plan", proposal_id: "proposal-a", result: { accepted: true, plan_revision_id: "plan-a" } }, "proposal_result"),
  msg(11, { message_id: "chunk-a", kind: "message", index: 0, total: 2, text: '{"role":"assistant","content":[{"type":"text","text":"Completed audit' }, "message_chunk"),
];

async function openAudit(page: Page, locale = "zh-CN", theme = "light", live = false) {
  await fixtureAPI(page);
  await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: "http://127.0.0.1" }]);
  await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
  await page.route(`**/api/v1/builds/${build.id}/plans`, route => route.fulfill({ json: { plans: [], runs: [], events: [] } }));
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations`, route => route.fulfill({ json: { conversations: [{ ...session, status: live ? "running" : session.status }, { ...session, run_id: "audit-revision", purpose: "revision", status: "interrupted", generation: 2 }] } }));
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations/*/*/messages?*`, route => {
    const url = new URL(route.request().url());
    return route.fulfill({ json: url.pathname.includes("audit-revision") ? { messages: [msg(1, { role: "assistant", content: [{ type: "text", text: "Revision evidence retained." }] })], next_cursor: null }
      : url.searchParams.get("after") === "0" ? { messages: entries, next_cursor: 11 }
      : { messages: [msg(12, { message_id: "chunk-a", kind: "message", index: 1, total: 2, text: ' after pagination."}]}' }, "message_chunk")], next_cursor: null } });
  });
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: { ...build, status: live ? "planning" : "succeeded", jobs: [], goal: "Build native application" } }));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  await page.getByRole("tab", { name: locale === "en" ? "AI audit" : "AI 审计", exact: true }).click();
  return page.locator(".planning-audit");
}

test("audit follows Plan as a unique lazy tab even without a build goal or conversation", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({ json: { ...build, status: "failed", goal: null, jobs: [] } }));
  let reads = 0;
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations`, route => {
    reads++;
    return route.fulfill({ json: { conversations: [] } });
  });
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  const tabs = page.getByRole("tablist", { name: "构建详情", exact: true });
  await expect(tabs.getByRole("tab")).toHaveText(["过程", "执行", "计划", "AI 审计", "制品"]);
  for (const name of ["过程", "执行", "计划", "制品"]) {
    await tabs.getByRole("tab", { name, exact: true }).click();
    await expect(page.locator(".planning-audit")).toBeHidden();
    expect(reads).toBe(0);
  }
  await tabs.getByRole("tab", { name: "计划", exact: true }).click();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "AI 审计", exact: true })).toBeFocused();
  await expect(page.locator(".planning-audit")).toHaveCount(1);
  await expect(page.locator(".planning-audit")).toContainText("尚无规划对话。");
  await expect(page.locator(".build-plan .planning-audit, .build-process .planning-audit")).toHaveCount(0);
  await expect(page.locator(".planning-audit-shell > summary")).toHaveCount(0);
  expect(reads).toBe(1);
});

test("pairs same-name tools by identity and renders nested file text without executing markup", async ({ page }) => {
  const audit = await openAudit(page);
  await audit.getByRole("button", { name: /读取文件.*README.md/ }).click();
  const detail = audit.getByRole("region", { name: "动作详情", exact: true });
  await expect(detail.getByRole("heading", { name: "Build demo", exact: true })).toBeVisible();
  await expect(detail.getByText("Read the lock file.", { exact: true })).toBeVisible();
  await expect(detail.getByText("1 ms", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as Record<string, unknown>).auditInjected)).toBeUndefined();
  await audit.getByRole("button", { name: /读取文件.*Dockerfile/ }).click();
  await expect(detail.getByText("file not found: Dockerfile", { exact: true })).toBeVisible();
  await expect(detail.getByText("7 ms", { exact: true })).toBeVisible();
  await expect(audit.getByRole("button", { name: /提交构建计划/ })).toHaveCount(1);
  await audit.getByRole("button", { name: /提交构建计划/ }).click();
  await expect(detail.getByText("平台已接纳", { exact: true }).first()).toBeVisible();
});

test("task input leads with the goal and keeps remaining context folded", async ({ page }) => {
  const audit = await openAudit(page);
  await audit.getByRole("button", { name: /任务输入/ }).click();
  const detail = audit.getByRole("region", { name: "动作详情", exact: true });
  await expect(detail.locator(".audit-context-primary dt").first()).toHaveText("构建目标");
  await expect(detail.locator(".audit-context-primary")).toContainText("Build native application");
  const extra = detail.locator(".audit-context-extra");
  await expect(extra).not.toHaveAttribute("open", "");
  await extra.locator("summary").click();
  await expect(extra).toContainText(build.id);
});

test("background audit polling preserves the selected raw record and reading position", async ({ page }) => {
  await page.clock.install();
  const audit = await openAudit(page, "zh-CN", "light", true);
  await audit.getByRole("button", { name: /读取文件.*README.md/ }).click();
  await audit.getByRole("button", { name: "加载更多消息", exact: true }).click();
  await audit.getByRole("tab", { name: "原始记录", exact: true }).click();
  const raw = audit.locator(".audit-raw:visible");
  const body = audit.locator(".audit-detail-body:visible");
  await body.evaluate(element => { element.scrollTop = 70; });
  const before = await body.evaluate(element => element.scrollTop);
  expect(before).toBeGreaterThan(0);
  let polls = 0;
  await page.route(`**/api/v1/builds/${build.id}/planning/conversations/audit-initial/1/messages?*`, route => {
    polls++;
    return route.fulfill({ json: { messages: [msg(13, { role: "assistant", content: "New audit evidence arrived." })], next_cursor: null } });
  });
  await page.clock.fastForward(3100);
  await expect(audit.getByRole("button", { name: /New audit evidence arrived/ })).toBeAttached();
  expect(polls).toBeGreaterThan(0);
  await expect(audit.getByRole("tab", { name: "原始记录", exact: true })).toHaveAttribute("aria-selected", "true");
  expect(await body.evaluate(element => element.scrollTop)).toBe(before);
  await expect(raw).toContainText('"toolCallId": "read-a"');
});

test("classifies injected runtime hints separately and preserves raw records and round selection", async ({ page }) => {
  const audit = await openAudit(page);
  await audit.getByRole("button", { name: /读取文件.*README.md/ }).click();
  await audit.getByRole("tab", { name: "原始记录", exact: true }).click();
  await expect(audit.getByRole("region", { name: "动作详情", exact: true })).toContainText('"toolCallId": "read-a"');
  await audit.getByRole("button", { name: /失败修订 1/ }).click();
  await expect(audit.getByText("本次会话已中断", { exact: false })).toBeVisible();
  await audit.getByRole("button", { name: /首次规划/ }).click();
  await expect(audit.getByRole("tab", { name: "原始记录", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(audit.getByRole("button", { name: /用户消息.*Planning budget: please/ })).toBeVisible();
  await audit.locator(".audit-hints > summary").click();
  await audit.getByRole("button", { name: /平台提示/ }).click();
  await expect(audit.getByRole("region", { name: "动作详情", exact: true })).toContainText("Planning budget: 19 model calls remain");
});

test("completes chunked messages across pages without changing the selected tool", async ({ page }) => {
  const audit = await openAudit(page);
  await audit.getByRole("button", { name: /读取文件.*README.md/ }).click();
  await audit.getByRole("button", { name: "加载更多消息", exact: true }).click();
  await expect(audit.getByRole("button", { name: /模型消息.*Completed audit after pagination/ })).toBeVisible();
  await expect(audit.getByRole("region", { name: "动作详情", exact: true }).getByRole("heading", { name: "Build demo", exact: true })).toBeVisible();
});

test("switching build tabs pauses audit polling and retains the selected raw record and reading position", async ({ page }) => {
  await page.clock.install();
  const audit = await openAudit(page, "zh-CN", "light", true);
  await audit.getByRole("button", { name: /读取文件.*README.md/ }).click();
  await audit.getByRole("button", { name: "加载更多消息", exact: true }).click();
  await audit.getByRole("tab", { name: "原始记录", exact: true }).click();
  const body = audit.locator(".audit-detail-body:visible");
  await body.evaluate(element => { element.scrollTop = 70; });
  const before = await body.evaluate(element => element.scrollTop);
  expect(before).toBeGreaterThan(0);
  let requests = 0;
  page.on("request", request => { if (request.url().includes("/planning/conversations")) requests++; });
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await expect(audit).toBeHidden();
  await page.clock.fastForward(6100);
  expect(requests).toBe(0);
  await page.getByRole("tab", { name: "AI 审计", exact: true }).click();
  await expect.poll(() => requests).toBeGreaterThan(0);
  await expect(audit.getByRole("tab", { name: "原始记录", exact: true })).toHaveAttribute("aria-selected", "true");
  expect(await body.evaluate(element => element.scrollTop)).toBe(before);
  await expect(body).toContainText('"toolCallId": "read-a"');
});

for (const locale of ["zh-CN", "en"]) for (const theme of ["light", "dark"]) test(`${locale} ${theme}: audit is readable, bounded and accessible`, async ({ page }, info) => {
  const audit = await openAudit(page, locale, theme);
  await audit.getByRole("button", { name: /README.md/ }).click();
  await expect(audit.getByRole("heading", { name: "Build demo", exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  const violations = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
  expect(violations.violations.map(item => ({ id: item.id, nodes: item.nodes.map(node => node.target) }))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`audit-${locale}-${theme}.png`), fullPage: true });
});
