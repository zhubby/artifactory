/**
 * [INPUT]: 依赖真实控件、业务夹具和三种视口的 Playwright 配置。
 * [OUTPUT]: 验证筛选同行、统一高度、局部滚动和下拉焦点边界。
 * [POS]: 跨列表、概览和通知页面的筛选布局回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Locator, type Page } from "@playwright/test";
import { fixtureAPI, project, tenantId, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

async function expectSingleRow(group: Locator, count: number) {
  const controls = group.locator(".workspace-filter, .select-trigger");
  await expect(controls).toHaveCount(count);
  const boxes = await controls.evaluateAll(elements => elements.map(element => {
    const box = element.getBoundingClientRect();
    return { top: box.top, height: box.height };
  }));
  for (const box of boxes) {
    expect(Math.abs(box.top - boxes[0].top)).toBeLessThanOrEqual(1);
    expect(Math.abs(box.height - boxes[0].height)).toBeLessThanOrEqual(1);
  }
  expect(boxes[0].height).toBe(group.page().viewportSize()!.width < 768 ? 44 : 36);
  await expectNoPageOverflow(group.page());
}

async function expectNoPageOverflow(page: Page) {
  expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBeLessThanOrEqual(1);
}

test("business list search, status and order stay on one line", async ({ page }, info) => {
  await fixtureAPI(page);
  for (const [path, count] of [
    ["/projects/desktop-client/releases", 3],
    ["/projects/desktop-client/builds", 3],
    ["/projects/desktop-client/artifacts", 2],
    ["/nodes", 2],
    ["/pools", 1],
  ] as const) {
    await page.goto(tenantURL(path));
    await expect(page.locator(".list-count")).toBeVisible();
    await expectSingleRow(page.locator(".list-toolbar-controls"), count);
    if (path.endsWith("/releases")) {
      await page.screenshot({ path: info.outputPath("release-filters.png"), fullPage: true });
    }
  }
});

test("scrolling filters keep keyboard focus and the dropdown visible", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/releases"));
  const sort = page.getByRole("combobox", { name: "排序", exact: true });
  await page.getByRole("textbox", { name: "筛选当前列表", exact: true }).focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  await expect(sort).toBeFocused();
  const triggerBox = (await sort.boundingBox())!;
  await page.keyboard.press("Space");
  const menu = page.getByRole("listbox");
  await expect(menu).toBeVisible();
  await expect.poll(async () => {
    const box = await menu.boundingBox();
    if (!box) return false;
    return Math.abs(box.width - triggerBox.width) <= 1
      && box.x >= 11
      && box.x + box.width <= page.viewportSize()!.width - 11;
  }).toBe(true);
  await page.keyboard.press("Escape");
  await expect(sort).toBeFocused();
  await chooseOption(sort, "oldest");
  await expect(sort).toHaveAttribute("data-value", "oldest");
  await page.getByRole("textbox", { name: "筛选当前列表", exact: true }).fill("no-result");
  await expect(page.locator("tbody tr")).toHaveCount(0);
  await expectSingleRow(page.locator(".list-toolbar-controls"), 3);
});

test("notification scope filters remain aligned with long project names", async ({ page }, info) => {
  const events = await fixtureAPI(page);
  await page.route("**/api/v1/notifications/scopes", route => route.fulfill({ json: {
    tenants: [{ id: tenantId, name: "Build Team", slug: "build-team" }],
    projects: [{ id: project.id, name: "Desktop Client with an exceptionally long project name", slug: project.slug, tenant_id: tenantId }],
  } }));
  await page.goto("/notifications");
  const filters = page.locator(".inbox-filters");
  await expectSingleRow(filters, 3);
  await chooseOption(filters.getByRole("combobox").nth(2), project.id);
  await expectSingleRow(filters, 3);
  expect((await filters.getByRole("combobox").nth(2).boundingBox())!.width).toBeLessThanOrEqual(220);
  await page.screenshot({ path: info.outputPath("notification-filters.png"), fullPage: true });
  expect(events.writes).toEqual([]);
});

test("standalone searches and dashboard ranges share control sizing", async ({ page }) => {
  await fixtureAPI(page);
  const height = page.viewportSize()!.width < 768 ? 44 : 36;
  for (const path of ["/projects", "/tenants"]) {
    await page.goto(tenantURL(path));
    const search = page.locator(".workspace-filter").first();
    await expect(search).toBeVisible();
    expect((await search.boundingBox())!.height).toBe(height);
    await expectNoPageOverflow(page);
  }
  await page.goto(tenantURL("/"));
  const range = page.getByRole("combobox", { name: "统计时间范围", exact: true });
  await expect(range).toBeVisible();
  expect((await range.boundingBox())!.height).toBe(height);
  await expectNoPageOverflow(page);
  if (test.info().project.name === "desktop") {
    await page.setViewportSize({ width: 680, height: 1000 });
    await expect.poll(async () => (await range.boundingBox())?.height).toBe(44);
    await expectNoPageOverflow(page);
  }
});
