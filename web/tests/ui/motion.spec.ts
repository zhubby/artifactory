/**
 * [INPUT]: 依赖 Playwright、页面路由及测试夹具。
 * [OUTPUT]: 验证默认动效、系统减少动态适配及无动效开关的偏好界面。
 * [POS]: 使用模拟 API 的界面浏览器回归入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Locator } from "@playwright/test";
import { fixtureAPI, openPreferences, closePreferences, tenantURL } from "./fixtures";

async function entranceFrames(panel: Locator) {
  return panel.evaluate(element => {
    const animation = element.getAnimations().find(item => item.effect instanceof KeyframeEffect && item.effect.target === element);
    if (!animation) throw new Error("The panel entrance animation did not start");
    animation.pause();
    const duration = Number(animation.effect!.getTiming().duration);
    const read = () => {
      const box = element.getBoundingClientRect();
      return { x: box.x, y: box.y, width: box.width, height: box.height, opacity: Number(getComputedStyle(element).opacity) };
    };
    const frames = [0, .25, .5, .9999].map(progress => {
      animation.currentTime = duration * progress;
      return read();
    });
    animation.finish();
    return { duration, frames, settled: read(), viewport: { width: innerWidth, height: innerHeight } };
  });
}

for (const kind of ["quick navigation", "personal settings"]) {
  test(`${kind} stays centered throughout its entrance without a final jump`, async ({ page }) => {
    await fixtureAPI(page);
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.goto(tenantURL("/projects/desktop-client"));
    await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
    // Pause only the playback so first-frame geometry remains observable even
    // when automation reaches the dialog after its short entrance would end.
    await page.addStyleTag({ content: ".modal-panel, .account-panel { animation-play-state: paused !important; }" });
    const trigger = kind === "quick navigation"
      ? page.getByRole("button", { name: "快捷导航", exact: true })
      : page.locator(".account-trigger");
    if (kind === "quick navigation") await trigger.click();
    else await openPreferences(page);
    const panel = page.locator(kind === "quick navigation" ? ".modal-panel" : ".account-panel");
    await expect(panel).toBeVisible();
    const { frames, settled, viewport, duration } = await entranceFrames(panel);
    expect(duration).toBeGreaterThanOrEqual(120);
    expect(duration).toBeLessThanOrEqual(180);
    for (const frame of frames) {
      expect(Math.abs(frame.x + frame.width / 2 - viewport.width / 2)).toBeLessThanOrEqual(1);
      expect(Math.abs(frame.y + frame.height / 2 - viewport.height / 2)).toBeLessThanOrEqual(5);
    }
    expect(Math.abs(frames.at(-1)!.x - settled.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(frames.at(-1)!.y - settled.y)).toBeLessThanOrEqual(1);
    await page.keyboard.press("Escape");
    await expect(trigger).toBeFocused();
    await expect(panel).toHaveCount(0);
  });
}

test("device drawers keep their right edge and vertical position during entrance", async ({ page }) => {
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto(tenantURL("/nodes"));
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await page.addStyleTag({ content: ".modal-panel { animation-play-state: paused !important; }" });
  const trigger = page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true });
  await trigger.click();
  const panel = page.locator(".drawer-panel");
  await expect(panel).toBeVisible();
  const { frames, settled, viewport } = await entranceFrames(panel);
  for (const frame of frames) {
    expect(Math.abs(frame.y)).toBeLessThanOrEqual(1);
    expect(frame.x + frame.width - viewport.width).toBeGreaterThanOrEqual(-1);
    expect(frame.x + frame.width - viewport.width).toBeLessThanOrEqual(17);
  }
  expect(Math.abs(frames.at(-1)!.x - settled.x)).toBeLessThanOrEqual(1);
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

test("enabling reduced motion during an entrance leaves the dialog centered and usable", async ({ page }) => {
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto(tenantURL("/projects/desktop-client"));
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  const pause = await page.addStyleTag({ content: ".modal-panel { animation-play-state: paused !important; }" });
  const trigger = page.getByRole("button", { name: "快捷导航", exact: true });
  await trigger.click();
  const panel = page.locator(".modal-panel");
  await expect(panel).toBeVisible();
  await panel.evaluate(element => { element.getAnimations()[0].currentTime = 60; });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await pause.evaluate(element => { element.parentNode?.removeChild(element); });
  await expect(page.locator("html")).toHaveAttribute("data-motion", "off");
  await expect.poll(() => panel.evaluate(element => element.getAnimations().filter(animation => animation.playState === "running").length)).toBe(0);
  const offset = await panel.evaluate(element => {
    const box = element.getBoundingClientRect();
    return { x: box.x + box.width / 2 - innerWidth / 2, y: box.y + box.height / 2 - innerHeight / 2 };
  });
  expect(Math.abs(offset.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(offset.y)).toBeLessThanOrEqual(1);
  await expect(page.getByRole("combobox", { name: "搜索项目或页面", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

test("legacy manual preferences are ignored while system reduced motion preserves drawer layout", async ({ page }) => {
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(() => localStorage.setItem("artifactory-effects", "off"));
  await page.goto(tenantURL("/"));
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await expect(page.locator("canvas,.atmosphere,.spotlight-card")).toHaveCount(0);
  await openPreferences(page);
  await expect(page.getByRole("button", { name: /界面动效|Turn (on|off) animations/ })).toHaveCount(0);
  await expect(page.getByText(/减少动态效果|reduced motion/i)).toHaveCount(0);
  await closePreferences(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(page.locator("html")).toHaveAttribute("data-motion", "off");
  await expect(page.locator(".nav-item").first()).toHaveCSS("transition-duration", "1e-05s");
  await openPreferences(page);
  await expect(page.getByText(/减少动态效果|reduced motion/i)).toHaveCount(0);
  await closePreferences(page);
  await page.goto(tenantURL("/nodes"));
  await page.getByRole("button", { name: "查看 Mac Studio 的详情", exact: true }).click();
  const drawer = page.locator(".drawer-panel");
  const geometry = await drawer.evaluate(element => {
    const box = element.getBoundingClientRect();
    return { right: box.right, viewport: innerWidth, animation: getComputedStyle(element).animationName };
  });
  expect(geometry.animation).toBe("none");
  expect(Math.abs(geometry.right - geometry.viewport)).toBeLessThanOrEqual(1);
  await page.keyboard.press("Escape");
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await expect(page.locator("canvas")).toHaveCount(0);
});

test("the login form works without graphics contexts and retains its draft across preferences", async ({ page }) => {
  const failures: string[] = [];
  page.on("pageerror", error => failures.push(error.message));
  await page.addInitScript(() => {
    HTMLCanvasElement.prototype.getContext = (() => { throw new Error("Graphics are unavailable"); }) as typeof HTMLCanvasElement.prototype.getContext;
  });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/login");
  await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
  await expect(page.locator("canvas,.atmosphere")).toHaveCount(0);
  await page.getByRole("textbox", { name: "用户名", exact: true }).fill("retained-draft");
  await openPreferences(page);
  await expect(page.getByRole("button", { name: /界面动效|Turn (on|off) animations/ })).toHaveCount(0);
  await page.getByRole("combobox", { name: "界面语言", exact: true }).selectOption("en");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(page.locator("html")).toHaveAttribute("data-motion", "off");
  await expect(page.getByRole("button", { name: /界面动效|Turn (on|off) animations/ })).toHaveCount(0);
  await expect(page.getByText(/减少动态效果|reduced motion/i)).toHaveCount(0);
  await closePreferences(page);
  await expect(page.getByRole("textbox", { name: "Username", exact: true })).toHaveValue("retained-draft");
  expect(failures).toEqual([]);
});

test("polling and locale updates preserve the title element without an entrance animation", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await fixtureAPI(page);
  await page.goto(tenantURL("/"));
  const title = page.locator(".page-title");
  await expect(page.getByRole("heading", { name: "Build Team", exact: true })).toBeVisible();
  const original = await title.elementHandle();
  await page.waitForTimeout(5100);
  await openPreferences(page);
  await page.getByRole("combobox", { name: "界面语言", exact: true }).selectOption("en");
  await closePreferences(page);
  expect(await title.evaluate((node, previous) => node === previous, original)).toBe(true);
  await expect(title).toHaveCSS("transform", "none");
  await expect(page.locator("canvas")).toHaveCount(0);
});
