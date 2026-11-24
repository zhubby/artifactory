/**
 * [INPUT]: 依赖真实全局工具、页面标题和项目订阅 API 夹具。
 * [OUTPUT]: 验证动作归位、工具顺序、320px 边界和关注写入恢复。
 * [POS]: 页面入口职责与窄屏交互的浏览器回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, project, tenantId, tenantURL } from "./fixtures";

test("creation actions stay in main page titles and never enter the global topbar", async ({ page }) => {
  await fixtureAPI(page);
  for (const [path, name, role] of [
    ["/projects", "接入项目", "button"],
    ["/projects/desktop-client", "新建 Release", "link"],
    ["/projects/desktop-client/releases", "新建 Release", "link"],
    ["/nodes", "注册新节点", "button"],
  ] as const) {
    await page.goto(tenantURL(path));
    await expect(page.locator("main .page-title").getByRole(role, { name, exact: true })).toBeVisible();
    await expect(page.locator(".workspace-topbar").getByRole(role, { name, exact: true })).toHaveCount(0);
    await expect(page.locator("#workspace-page-actions")).toHaveCount(0);
  }
});

for (const theme of ["dark", "light"]) {
  test(`${theme}: global tools keep documentation before the rightmost bell`, async ({ page }, info) => {
    await fixtureAPI(page);
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    // 默认项目覆盖桌面、平板、390px；手机补充最窄320px。
    const widths = info.project.name === "mobile" ? [390, 320] : [page.viewportSize()!.width];
    for (const width of widths) {
      await page.setViewportSize({ width, height: 844 });
      for (const path of ["/tenants", tenantURL("/projects/desktop-client"), "/notifications"]) {
        await page.goto(path);
        const tools = page.locator(".header-tools");
        const bell = tools.getByRole("button", { name: /通知中心/ });
        const docs = tools.getByRole("link", { name: "文档", exact: true });
        await expect(bell).toBeVisible();
        await expect(docs).toBeVisible();
        await expect(page.locator(".sidebar").getByRole("link", { name: "文档", exact: true })).toHaveCount(0);
        const [docBounds, bellBounds] = await Promise.all([docs.boundingBox(), bell.boundingBox()]);
        expect(docBounds!.x + docBounds!.width).toBeLessThanOrEqual(bellBounds!.x + 1);
        expect(Math.abs(docBounds!.y + docBounds!.height / 2 - bellBounds!.y - bellBounds!.height / 2)).toBeLessThan(2);
        expect(bellBounds!.x + bellBounds!.width).toBeLessThanOrEqual(width);
        expect(await bell.evaluate(element => element.parentElement?.lastElementChild === element)).toBe(true);
        if (width < 1024 || path !== tenantURL("/projects/desktop-client")) {
          const account = page.locator(".account-trigger");
          await expect(account).toBeVisible();
          const bounds = (await account.boundingBox())!;
          expect(bounds.x + bounds.width).toBeLessThanOrEqual(bellBounds!.x + 1);
          expect(Math.abs(bounds.y + bounds.height / 2 - bellBounds!.y - bellBounds!.height / 2)).toBeLessThan(2);
          if (width === 320) {
            expect(bounds.width).toBeGreaterThanOrEqual(44);
            expect(bounds.height).toBeGreaterThanOrEqual(44);
          }
        }
        if (width === 320) {
          expect(docBounds!.width).toBeGreaterThanOrEqual(44);
          expect(docBounds!.height).toBeGreaterThanOrEqual(44);
          expect(bellBounds!.width).toBeGreaterThanOrEqual(44);
          expect(bellBounds!.height).toBeGreaterThanOrEqual(44);
        }
        await docs.focus();
        await page.keyboard.press("Tab");
        if (width < 1024 || path !== tenantURL("/projects/desktop-client")) {
          await expect(page.locator(".account-trigger")).toBeFocused();
          await page.keyboard.press("Tab");
        }
        await expect(bell).toBeFocused();
        await page.keyboard.press("Enter");
        const inbox = page.getByRole("dialog", { name: "通知中心", exact: true });
        await expect(inbox).toBeVisible();
        await page.keyboard.press("Escape");
        await expect(inbox).toBeHidden();
        await expect(bell).toBeFocused();
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      }
    }
  });
}

test("project follow is a title star, stays stable after a failed write and retries once", async ({ page }) => {
  await fixtureAPI(page);
  let releaseRead!: () => void;
  const reading = new Promise<void>(resolve => { releaseRead = resolve; });
  let releaseWrite!: () => void;
  const writing = new Promise<void>(resolve => { releaseWrite = resolve; });
  const writes: unknown[] = [];
  let following = false;
  await page.route(`**/api/v1/projects/${project.id}/notification-subscription`, async route => {
    expect(route.request().headers()["x-artifactory-tenant"]).toBe(tenantId);
    if (route.request().method() === "GET") await reading;
    else {
      writes.push(route.request().postDataJSON());
      if (writes.length === 1) {
        await writing;
        return route.fulfill({ status: 503, json: { error: "关注服务暂不可用" } });
      }
      following = route.request().postDataJSON().following;
    }
    return route.fulfill({ json: { following } });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}`));
    const star = page.getByRole("button", { name: "关注项目", exact: true });
    await expect(star).toBeDisabled();
    releaseRead();
    await expect(star).toBeEnabled();
    await expect(star).toHaveAttribute("aria-pressed", "false");
    const title = page.locator("main .page-title-heading");
    await expect(title.getByRole("heading", { name: project.name, exact: true })).toBeVisible();
    await expect(title.getByRole("button", { name: "关注项目", exact: true })).toHaveCount(1);
    await expect(star.locator("svg")).toHaveAttribute("fill", "none");
    await expect(page.locator(".workspace-topbar").getByRole("button", { name: /关注项目/ })).toHaveCount(0);
    await star.click();
    await expect(star).toBeDisabled();
    releaseWrite();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("关注服务暂不可用");
    await expect(star).toBeEnabled();
    await expect(star).toHaveAttribute("aria-pressed", "false");
    await star.click();
    const followed = page.getByRole("button", { name: "取消关注项目", exact: true });
    await expect(followed).toHaveAttribute("aria-pressed", "true");
    await expect(followed.locator("svg")).toHaveAttribute("fill", "currentColor");
    expect(writes).toEqual([{ following: true }, { following: true }]);
    for (const suffix of ["/releases", "/settings/general"]) {
      await page.goto(tenantURL(`/projects/${project.slug}${suffix}`));
      await expect(page.locator("main h1")).toBeVisible();
      await expect(page.getByRole("button", { name: /^(关注项目|取消关注项目)$/ })).toHaveCount(0);
    }
  } finally {
    releaseRead();
    releaseWrite();
  }
});
