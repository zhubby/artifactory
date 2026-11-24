/**
 * [INPUT]: 依赖真实通知浮层、概览聚合、浏览器几何信息和受控 API 失败。
 * [OUTPUT]: 验证浮层布局、顶栏访问、计时暂停及交互期间的来源隔离与轮询恢复。
 * [POS]: 通知机制的桌面、平板和手机回归；不访问线上数据。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Locator, type Page } from "@playwright/test";
import { dashboardFixture, fixtureAPI, tenantURL } from "./fixtures";

const errors = (page: Page) => page.locator('[data-notify][data-kind="error"]');
const successes = (page: Page) => page.locator('[data-notify][data-kind="success"]');

async function expectCentered(page: Page, notice: Locator) {
  await expect(notice).toBeVisible();
  const box = (await notice.boundingBox())!;
  const width = page.viewportSize()!.width;
  expect(Math.abs(box.x + box.width / 2 - width / 2)).toBeLessThan(2);
  expect(box.y).toBeGreaterThanOrEqual(width <= 640 ? 72 : 10);
  expect(box.y).toBeLessThan(width <= 640 ? 110 : 65);
  expect(box.x).toBeGreaterThanOrEqual(10);
  expect(box.x + box.width).toBeLessThanOrEqual(width - 10);
}

test("login failure and native validation notify without moving the form", async ({ page }, info) => {
  await fixtureAPI(page);
  const detail = "服务暂时不可用。".repeat(100);
  await page.route("**/api/v1/auth/login", route => route.fulfill({ status: 503, json: { error: detail } }));
  await page.goto("/login");
  const submit = page.getByRole("button", { name: "进入工作空间", exact: true });
  const form = page.locator("form").filter({ has: submit });
  const before = await form.boundingBox();
  await submit.click();
  await expectCentered(page, errors(page));
  await expect(page.locator('input[name="username"]')).toBeFocused();
  await expect(form.locator('[role="alert"]')).toHaveCount(0);
  expect(await form.boundingBox()).toEqual(before);
  const preferences = page.getByRole("button", { name: "界面偏好", exact: true });
  if (await preferences.isVisible()) {
    const control = (await preferences.boundingBox())!;
    const notice = (await errors(page).boundingBox())!;
    const separated = notice.y >= control.y + control.height + 8
      || notice.x + notice.width <= control.x - 8;
    expect(separated).toBe(true);
    await preferences.click({ timeout: 2000 });
    await expect(page.getByRole("dialog")).toBeVisible();
    await expect(errors(page)).toBeVisible();
    await page.keyboard.press("Escape");
  }
  await page.locator('input[name="username"]').fill("notification-draft");
  await page.locator('input[name="password"]').fill("retained-password");
  const filled = await form.boundingBox();
  await submit.click();
  await expect(errors(page)).toHaveCount(1);
  await expect(errors(page)).toContainText(detail);
  await expectCentered(page, errors(page));
  expect(await form.boundingBox()).toEqual(filled);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
  await page.screenshot({ path: info.outputPath("login-notification.png"), fullPage: true });
  await errors(page).getByRole("button", { name: "关闭通知" }).click();
  await expect(errors(page)).toHaveCount(0);
  await expect(page.locator('input[name="username"]')).toHaveValue("notification-draft");
});

test("modal feedback preserves geometry and allows pointer and keyboard dismissal", async ({ page }, info) => {
  await fixtureAPI(page);
  let failure = false;
  let account = { id: "user-main", username: "alex", display_name: "Alex" };
  await page.route("**/api/v1/auth/me", route => {
    if (route.request().method() === "PATCH") {
      if (failure) return route.fulfill({ status: 503, json: { error: "请稍后重试保存" } });
      account = { ...account, ...route.request().postDataJSON() };
    }
    return route.fulfill({ json: account });
  });
  await page.goto(tenantURL("/projects/desktop-client"));
  await page.locator(".account-trigger").click();
  await page.getByRole("menuitem", { name: "个人设置", exact: true }).click();
  const dialog = page.getByRole("dialog");
  const field = dialog.getByRole("textbox", { name: "显示名称", exact: true });
  await field.fill("Alex saved");
  const before = await dialog.boundingBox();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(successes(page)).toContainText("修改已保存");
  await expectCentered(page, successes(page));
  expect(await dialog.boundingBox()).toEqual(before);
  await expect(dialog.locator('[data-notify]')).toHaveCount(0);
  await page.keyboard.press("F8");
  await expect(successes(page).getByRole("button", { name: "关闭通知" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(successes(page)).toHaveCount(0);
  await expect(dialog).toBeVisible();
  failure = true;
  await field.fill("Alex unsaved draft");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(errors(page)).toContainText("请稍后重试保存");
  expect(await dialog.boundingBox()).toEqual(before);
  await page.screenshot({ path: info.outputPath("modal-notification.png"), fullPage: true });
  let confirmations = 0;
  page.on("dialog", async prompt => { confirmations++; await prompt.dismiss(); });
  await errors(page).getByRole("button", { name: "关闭通知" }).click();
  await expect(dialog).toBeVisible();
  await expect(field).toHaveValue("Alex unsaved draft");
  expect(confirmations).toBe(0);
  failure = false;
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(successes(page)).toContainText("修改已保存");
  await expect(errors(page)).toHaveCount(0);
});

test("unchanged polling failures do not reopen dismissed notifications, manual retry does", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let calls = 0;
  await page.route("**/api/v1/projects", route => {
    calls++;
    return route.fulfill({ status: 503, json: { error: "项目查询暂不可用" } });
  });
  await page.goto(tenantURL("/"));
  await expect(errors(page)).toHaveText(/项目查询暂不可用/);
  const firstCalls = calls;
  await errors(page).getByRole("button", { name: "关闭通知" }).click();
  await page.clock.fastForward(30000);
  await expect.poll(() => calls).toBeGreaterThan(firstCalls);
  await expect(errors(page)).toHaveCount(0);
  await page.getByRole("button", { name: "重试", exact: true }).click();
  await expect(errors(page)).toContainText("项目查询暂不可用");
  await expect(errors(page)).toHaveCount(1);
});

test("login success survives the full navigation exactly once", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/auth/login", route => route.fulfill({ json: { ok: true } }));
  await page.goto("/login");
  await page.locator('input[name="username"]').fill("alex");
  await page.locator('input[name="password"]').fill("retained-password");
  await page.getByRole("button", { name: "进入工作空间", exact: true }).click();
  await expect(page).toHaveURL(/\/tenants$/);
  await expect(successes(page)).toContainText("操作成功");
  await expectCentered(page, successes(page));
  await page.reload();
  await expect(page.locator("#app-notifications")).toBeAttached();
  await expect(successes(page)).toHaveCount(0);
});

test("independent failures keep their ownership through recovery and recurrence", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let dashboardFail = true;
  let dashboardCalls = 0;
  await page.route(/\/api\/v1\/dashboard\?/, route => {
    dashboardCalls++;
    return dashboardFail
      ? route.fulfill({ status: 503, json: { error: "查询服务暂不可用" } })
      : route.fulfill({ json: dashboardFixture({ projectId: "project-main" }) });
  });
  await page.route("**/api/v1/projects/*/notification-subscription", route => route.fulfill({ status: 503, json: { error: "查询服务暂不可用" } }));
  await page.goto(tenantURL("/projects/desktop-client"));
  await expect(errors(page)).toHaveCount(2);
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1000));
  // 悬停和键盘焦点分别暂停两个来源，避免轮询等待触发通知超时。
  await errors(page).first().hover();
  await errors(page).last().getByRole("button", { name: "关闭通知" }).focus();
  const initialCalls = dashboardCalls;
  dashboardFail = false;
  await page.clock.fastForward(10000);
  await expect.poll(() => dashboardCalls).toBeGreaterThan(initialCalls);
  await expect(errors(page)).toHaveCount(1);
  await expect(errors(page)).toContainText("查询服务暂不可用");
  await page.getByRole("combobox", { name: "统计时间范围", exact: true }).focus();
  await page.keyboard.press("F8");
  await expect(errors(page).getByRole("button", { name: "关闭通知" })).toBeFocused();
  dashboardFail = true;
  await page.clock.fastForward(10000);
  await expect(errors(page)).toHaveCount(2);
});

test("notification timers pause during interaction and expire after the remaining duration", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let failure = true;
  await page.route("**/api/v1/auth/login", route => failure
    ? route.fulfill({ status: 503, json: { error: "稍后重试登录" } })
    : route.fulfill({ json: { ok: true } }));
  await page.goto("/login");
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1000));
  const username = page.locator('input[name="username"]');
  await username.fill("alex");
  await page.locator('input[name="password"]').fill("retained-password");
  const submit = page.getByRole("button", { name: "进入工作空间", exact: true });
  await submit.click();
  await expect(errors(page)).toContainText("稍后重试登录");
  await page.clock.fastForward(4000);
  await errors(page).hover();
  await page.clock.fastForward(15000);
  await expect(errors(page)).toBeVisible();
  await page.mouse.move(0, page.viewportSize()!.height - 1);
  await page.clock.fastForward(2000);
  await page.keyboard.press("F8");
  await expect(errors(page).getByRole("button", { name: "关闭通知" })).toBeFocused();
  await page.clock.fastForward(15000);
  await expect(errors(page)).toBeVisible();
  await username.focus();
  await page.clock.fastForward(2000);
  await expect(errors(page)).toBeVisible();
  await page.clock.fastForward(3000);
  await expect(errors(page)).toHaveCount(0);
  failure = false;
  await submit.click();
  await expect(page).toHaveURL(/\/tenants$/);
  await expect(successes(page)).toBeVisible();
  await page.clock.fastForward(4000);
  await expect(successes(page)).toBeVisible();
  await page.clock.fastForward(1500);
  await expect(successes(page)).toHaveCount(0);
});
