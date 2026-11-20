/**
 * [INPUT]: 依赖可控 API、浏览器时钟和持续构建事件流。
 * [OUTPUT]: 验证加载动画帧、运行时减少动态适配与快速请求防闪。
 * [POS]: 全局加载反馈的动效及长连接回归边界。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Locator } from "@playwright/test";
import { build, fixtureAPI, release as buildRelease, tenantId, tenantSlug, tenantURL } from "./fixtures";

const tenant = {
  id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "active",
  archived_at: null, owner_id: "user-main", created_at: "2026-11-10T08:30:00Z",
};

async function loadingFrames(target: Locator) {
  return target.evaluate(element => {
    const animation = element.getAnimations().find(item => item.effect instanceof KeyframeEffect && item.effect.target === element);
    if (!animation) throw new Error("The loading animation did not start");
    animation.pause();
    const duration = Number(animation.effect!.getTiming().duration);
    const frames = [0, .5, .9999].map(fraction => {
      animation.currentTime = duration * fraction;
      const style = getComputedStyle(element);
      return { opacity: Number(style.opacity), scale: new DOMMatrixReadOnly(style.transform).a };
    });
    animation.play();
    return frames;
  });
}

test("loading animations advance and stop when reduced motion changes at runtime", async ({ page }) => {
  await fixtureAPI(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/tenants", async route => {
    await pending;
    await route.fulfill({ json: [tenant] });
  });
  try {
    await page.goto("/tenants");
    await expect(page.locator("html")).toHaveAttribute("data-motion", "on");
    const skeleton = page.locator(".loading-state .skeleton-block").first();
    const progress = page.getByRole("progressbar");
    const line = progress.locator("span");
    await expect(skeleton).toBeVisible();
    await expect(progress).toBeVisible();
    const skeletonFrames = await loadingFrames(skeleton);
    expect(skeletonFrames[0].opacity).toBeCloseTo(1, 2);
    expect(skeletonFrames[1].opacity).toBeCloseTo(.45, 2);
    expect(skeletonFrames[2].opacity).toBeCloseTo(1, 2);
    const progressFrames = await loadingFrames(line);
    expect(progressFrames[0].scale).toBeCloseTo(.08, 2);
    expect(progressFrames[1].scale).toBeGreaterThan(progressFrames[0].scale);
    expect(progressFrames[1].scale).toBeLessThan(.9);
    expect(progressFrames[2].scale).toBeCloseTo(.9, 2);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await expect(page.locator("html")).toHaveAttribute("data-motion", "off");
    for (const target of [skeleton, line]) {
      await expect.poll(() => target.evaluate(element => element.getAnimations().filter(animation => animation.playState === "running").length)).toBe(0);
    }
    const staticFrame = async () => ({
      opacity: await skeleton.evaluate(element => Number(getComputedStyle(element).opacity)),
      scale: await line.evaluate(element => new DOMMatrixReadOnly(getComputedStyle(element).transform).a),
    });
    const before = await staticFrame();
    await page.waitForTimeout(150);
    expect(await staticFrame()).toEqual(before);
    expect(before.opacity).toBe(1);
    expect(before.scale).toBeCloseTo(.65, 2);
  } finally {
    release();
  }
  await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
  await expect(page.getByRole("progressbar")).toBeHidden();
});

test("foreground requests completed before the display delay never flash the top bar", async ({ page }) => {
  await fixtureAPI(page);
  await page.clock.install();
  let release!: () => void;
  let started!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  const requested = new Promise<void>(resolve => { started = resolve; });
  let retrying = false;
  await page.route("**/api/v1/tenants", async route => {
    if (!retrying) return route.fulfill({ status: 503, json: { error: "Retry the tenant request" } });
    started();
    await pending;
    await route.fulfill({ json: [tenant] });
  });
  await page.goto("/tenants");
  const retry = page.getByRole("button", { name: "重试", exact: true });
  await expect(retry).toBeVisible();
  await expect(page.getByRole("progressbar")).toBeHidden();
  await page.clock.pauseAt(await page.evaluate(() => Date.now() + 1000));
  await page.evaluate(() => {
    const progress = document.querySelector<HTMLElement>('[role="progressbar"]')!;
    document.documentElement.dataset.progressFlashed = "false";
    new MutationObserver(() => {
      if (!progress.hidden && progress.getBoundingClientRect().height > 0)
        document.documentElement.dataset.progressFlashed = "true";
    }).observe(progress, { attributes: true, attributeFilter: ["hidden"] });
  });
  retrying = true;
  try {
    await retry.click({ force: true });
    await requested;
    await page.clock.runFor(100);
    await expect(page.getByRole("progressbar")).toBeHidden();
    release();
    await expect(page.getByRole("link", { name: "Build Team", exact: true })).toBeVisible();
    await page.clock.runFor(400);
    await expect(page.getByRole("progressbar")).toBeHidden();
    await expect(page.locator("html")).toHaveAttribute("data-progress-flashed", "false");
  } finally {
    release();
    await page.clock.resume();
  }
});

test("a persistent build stream stays live after foreground loading finishes", async ({ page }) => {
  await fixtureAPI(page);
  let releaseRead!: () => void;
  const reading = new Promise<void>(resolve => { releaseRead = resolve; });
  await page.route(`**/api/v1/releases/${buildRelease.id}`, async route => {
    await reading;
    await route.fulfill({ json: buildRelease });
  });
  await page.addInitScript(snapshot => {
    const original = window.fetch.bind(window);
    window.fetch = async (...args) => {
      if (typeof args[0] === "string" && args[0].endsWith("/builds/build-main/events")) {
        const encoder = new TextEncoder();
        return new Response(new ReadableStream({ start(controller) {
          const send = (kind: string, data: unknown) => controller.enqueue(encoder.encode(`event: ${kind}\ndata: ${JSON.stringify(data)}\n\n`));
          send("snapshot", snapshot);
          send("log", { id: 1, attempt_id: "attempt-main", step: "compile", text: "persistent stream opened\n" });
          window.addEventListener("message", event => {
            if (event.source === window && event.data?.type === "loading-test-log")
              send("log", { id: 2, attempt_id: "attempt-main", step: "compile", text: "stream remains live\n" });
          });
        } }), { headers: { "Content-Type": "text/event-stream" } });
      }
      return original(...args);
    };
  }, build);
  try {
    await page.goto(tenantURL("/projects/desktop-client/builds/128"));
    const log = page.getByRole("log");
    await expect(log).toContainText("persistent stream opened");
    await expect(page.getByRole("progressbar")).toBeVisible();
    releaseRead();
    await expect(page.getByRole("progressbar")).toBeHidden();
    await page.evaluate(() => window.postMessage({ type: "loading-test-log" }, location.origin));
    await expect(log).toContainText("stream remains live");
    await expect(page.getByRole("progressbar")).toBeHidden();
  } finally {
    releaseRead();
  }
});
