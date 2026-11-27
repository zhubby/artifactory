/**
 * [INPUT]: 依赖真实指针事件、Prism 装饰层与隔离 API 夹具。
 * [OUTPUT]: 验证高光跟随、弹簧中断、按压命中与偏好中断。
 * [POS]: 动态材质回归；显式启用动画并采样中间帧。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, tenantURL } from "./fixtures";

test.beforeEach(async ({ page }) => {
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
});

test("reflection position and chromatic rim track the pointer and clear on exit", async ({ page }, info) => {
  test.skip(info.project.name === "mobile", "Fine pointer enhancement");
  await page.goto(tenantURL("/projects"));
  const shell = page.locator(".workspace-topbar");
  await expect(shell.locator(".prism-sheen")).toHaveCount(1);
  const bounds = (await shell.boundingBox())!;
  await page.mouse.move(bounds.x + 20, bounds.y + 20);
  await expect(shell).toHaveAttribute("data-prism-pointer", "");
  const read = () => shell.evaluate(element => ["--prism-pointer-x", "--prism-pointer-y", "--prism-rim-angle"].map(name => (element as HTMLElement).style.getPropertyValue(name)));
  const first = await read();
  await page.mouse.move(bounds.x + bounds.width - 20, bounds.y + 40, { steps: 12 });
  await expect.poll(read).not.toEqual(first);
  const last = await read();
  expect(parseFloat(last[0]) - parseFloat(first[0])).toBeGreaterThan(300);
  expect(Math.abs(parseFloat(last[2]) - parseFloat(first[2]))).toBeGreaterThan(30);
  await page.mouse.move(700, 700);
  await expect(shell).not.toHaveAttribute("data-prism-pointer", "");
  expect(await read()).toEqual(["", "", ""]);
});

test("selection keeps its element and moves through intermediate positions across routes", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Persistent desktop navigation");
  await page.goto(tenantURL("/projects"));
  const selection = page.locator(".sidebar .prism-selection-capsule");
  await expect(selection).toBeVisible();
  const original = await selection.elementHandle();
  await page.locator(".sidebar-navigation .nav-item").nth(3).click();
  await expect(page).toHaveURL(tenantURL("/pools"));
  expect(await selection.evaluate((element, previous) => element === previous, original)).toBe(true);
  // 路由立即改选中项；背板在后续帧中连续抵达。
  const samples = await selection.evaluate(async element => {
    const frames: number[] = [];
    for (let i = 0; i < 14; i++) {
      await new Promise(requestAnimationFrame);
      frames.push(element.getBoundingClientRect().top);
    }
    return frames;
  });
  expect(Math.max(...samples) - Math.min(...samples)).toBeGreaterThan(2);
  await page.locator(".sidebar-navigation .nav-item").nth(0).click();
  await page.locator(".sidebar-navigation .nav-item").nth(2).click();
  await expect(selection).not.toHaveAttribute("data-prism-moving", "");
  const offset = await selection.evaluate(element => {
    const active = element.parentElement!.querySelector(".nav-item.active")!.getBoundingClientRect();
    const rect = element.getBoundingClientRect();
    return Math.abs(rect.top - active.top) + Math.abs(rect.width - active.width);
  });
  expect(offset).toBeLessThan(1);
});

test("press feedback stays inside the original hit area and clears on release", async ({ page }) => {
  await page.goto(tenantURL("/projects"));
  const button = page.getByRole("button", { name: "接入项目", exact: true });
  const before = (await button.boundingBox())!;
  await page.mouse.move(before.x + before.width / 2, before.y + before.height / 2);
  await page.mouse.down();
  await expect(button).toHaveAttribute("data-prism-pressed", "");
  await expect(button.locator(".prism-press-wave")).toHaveCount(1);
  const during = (await button.boundingBox())!;
  expect(during).toEqual(before);
  // 移出后释放，避免打开业务表单。
  await page.mouse.move(700, 700);
  await page.mouse.up();
  await expect(button).not.toHaveAttribute("data-prism-pressed", "");
  await expect(button.locator(".prism-press-wave")).toHaveCount(0);
  await expect(page.locator(".modal-panel")).toHaveCount(0);
});

test("motion and transparency changes interrupt optical feedback without disabling navigation", async ({ page }, info) => {
  test.skip(info.project.name !== "desktop", "Fine pointer and persistent navigation");
  await page.goto(tenantURL("/projects"));
  const shell = page.locator(".sidebar");
  const overview = shell.getByRole("link", { name: "概览", exact: true });
  const nodes = shell.getByRole("link", { name: "构建节点", exact: true });
  await overview.hover();
  await nodes.hover();
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(shell).not.toHaveAttribute("data-prism-pointer", "");
  await expect(page.locator("[data-prism-moving],[data-prism-active],[data-prism-pressed],.prism-press-wave")).toHaveCount(0);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await overview.hover();
  await page.evaluate(() => { document.documentElement.dataset.transparency = "reduce"; });
  await expect(shell).not.toHaveAttribute("data-prism-pointer", "");
  await expect(page.locator(".prism-hover-capsule.visible")).toHaveCount(0);
  await expect(shell).toHaveCSS("backdrop-filter", "none");
  await nodes.click();
  await expect(page).toHaveURL(tenantURL("/nodes"));
});

test("entry sculpture responds to pointer without a permanent animation loop", async ({ page }, info) => {
  test.skip(info.project.name === "mobile", "Fine pointer enhancement");
  await page.goto("/tenants");
  const pane = page.locator(".ph-pane-front");
  await pane.hover();
  await expect(pane).toHaveAttribute("data-prism-pointer", "");
  await page.mouse.move(20, 20);
  await expect(pane).not.toHaveAttribute("data-prism-active", "");
  expect(await pane.evaluate(element => element.getAnimations({ subtree: true }).filter(animation => animation.playState === "running").length)).toBe(0);
});

test("touch press and cancellation clear the highlight without moving the target", async ({ page }, info) => {
  test.skip(info.project.name !== "mobile", "Touch input");
  await page.goto(tenantURL("/projects"));
  const button = page.getByRole("button", { name: "接入项目", exact: true });
  const before = (await button.boundingBox())!;
  const client = await page.context().newCDPSession(page);
  await client.send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [{ x: before.x + before.width / 2, y: before.y + before.height / 2 }] });
  await expect(button).toHaveAttribute("data-prism-pressed", "");
  expect(await button.boundingBox()).toEqual(before);
  await client.send("Input.dispatchTouchEvent", { type: "touchCancel", touchPoints: [] });
  await expect(button).not.toHaveAttribute("data-prism-pressed", "");
  await expect(button.locator(".prism-press-wave")).toHaveCount(0);
  await expect(page.locator(".modal-panel")).toHaveCount(0);
  await client.detach();
});
