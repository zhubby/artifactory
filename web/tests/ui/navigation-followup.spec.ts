/**
 * [INPUT]: 依赖账户单实例宿主、项目关注首读和响应式工作区夹具。
 * [OUTPUT]: 验证断点焦点与账户草稿保留、关注读取恢复、404边界和明暗截图。
 * [POS]: 导航审查的边界回归；截图由主任务统一浏览器运行生成。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, openNavigation, project, tenantId, tenantSlug, tenantURL } from "./fixtures";

test("account menu keeps the focused menu item while its host crosses 1023 and 1024px", async ({ page }) => {
  await page.setViewportSize({ width: 1023, height: 900 });
  await fixtureAPI(page);
  await page.goto(tenantURL(`/projects/${project.slug}`));
  const trigger = page.locator(".account-trigger");
  await expect(page.locator(".account-slot-header").locator(".account-trigger")).toBeVisible();
  await trigger.click();
  const settings = page.getByRole("menuitem", { name: "个人设置", exact: true });
  await expect(settings).toBeFocused();
  await page.keyboard.press("End");
  const logout = page.getByRole("menuitem", { name: "退出登录", exact: true });
  await expect(logout).toBeFocused();
  await logout.evaluate(node => Object.assign(window, { stableAccountMenuItem: node }));
  for (const width of [1024, 1023, 1024]) {
    await page.setViewportSize({ width, height: 900 });
    const slot = page.locator(width === 1024 ? ".account-slot-sidebar" : ".account-slot-header");
    await expect(slot.locator(".account-trigger")).toBeVisible();
    await expect(logout).toBeVisible();
    await expect(logout).toBeFocused();
    await expect(trigger).toHaveAttribute("aria-expanded", "true");
    expect(await logout.evaluate(node => node === (window as unknown as { stableAccountMenuItem: Element }).stableAccountMenuItem)).toBe(true);
  }
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toBeHidden();
  await expect(trigger).toBeFocused();
  await page.setViewportSize({ width: 1023, height: 900 });
  await expect(page.locator(".account-slot-header .account-trigger")).toBeFocused();
});

test("dirty account profile survives breakpoint changes and returns focus after a confirmed close", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 900 });
  const events = await fixtureAPI(page);
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await page.locator(".account-trigger").click();
  await page.getByRole("menuitem", { name: "个人设置", exact: true }).click();
  const panel = page.locator(".account-panel");
  const name = panel.locator('input[name="display_name"]');
  await name.fill("Breakpoint profile draft");
  await expect(panel.locator('form[data-dirty="true"]')).toBeVisible();
  await name.evaluate(node => Object.assign(window, { stableAccountDraft: node }));
  for (const width of [1023, 1024, 1023]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(name).toHaveValue("Breakpoint profile draft");
    await expect(name).toBeFocused();
    expect(await name.evaluate(node => node === (window as unknown as { stableAccountDraft: Element }).stableAccountDraft)).toBe(true);
    await expect(panel.locator('form[data-dirty="true"]')).toBeVisible();
  }
  const close = panel.locator("header").getByRole("button", { name: "关闭对话框", exact: true });
  page.once("dialog", dialog => dialog.dismiss());
  await close.click();
  await expect(panel).toBeVisible();
  await expect(name).toHaveValue("Breakpoint profile draft");
  page.once("dialog", dialog => dialog.accept());
  await close.click();
  await expect(panel).toBeHidden();
  await expect(page.locator(".account-slot-header .account-trigger")).toBeFocused();
  expect(events.writes).toEqual([]);
});

test("project follow first-read failure offers a retry and enables the unchanged star", async ({ page }) => {
  await fixtureAPI(page);
  let reads = 0;
  let writes = 0;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**/api/v1/projects/${project.id}/notification-subscription`, async route => {
    if (route.request().method() !== "GET") {
      writes++;
      return route.fulfill({ json: { following: true } });
    }
    if (++reads === 1) return route.fulfill({ status: 503, json: { error: "关注状态暂不可用" } });
    await pending;
    return route.fulfill({ json: { following: false } });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}`));
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("关注状态暂不可用");
    const retry = page.getByRole("button", { name: "重试读取项目关注状态", exact: true });
    await expect(retry).toBeEnabled();
    await expect(retry).toHaveAttribute("aria-pressed", "false");
    await retry.click();
    const star = page.getByRole("button", { name: "关注项目", exact: true });
    await expect(star).toBeDisabled();
    await expect(star).toHaveAttribute("aria-busy", "true");
    release();
    await expect(star).toBeEnabled();
    await expect(star).toHaveAttribute("aria-pressed", "false");
    await expect(star.locator("svg")).toHaveAttribute("fill", "none");
    await expect(page.locator('[data-notify][data-kind="error"]')).toHaveCount(0);
    expect(reads).toBe(2);
    expect(writes).toBe(0);
  } finally { release(); }
});

for (const theme of ["dark", "light"]) {
  test(`${theme}: capture the final overview and delayed sidebar at desktop and 320px`, async ({ page }, info) => {
    test.skip(info.project.name === "tablet", "截图分别记录桌面和320px；平板由行为回归覆盖");
    const width = info.project.name === "desktop" ? 1440 : 320;
    await page.setViewportSize({ width, height: 1000 });
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    await fixtureAPI(page);
    await page.goto(tenantURL(`/projects/${project.slug}`));
    await expect(page.getByRole("heading", { name: project.name, level: 1, exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "关注项目", exact: true })).toBeEnabled();
    await expect(page.locator("main .loading-state")).toHaveCount(0);
    await expect(page.locator(".top-progress")).toBeHidden();
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await page.evaluate(() => document.fonts.ready);
    await page.addStyleTag({ content: "nextjs-portal {display:none}" });
    await page.screenshot({ path: info.outputPath(`overview-${theme}-${width}.png`), fullPage: true });
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    await page.route("**/api/v1/tenant", async route => {
      await pending;
      await route.fulfill({ json: { tenant: { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "active", archived_at: null, owner_id: "user-main", created_at: "2026-11-10T08:30:00Z" }, project_roles: { [project.id]: "admin" } } });
    });
    try {
      await page.reload();
      await page.addStyleTag({ content: "nextjs-portal {display:none}" });
      await openNavigation(page);
      const skeleton = page.locator('.sidebar-skeleton[data-scope="project"]');
      await expect(skeleton).toBeVisible();
      await expect(skeleton).toHaveAttribute("aria-busy", "true");
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: info.outputPath(`sidebar-pending-${theme}-${width}.png`), fullPage: false });
    } finally { release(); }
  });
}


test("an invalid tenant workspace route renders only the 404 page", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto("/tenants/build-team/unknown");
  await expect(page.getByRole("heading", { name: "页面不存在", level: 1, exact: true })).toBeVisible();
  await expect(page.locator(".workspace-shell")).toHaveCount(0);
  await expect(page.locator(".header-tools")).toHaveCount(0);
});
