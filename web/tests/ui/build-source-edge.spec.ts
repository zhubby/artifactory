/**
 * [INPUT]: 依赖源码引用查询和 构建自定义建议列表。
 * [OUTPUT]: 验证建议等宽对齐、搜索、键盘、任意引用与迟到响应隔离。
 * [POS]: 构建独立源码入口的交互回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, release, repositoryRefs, tenantURL } from "./fixtures";

async function openWizard(page: Page) {
  await page.goto(tenantURL(`/projects/desktop-client/releases/${release.id}`));
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
}
const sourceInput = (page: Page) => page.locator('input[name="ref"]');

test("all repository refs remain available in a searchable custom suggestion list", async ({ page }, testInfo) => {
  const events = await fixtureAPI(page);
  const branches = Array.from({ length: 125 }, (_, index) => ({
    name: `topic/${index}`, ref: `refs/heads/topic/${index}`, oid: "a".repeat(40),
  }));
  await page.route("**/api/v1/projects/project-main/refs", route => route.fulfill({
    json: { default_ref: "refs/heads/topic/124", branches, tags: [] },
  }));
  await openWizard(page);
  const source = sourceInput(page);
  await expect(source).toHaveValue("refs/heads/topic/124");
  await expect(page.locator("datalist")).toHaveCount(0);
  await expect(source).toHaveAttribute("role", "combobox");
  await source.clear();
  const list = page.getByRole("listbox");
  await expect(list.getByRole("option")).toHaveCount(125);
  await source.fill("TOPIC/12");
  await expect(list.getByRole("option")).toHaveCount(6);
  await expect(list).toBeVisible();
  const bounds = await list.locator("..").boundingBox();
  const inputBounds = await source.boundingBox();
  expect(bounds).not.toBeNull();
  expect(inputBounds).not.toBeNull();
  expect(Math.abs(bounds!.width - inputBounds!.width)).toBeLessThanOrEqual(1);
  expect(Math.abs(bounds!.x - inputBounds!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(bounds!.x + bounds!.width - inputBounds!.x - inputBounds!.width)).toBeLessThanOrEqual(1);
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(page.viewportSize()!.width);
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(page.viewportSize()!.height);
  await page.screenshot({ path: testInfo.outputPath("source-suggestions.png"), fullPage: true });
  await list.getByRole("option", { name: "topic/123 refs/heads/topic/123", exact: true }).click();
  await expect(source).toHaveValue("refs/heads/topic/123");
  await expect(source).toBeFocused();
  await expect(list).toHaveCount(0);
  expect(events.writes).toEqual([]);
});

test("source keyboard navigation selects explicitly and preserves ordinary text cursor", async ({ page }) => {
  const events = await fixtureAPI(page);
  await openWizard(page);
  const source = sourceInput(page);
  await expect(source).toHaveValue(repositoryRefs.default_ref);
  await source.clear();
  await source.press("ArrowDown");
  await source.press("End");
  const active = await source.getAttribute("aria-activedescendant");
  await expect(page.locator(`[id="${active}"]`)).toHaveText("v1.2.0refs/tags/v1.2.0");
  await source.press("Home");
  const first = await source.getAttribute("aria-activedescendant");
  await expect(page.locator(`[id="${first}"]`)).toHaveText("mainrefs/heads/main");
  await source.press("ArrowDown");
  await source.press("Enter");
  await expect(source).toHaveValue("refs/heads/release/v1");
  await expect(source).toHaveAttribute("aria-expanded", "false");
  expect(events.writes).toEqual([]);
  await source.fill("custom-ref");
  await source.evaluate(element => {
    element.addEventListener("keydown", event => {
      if (!["Home", "End"].includes((event as KeyboardEvent).key)) return;
      queueMicrotask(() => { (element as HTMLElement).dataset.cursorPrevented = String(event.defaultPrevented); });
    });
  });
  await source.press("Home");
  await expect(source).toHaveAttribute("data-cursor-prevented", "false");
  await source.press("End");
  await expect(source).toHaveAttribute("data-cursor-prevented", "false");
  const mac = await page.evaluate(() => /Mac/.test(navigator.platform));
  await source.press(mac ? "Meta+ArrowLeft" : "Home");
  await expect.poll(() => source.evaluate(element => (element as HTMLInputElement).selectionStart)).toBe(0);
  await source.press(mac ? "Meta+ArrowRight" : "End");
  await expect.poll(() => source.evaluate(element => (element as HTMLInputElement).selectionStart)).toBe(10);
  await source.press("Enter");
  await expect(source).toHaveValue("custom-ref");
  await expect(source).toHaveAttribute("aria-expanded", "false");
  expect(events.writes).toEqual([]);
  await source.press("ArrowDown");
  await source.press("Escape");
  await expect(source).toHaveAttribute("aria-expanded", "false");
  await expect(source).toHaveValue("custom-ref");
  expect(events.writes).toEqual([]);
});

test("a late source lookup never replaces an explicit source selection", async ({ page }) => {
  const events = await fixtureAPI(page);
  let finish!: () => void;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  await page.route("**/api/v1/projects/project-main/refs", async route => {
    await pending; await route.fulfill({ json: repositoryRefs });
  });
  const requested = page.waitForRequest("**/api/v1/projects/project-main/refs");
  await openWizard(page);
  await requested;
  const source = sourceInput(page);
  await source.fill("refs/tags/custom");
  await expect(page.getByRole("listbox")).toHaveAttribute("aria-busy", "true");
  await expect(page.getByRole("listbox").locator("..").getByRole("status")).toBeVisible();
  const completed = page.waitForResponse("**/api/v1/projects/project-main/refs");
  finish(); await completed;
  await expect(page.getByRole("listbox")).toHaveAttribute("aria-busy", "false");
  await expect(source).toHaveValue("refs/tags/custom");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(0);
  await source.clear();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(3);
  expect(events.writes).toEqual([]);
});

test("failed suggestions retain arbitrary input and explicit retry restores the list", async ({ page }) => {
  const events = await fixtureAPI(page);
  let attempts = 0;
  await page.route("**/api/v1/projects/project-main/refs", route => {
    attempts += 1;
    return route.fulfill(attempts === 1
      ? { status: 502, json: { error: "Source lookup unavailable" } }
      : { json: repositoryRefs });
  });
  await openWizard(page);
  const source = sourceInput(page);
  await source.fill("unlisted/ref");
  await expect(page.getByRole("listbox").locator("..").getByRole("status")).toHaveText("请求未能完成");
  await source.press("ArrowDown");
  await source.press("Enter");
  await expect(source).toHaveValue("unlisted/ref");
  expect(events.writes).toEqual([]);
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await source.click();
  await expect(source).toHaveValue("unlisted/ref");
  await source.clear();
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(3);
  expect(events.writes).toEqual([]);
});
