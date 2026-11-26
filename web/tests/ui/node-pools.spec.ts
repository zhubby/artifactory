/**
 * [INPUT]: 依赖 Playwright、租户夹具与真实控制台组件。
 * [OUTPUT]: 验证节点池导航、写入流程、只读权限与设备筛选。
 * [POS]: 节点池管理的浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { fixtureAPI, nodes, openNavigation, tenantURL } from "./fixtures";

test("tenant sidebar opens node pools and administrators create, rename and delete a pool", async ({ page }) => {
  await fixtureAPI(page);
  let pools = [{ id: "pool-main", name: "default", is_default: true, node_count: 2, online_count: 1 }];
  await page.route("**/api/v1/pools{,/*}", async route => {
    const method = route.request().method();
    if (method === "GET") return route.fulfill({ json: pools });
    const id = route.request().url().split("/").at(-1);
    if (method === "POST") pools.push({ id: "pool-new", name: route.request().postDataJSON().name, is_default: false, node_count: 0, online_count: 0 });
    if (method === "PATCH") pools = pools.map(pool => pool.id === id ? { ...pool, name: route.request().postDataJSON().name } : pool);
    if (method === "DELETE") pools = pools.filter(pool => pool.id !== id);
    return route.fulfill({ json: { id: "pool-new", ok: true } });
  });
  await page.goto(tenantURL("/"));
  await openNavigation(page);
  await page.locator(".sidebar").getByRole("link", { name: "节点池", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/pools"));
  await expect(page.getByRole("heading", { name: "节点池", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "创建节点池", exact: true }).click();
  let dialog = page.getByRole("dialog");
  await dialog.getByRole("textbox", { name: "节点池名称" }).fill("Linux builders");
  await dialog.getByRole("button", { name: "创建节点池", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  let card = page.getByRole("article").filter({ has: page.getByRole("heading", { name: "Linux builders", exact: true }) });
  await card.getByRole("button", { name: "重命名", exact: true }).click();
  dialog = page.getByRole("dialog");
  await dialog.getByRole("textbox", { name: "节点池名称" }).fill("macOS builders");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  card = page.getByRole("article").filter({ has: page.getByRole("heading", { name: "macOS builders", exact: true }) });
  await card.getByRole("button", { name: "删除", exact: true }).click();
  dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("macOS builders");
  await dialog.getByRole("button", { name: "删除节点池", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "macOS builders", exact: true })).toHaveCount(0);
  await expect(page.getByRole("article").filter({ hasText: "default" }).getByRole("button", { name: "删除", exact: true })).toBeDisabled();
});

test("members can inspect pools and follow an exact device filter without administrative actions", async ({ page }) => {
  const events = await fixtureAPI(page, "viewer");
  await page.goto(tenantURL("/pools"));
  await expect(page.getByRole("heading", { name: "节点池", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /创建节点池|重命名|^删除$/ })).toHaveCount(0);
  await page.getByRole("link", { name: "查看设备", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/nodes?pool=pool-main"));
  await expect(page.getByRole("heading", { name: nodes[0].name, exact: true })).toBeVisible();
  const requests: string[] = [];
  page.on("request", request => { if (request.url().includes("/api/v1/nodes?")) requests.push(request.url()); });
  await page.reload();
  await expect.poll(() => requests.some(url => new URL(url).searchParams.get("pool_id") === "pool-main")).toBe(true);
  expect(events.writes).toEqual([]);
});

test("failed pool writes preserve drafts and failed first reads offer retry", async ({ page }) => {
  await fixtureAPI(page);
  let failed = true;
  await page.route("**/api/v1/pools", route => route.fulfill(failed || route.request().method() !== "GET"
    ? { status: 503, json: { error: "Pool service unavailable" } }
    : { json: [] }));
  await page.goto(tenantURL("/pools"));
  await page.getByRole("button", { name: "重试", exact: true }).click();
  failed = false;
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await page.getByRole("button", { name: "创建节点池", exact: true }).click();
  const dialog = page.getByRole("dialog");
  const input = dialog.getByRole("textbox", { name: "节点池名称" });
  await input.fill("Persistent draft");
  await dialog.getByRole("button", { name: "创建节点池", exact: true }).click();
  await expect(input).toHaveValue("Persistent draft");
  await expect(dialog.getByRole("button", { name: "创建节点池", exact: true })).toBeEnabled();
});

test("pool names and actions remain accessible at narrow widths in both languages", async ({ page, baseURL }, info) => {
  await fixtureAPI(page);
  if (info.project.name === "mobile") await page.setViewportSize({ width: 320, height: 800 });
  for (const locale of ["zh-CN", "en"]) {
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: baseURL! }]);
    await page.route("**/api/v1/pools", route => route.fulfill({ json: [{
      id: "pool-main", name: "Linux构建设备池".repeat(5), is_default: true, node_count: 1501, online_count: 1200,
    }] }));
    await page.goto(tenantURL("/pools"));
    await expect(page.getByRole("heading", { name: locale === "en" ? "Node pools" : "节点池", exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: locale === "en" ? "View devices" : "查看设备", exact: true })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)).toBeLessThanOrEqual(1);
    const audit = await new AxeBuilder({ page }).include(".workspace").withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
    expect(audit.violations.map(item => ({ id: item.id, targets: item.nodes.map(node => node.target) }))).toEqual([]);
    await page.screenshot({ path: info.outputPath(`pools-${locale}.png`), fullPage: true });
  }
});
