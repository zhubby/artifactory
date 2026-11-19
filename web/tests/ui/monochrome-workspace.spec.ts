/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证快捷导航、业务草稿保护与日志状态保持。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, build, project, tenantURL, openPreferences, closePreferences } from "./fixtures";

test("keyboard quick navigation searches authorized projects and restores focus on Escape", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/"));
  const trigger = page.getByRole("button", { name: "快捷导航", exact: true });
  await trigger.focus();
  await page.keyboard.press("Control+k");
  const search = page.getByRole("combobox", { name: "搜索项目或页面", exact: true });
  await expect(search).toBeFocused();
  await search.fill("Desktop Client");
  await expect(page.getByRole("option").filter({ hasText: "Desktop Client" }).first()).toBeVisible();
  await search.fill("not-an-accessible-project");
  await expect(page.getByRole("option")).toHaveCount(0);
  await search.fill("构建节点");
  await expect(page.getByRole("option")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await trigger.click();
  await search.fill("构建节点");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(tenantURL("/nodes"));
  expect(events.writes).toEqual([]);
});

test("quick navigation respects unsaved build drafts and viewer permissions", async ({ page }) => {
  const events = await fixtureAPI(page);
  await page.goto(tenantURL("/projects/desktop-client/releases/new"));
  await page.locator('textarea[name="goal"]').fill("Preserve the exact draft");
  await page.keyboard.press("Control+k");
  await page.getByRole("combobox", { name: "搜索项目或页面", exact: true }).fill("返回租户工作台");
  const rejected = page.waitForEvent("dialog").then(dialog => dialog.dismiss());
  await page.keyboard.press("Enter");
  await rejected;
  await expect(page).toHaveURL(tenantURL("/projects/desktop-client/releases/new"));
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

test("Release target forms keep host and application platforms consistent", async ({page})=>{
 await fixtureAPI(page);await page.goto(tenantURL("/projects/desktop-client/releases/new"));
 const target=page.locator("fieldset").first();
 await target.getByRole("combobox",{name:"构建宿主系统",exact:true}).selectOption("darwin");
 await expect(target.getByRole("combobox",{name:"应用平台",exact:true})).toHaveValue("");
 await expect(target.getByRole("combobox",{name:"应用平台",exact:true}).getByRole("option")).toHaveText(["非应用目标","macos","ios","android"]);
 await target.getByRole("combobox",{name:"应用平台",exact:true}).selectOption("ios");
 await expect(target.getByRole("combobox",{name:"应用平台",exact:true})).toHaveValue("ios");
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
  await expect(page.getByRole("option").filter({ hasText: "新建 Release" })).toHaveCount(0);
  await expect(page.getByRole("option").filter({ hasText: "租户设置" })).toHaveCount(0);
  await expect(page.getByRole("option").filter({ hasText: project.name }).first()).toBeVisible();
  await page.keyboard.press("Escape");
  await page.goto(tenantURL("/settings/credentials"));
  await expect(page.locator(".management-content")).toContainText("此页面需要管理员权限");
  await expect(page.locator(".management-content button")).toHaveCount(0);
});
