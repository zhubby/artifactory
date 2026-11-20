/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具和共享 Select 交互助手。
 * [OUTPUT]: 验证全局错误通知、本地化切换与草稿保留。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "./select-helpers";
import { expect, test } from "@playwright/test";
import {
  fixtureAPI,
  release,
  openPreferences,
  closePreferences,
  tenantURL,
} from "./fixtures";

test("local network failures translate live without clearing the login draft", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/auth/login", (route) => route.abort("failed"));
  await page.goto(tenantURL("/login"));
  await page.locator('input[name="username"]').fill("network-draft");
  await page.locator('input[name="password"]').fill("retained-password");
  await page.getByRole("button", { name: "进入工作空间", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "无法连接服务，请检查网络后重试",
  );
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "界面语言", exact: true }), "en");
  await closePreferences(page);
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "Cannot connect to the service. Check your network and try again.",
  );
  await expect(page.locator('input[name="username"]')).toHaveValue(
    "network-draft",
  );
});

test("server error details remain verbatim when changing language", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/auth/login", (route) =>
    route.fulfill({
      status: 403,
      json: { error: "账户已停用 · account locked [E403]" },
    }),
  );
  await page.goto(tenantURL("/login"));
  await page.locator('input[name="username"]').fill("locked-user");
  await page.locator('input[name="password"]').fill("retained-password");
  await page.getByRole("button", { name: "进入工作空间", exact: true }).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "账户已停用 · account locked [E403]",
  );
  await openPreferences(page);
  await chooseOption(page.getByRole("combobox", { name: "界面语言", exact: true }), "en");
  await closePreferences(page);
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "账户已停用 · account locked [E403]",
  );
  await expect(page.locator('[data-notify][data-kind="error"] strong')).toHaveText(
    "The request could not be completed",
  );
});

test("invalid JSON is a localized form error and never reaches the backend", async ({
  page,
}) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL(`/projects/desktop-client/releases/${release.id}`));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await dialog.locator('textarea[name="parameters"]').fill('{"unfinished":');
  await dialog.getByRole("button",{name:"发起构建",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText(
    "JSON 格式无效，请检查后重试",
  );
  await expect(page.locator('textarea[name="parameters"]')).toHaveValue(
    '{"unfinished":',
  );
  expect(events.writes).toEqual([]);
});
