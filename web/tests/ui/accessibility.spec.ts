/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证租户卡片、业务页面、侧栏与个人设置的无障碍边界。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, tenantURL, openPreferences, closePreferences } from "./fixtures";

async function audit(page: Page) {
  await page.evaluate(() => document.fonts.ready);
  const result = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
    .analyze();
  expect(result.violations.map(violation => ({
    id: violation.id,
    nodes: violation.nodes.map(node => ({ target: node.target, reason: node.failureSummary })),
  }))).toEqual([]);
}

for (const theme of ["light", "dark"]) {
  test(`${theme}: core pages and overlays meet automated WCAG AA checks`, async ({ page }) => {
    test.setTimeout(120000);
    await fixtureAPI(page);
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    await page.route("**/api/v1/auth/me", route => route.fulfill({ status: 401, json: { error: "Unauthenticated" } }));
    await page.goto("/login");
    await expect(page.locator('input[name="username"]')).toBeVisible();
    await audit(page);
    await page.unroute("**/api/v1/auth/me");
    await page.goto("/tenants");
    await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
    await audit(page);
    await page.goto(tenantURL("/projects/desktop-client"));
    await expect(page.getByRole("heading", { name: "Desktop Client", exact: true })).toBeVisible();
    await audit(page);
    await page.goto(tenantURL("/projects/desktop-client/releases/new"));
    await expect(page.locator('input[name="version"]')).toBeVisible();
    await audit(page);
    await page.goto(tenantURL("/projects/desktop-client/builds/128"));
    await expect(page.getByRole("log")).toContainText("native compilation complete");
    await audit(page);
    await page.getByRole("tab", { name: "计划", exact: true }).click();
    await audit(page);
    await page.goto(tenantURL("/nodes"));
    await page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true }).click();
    await audit(page);
    await page.keyboard.press("Escape");
    await page.goto(tenantURL("/settings"));
    await expect(page.getByRole("heading", { name: "基本信息", level: 1, exact: true })).toBeVisible();
    await audit(page);
    await page.goto(tenantURL("/settings/credentials"));
    await expect(page.getByRole("heading", { name: "Git 凭据", level: 1, exact: true })).toBeVisible();
    await audit(page);
    await openPreferences(page);
    await audit(page);
    await closePreferences(page);
    await page.getByRole("button", { name: "快捷导航", exact: true }).click();
    await expect(page.getByRole("combobox", { name: "搜索项目或页面", exact: true })).toBeFocused();
    await audit(page);
  });
}
