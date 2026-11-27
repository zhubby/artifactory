/**
 * [INPUT]: 依赖真实 Prism 控件、授权项目快照与隔离 API 夹具。
 * [OUTPUT]: 验证项目目录、响应式阅读、材质降级与交互动效。
 * [POS]: 原型设计语言进入生产界面后的跨页面回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, project, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

test("project search, lifecycle, ordering and views preserve authorized snapshots", async ({ page }) => {
  const events = await fixtureAPI(page);
  const projects = [
    { ...project, id: "older", slug: "alpha", name: "Alpha Desktop", build_count: 0, last_build: null },
    { ...project, id: "latest", slug: "zeta", name: "Zeta Service", status: "archived", build_count: 17, effective_role: "viewer" },
  ];
  await page.route("**/api/v1/projects", route => route.fulfill({ json: projects }));
  await page.goto(tenantURL("/projects"));
  const cards = page.locator(".prism-project-card");
  await expect(cards).toHaveCount(2);
  await expect(cards.first()).toContainText("Zeta Service");
  await expect(cards.first().getByRole("link", { name: /设置/ })).toHaveCount(0);
  await expect(cards.last().getByRole("link", { name: "管理项目 Alpha Desktop", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "列表视图", exact: true }).click();
  await expect(page.getByRole("button", { name: "列表视图", exact: true })).toHaveAttribute("aria-pressed", "true");
  await chooseOption(page.getByRole("combobox", { name: "项目排序", exact: true }), "name");
  await expect(cards.first()).toContainText("Alpha Desktop");
  await page.getByRole("textbox", { name: "搜索项目", exact: true }).fill("ZETA");
  await expect(cards).toHaveCount(1);
  await chooseOption(page.getByRole("combobox", { name: "筛选项目状态", exact: true }), "active");
  await expect(cards).toHaveCount(0);
  await page.getByRole("button", { name: "清除筛选", exact: true }).click();
  await expect(cards).toHaveCount(2);
  await expect(page.getByRole("textbox", { name: "搜索项目", exact: true })).toHaveValue("");
  expect(events.writes).toEqual([]);
});

test("entry, projects and devices remain readable on a small phone and in landscape", async ({ page }, info) => {
  await fixtureAPI(page);
  for (const viewport of [{ width: 375, height: 812 }, { width: 812, height: 375 }, { width: 1440, height: 1000 }]) {
    await page.setViewportSize(viewport);
    for (const path of ["/login", "/tenants", tenantURL("/projects"), tenantURL("/nodes")]) {
      await page.goto(path);
      await expect(page.locator("h1")).toBeVisible();
      await expect(page.locator(".loading-state")).toHaveCount(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth - innerWidth), JSON.stringify(await page.locator('body *').evaluateAll(nodes => nodes.map(n => ({tag:n.tagName,cls:n.className,x:n.getBoundingClientRect().x,right:n.getBoundingClientRect().right})).filter(n => n.right > innerWidth + 1 && n.x >= 0)))).toBeLessThanOrEqual(1);
      if (path.endsWith("/projects")) {
        const search = page.getByRole("textbox", { name: "搜索项目", exact: true });
        expect((await search.boundingBox())!.width).toBeGreaterThan(120);
        const filters=await page.locator('.prism-project-toolbar .select-trigger').evaluateAll(nodes=>nodes.map(node=>node.getBoundingClientRect().toJSON()));
        expect(Math.abs(filters[0].y-filters[1].y)).toBeLessThanOrEqual(1);
        expect(filters[1].x).toBeGreaterThan(filters[0].x+filters[0].width);
        await page.addStyleTag({ content: "nextjs-portal{display:none!important}" });
        await page.screenshot({ path: info.outputPath(`prism-projects-${viewport.width}.png`), fullPage: true });
      }
    }
  }
});

test("functional glass and Standard content adapt independently to accessibility preferences", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects"));
  const glass = page.locator(".workspace-topbar");
  const content = page.locator(".prism-project-feature");
  await expect(glass).toHaveCSS("backdrop-filter", "blur(10px) saturate(2.1)");
  await expect(content).toHaveCSS("backdrop-filter", "none");
  await page.evaluate(() => { document.documentElement.dataset.transparency = "reduce"; });
  await expect(glass).toHaveCSS("backdrop-filter", "none");
  await expect(page.locator(".prism-sheen").first()).toBeHidden();
  await page.evaluate(() => { delete document.documentElement.dataset.transparency; });
  await expect(glass).toHaveCSS("backdrop-filter", "blur(10px) saturate(2.1)");
  await page.emulateMedia({ contrast: "more", reducedMotion: "reduce" });
  await expect(glass).toHaveCSS("backdrop-filter", "blur(10px) saturate(2.1)");
  await page.emulateMedia({ forcedColors: "active" });
  await expect(glass).toHaveCSS("backdrop-filter", "none");
  await expect(glass).toHaveCSS("box-shadow", "none");
  await expect(page.getByRole("button", { name: "接入项目", exact: true })).toBeVisible();
});

test("glass reflections respond to navigation and settle without decorative loops", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Fine pointer enhancement");
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto(tenantURL("/projects"));
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await page.mouse.move(1200, 900);
  await page.locator(".sidebar").getByRole("link", { name: "概览", exact: true }).hover();
  await expect(page.locator(".sidebar .prism-hover-capsule.visible")).toHaveCount(1);
  await expect(page.locator(".sidebar")).toHaveAttribute("data-prism-active", "");
  await expect(page.locator(".sidebar")).not.toHaveAttribute("data-prism-active", "");
  expect(await page.locator(".sidebar").evaluate(element => element.getAnimations({ subtree: true }).filter(animation => animation.playState === "running" && animation.effect?.getComputedTiming().iterations === Infinity).length)).toBe(0);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.mouse.move(1200, 900);
  await page.locator(".sidebar").getByRole("link", { name: "项目", exact: true }).hover();
  await expect(page.locator(".sidebar")).not.toHaveAttribute("data-prism-active", "");
  await page.locator(".sidebar").getByRole("link", { name: "概览", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/"));
});
