/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证monochrome-workspace页面行为。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, build, project, tenantURL, openPreferences, closePreferences } from "./fixtures";

test("keyboard quick navigation searches authorized projects and restores focus on Escape", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  const trigger = page.getByRole("button", { name: "快捷导航", exact: true });
  await trigger.focus();
  await page.keyboard.press("Control+k");
  const search = page.getByRole("combobox", { name: "搜索项目或页面", exact: true });
  await expect(search).toBeFocused();
  await search.fill("Desktop Client");
  await expect(page.getByRole("option").filter({ hasText: "Desktop Client" }).first()).toBeVisible();
  await search.fill("not-an-accessible-project");
  await expect(page.getByRole("option")).toHaveCount(0);
  await search.fill("共享设备");
  await expect(page.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await trigger.click();
  await search.fill("共享设备");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(tenantURL("/nodes"));
  expect(events.writes).toEqual([]);
});

test("quick navigation respects unsaved build drafts and viewer permissions", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.locator('textarea[name="goal"]').fill("Preserve the exact draft");
  await page.keyboard.press("Control+k");
  await page.getByRole("combobox", { name: "搜索项目或页面", exact: true }).fill("共享设备");
  const rejected = page.waitForEvent("dialog").then(dialog => dialog.dismiss());
  await page.keyboard.press("Enter");
  await rejected;
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/builds/new"));
  await expect(page.locator('textarea[name="goal"]')).toHaveValue("Preserve the exact draft");
  expect(events.writes).toEqual([]);
});

test("quick navigation shortcuts preserve the active modal and still toggle the palette itself", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client"));
  await openPreferences(page);
  const theme = page.getByRole("combobox", { name: "主题", exact: true });
  await theme.focus();
  for (const shortcut of ["Control+k", "Meta+k"]) {
    await page.keyboard.press(shortcut);
    await expect(page.locator(".command-input")).toHaveCount(0);
    await expect(theme).toBeFocused();
  }
  await closePreferences(page);
  await page.keyboard.press("Control+k");
  await expect(page.getByRole("combobox", { name: "搜索项目或页面", exact: true })).toBeFocused();
  await page.keyboard.press("Control+k");
  await expect(page.locator(".command-input")).toHaveCount(0);

  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true }).click();
  const close = page.getByRole("button", { name: "关闭对话框", exact: true });
  await close.focus();
  await page.keyboard.press("Control+k");
  await expect(page.locator(".command-input")).toHaveCount(0);
  await expect(close).toBeFocused();
});

test("the wizard presents three stages with goal and platform validation together", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds/new"));
  await expect(page.locator(".wizard-steps li")).toHaveCount(3);
  await expect(page.locator('.wizard-steps [aria-current="step"]')).toHaveText(/源码/);
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.getByRole("radio", { name: "构建并收集产物", exact: true }).check();
  await expect(page.getByRole("radio", { name: "自动选择平台", exact: true })).toBeChecked();
  await page.getByRole("radio", { name: "指定目标平台", exact: true }).check();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.locator(".error-box")).toContainText("请至少选择一个目标平台");
  await page.getByRole("checkbox", { name: "Linux · x64", exact: true }).check();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "确认后即可发起构建", exact: true })).toBeVisible();
  await expect(page.locator('.wizard-steps [aria-current="step"]')).toHaveText(/确认/);
  await page.getByRole("button", { name: "上一步", exact: true }).click();
  await expect(page.locator('.wizard-steps [aria-current="step"]')).toHaveText(/目标与平台/);
  await expect(page.getByRole("checkbox", { name: "Linux · x64", exact: true })).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await expect(page.getByRole("heading", { name: "确认后即可发起构建", exact: true })).toBeVisible();
});

test("detail tabs and preferences preserve the live subscription and the log position", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.route("**/api/v1/builds/build-main/events", route => {
    events.streams++;
    return route.fulfill({ contentType: "text/event-stream",
      body: "retry: 600000\nevent: snapshot\ndata: " + JSON.stringify(build) + "\n\nevent: log\ndata: " +
        JSON.stringify({ id: 1, attempt_id: "attempt-main", step: "compile", text: "native compilation complete\n".repeat(300) }) + "\n\n" });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.getByRole("log");
  await expect(log).toContainText("native compilation complete");
  await page.getByRole("button", { name: "暂停跟随日志", exact: true }).click();
  await log.evaluate(node => { node.scrollTop = 120; });
  const before = events.streams;
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await expect(log).toBeHidden();
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  await expect(page.getByRole("link", { name: /DesktopClient.zip/ })).toBeVisible();
  await page.getByRole("tab", { name: "执行", exact: true }).click();
  await expect(log).toBeVisible();
  expect(await log.evaluate(node => node.scrollTop)).toBe(120);
  await openPreferences(page);
  await page.getByRole("combobox", { name: "主题", exact: true }).selectOption("light");
  await closePreferences(page);
  expect(events.streams).toBe(before);
  expect(await log.evaluate(node => node.scrollTop)).toBe(120);
});

test("returning to execution follows background log updates while paused readers keep their position", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.addInitScript(snapshot => {
    const original = window.fetch.bind(window);
    window.fetch = async (...args) => {
      if (typeof args[0] === "string" && args[0].endsWith("/builds/build-main/events")) {
        const encoder = new TextEncoder();
        return new Response(new ReadableStream({
          start(controller) {
            const send = (kind: string, data: unknown) => controller.enqueue(
              encoder.encode(`event: ${kind}\ndata: ${JSON.stringify(data)}\n\n`),
            );
            send("snapshot", snapshot);
            send("log", { id: 1, attempt_id: "attempt-main", step: "compile", text: "initial output\n".repeat(100) });
            window.addEventListener("message", event => {
              if (event.source === window && event.data?.type === "test-build-log") {
                send("log", { id: event.data.id, attempt_id: "attempt-main", step: "compile", text: event.data.text });
              }
            });
          },
        }), { headers: { "Content-Type": "text/event-stream" } });
      }
      return original(...args);
    };
  }, build);
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.locator('[role="log"]');
  const distanceFromEnd = () => log.evaluate(node => node.scrollHeight - node.clientHeight - node.scrollTop);
  await expect(log).toContainText("initial output");
  await expect.poll(distanceFromEnd).toBeLessThanOrEqual(1);
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await page.evaluate(() => window.postMessage({ type: "test-build-log", id: 2, text: "final background output\n".repeat(200) }, location.origin));
  await expect(log).toContainText("final background output");
  await page.getByRole("tab", { name: "执行", exact: true }).click();
  await expect.poll(distanceFromEnd).toBeLessThanOrEqual(1);

  await page.getByRole("button", { name: "暂停跟随日志", exact: true }).click();
  await log.evaluate(node => { node.scrollTop = 120; });
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  await page.evaluate(() => window.postMessage({ type: "test-build-log", id: 3, text: "more background output\n".repeat(200) }, location.origin));
  await expect(log).toContainText("more background output");
  await page.getByRole("tab", { name: "执行", exact: true }).click();
  expect(await log.evaluate(node => node.scrollTop)).toBe(120);
  expect(events.writes).toEqual([]);
});

test("status and text filters compose, and the empty result does not hide the toolbar", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/builds"));
  await page.getByRole("combobox", { name: "按状态筛选", exact: true }).selectOption("succeeded");
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await expect(page.locator("tbody")).toContainText("#127");
  await page.getByRole("textbox", { name: "筛选当前列表", exact: true }).fill("no-result");
  await expect(page.locator("tbody tr")).toHaveCount(0);
  await expect(page.locator(".list-toolbar")).toBeVisible();
  await page.getByRole("textbox", { name: "筛选当前列表", exact: true }).clear();
  await expect(page.locator("tbody tr")).toHaveCount(1);
});

test("viewer quick navigation exposes no administrative or create-build actions", async ({ page }) => {
  await fixtureAPI(page, "viewer");
  await page.goto(tenantURL("/projects/desktop-client"));
  await page.getByRole("button", { name: "快捷导航", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("option").filter({ hasText: "发起构建" })).toHaveCount(0);
  await expect(page.getByRole("option").filter({ hasText: "租户管理" })).toHaveCount(0);
  await expect(page.getByRole("option").filter({ hasText: project.name }).first()).toBeVisible();
  await page.keyboard.press("Escape");
  await page.goto(tenantURL("/settings/credentials"));
  await expect(page.locator(".management-content")).toContainText("此页面需要管理员权限");
  await expect(page.locator(".management-content button")).toHaveCount(0);
});
