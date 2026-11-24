/**
 * [INPUT]: 依赖延迟 API 夹具与真实控制台组件。
 * [OUTPUT]: 验证读取骨架、失败收尾和日志分页保留。
 * [POS]: 加载界面的行为回归，不替代后台接口验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { build, fixtureAPI, tenantURL } from "./fixtures";

test("node reads show a skeleton without visible reading text and settle on failure", async ({ page }) => {
  await fixtureAPI(page);
  let finish!: () => void;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  await page.route("**/api/v1/nodes?*", async route => {
    await pending;
    await route.fulfill({ status: 503, json: { error: "Node query unavailable" } });
  });
  await page.goto(tenantURL("/nodes"));
  const content = page.locator("#workspace-content");
  const skeleton = content.locator(".loading-state");
  try {
    await expect(content.getByRole("heading", { name: "构建节点", exact: true, level: 1 })).toBeVisible();
    await expect(skeleton).toHaveAttribute("aria-busy", "true");
    await expect(skeleton).toBeVisible();
    expect(await skeleton.evaluate(node => Array.from(node.childNodes)
      .filter(child => child.nodeType === Node.TEXT_NODE).map(child => child.textContent).join(""))).not.toContain("正在读取");
    await expect(skeleton.locator(".sr-only")).toHaveText("正在读取控制平面…");
    await expect(skeleton.locator(".skeleton-block").first()).toBeVisible();
  } finally {
    finish();
  }
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Node query unavailable");
  await expect(skeleton).toHaveCount(0);
});

test("history starts with a log skeleton and keeps existing output while paging", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: {
    ...build, status: "succeeded", publish_status: "succeeded", finished_at: "2026-11-10T08:32:30Z",
    jobs: build.jobs?.map(job => ({ ...job, status: "succeeded" })),
  } }));
  let finishInitial!: () => void;
  let finishPage!: () => void;
  const initial = new Promise<void>(resolve => { finishInitial = resolve; });
  const older = new Promise<void>(resolve => { finishPage = resolve; });
  await page.route("**/api/v1/builds/build-main/logs?*", async route => {
    const paging = new URL(route.request().url()).searchParams.has("before");
    await (paging ? older : initial);
    await route.fulfill({ json: {
      logs: [{ id: paging ? 10 : 20, attempt_id: "attempt-main", step: "compile", text: paging ? "earlier output\n" : "current output\n" }],
      next_before: paging ? null : 20,
    } });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.getByRole("log");
  try {
    await expect(log.locator(".loading-state")).toBeVisible();
  } finally {
    finishInitial();
  }
  await expect(log).toContainText("current output");
  await expect(log.locator(".loading-state")).toHaveCount(0);
  const loadOlder = page.getByRole("button", { name: "加载更早日志", exact: true });
  await loadOlder.click();
  try {
    await expect(loadOlder).toBeDisabled();
    await expect(log).toContainText("current output");
    await expect(log.locator(".loading-state")).toHaveCount(0);
  } finally {
    finishPage();
  }
  await expect(log).toContainText("earlier output");
});
