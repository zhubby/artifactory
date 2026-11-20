/**
 * [INPUT]: 依赖业务夹具、可控事件流和浏览器时钟。
 * [OUTPUT]: 验证终态静态读取、历史分页、尾部日志与发布状态轮询。
 * [POS]: 构建详情的数据生命周期回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { test, expect } from "@playwright/test";
import { build, fixtureAPI, tenantURL } from "./fixtures";

const complete = {
  ...build,
  status: "succeeded",
  publish_status: "succeeded",
  goal: "Build the application",
  finished_at: "2026-11-10T08:32:30Z",
  jobs: build.jobs?.map(job => ({ ...job, status: "succeeded" })),
};
const latest = { id: 201, attempt_id: "attempt-main", step: "compile", text: "final history output\n".repeat(300) };

test("completed records load once without SSE or background polling", async ({ page }) => {
  await fixtureAPI(page);
  await page.clock.install();
  const requests: string[] = [];
  page.on("request", request => {
    const path = new URL(request.url()).pathname;
    if (path.endsWith("/builds/128") || /\/builds\/build-main\/(events|logs|plans)$/.test(path)) requests.push(path);
  });
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: complete }));
  await page.route("**/api/v1/builds/build-main/events", route => route.fulfill({ contentType: "text/event-stream", body: `retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(complete)}\n\n` }));
  await page.route("**/api/v1/builds/build-main/logs?*", route => route.fulfill({ json: { logs: [latest], next_before: null } }));
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.getByRole("log");
  await expect(log).toContainText("final history output");
  await expect(page.getByText("历史日志", { exact: true })).toBeVisible();
  await log.evaluate(node => { node.scrollTop = 120; });
  const initial = [...requests];
  await page.clock.fastForward(65000);
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await page.getByRole("tab", { name: "执行", exact: true }).click();
  expect(requests).toEqual(initial);
  expect(requests.some(path => path.endsWith("/events"))).toBe(false);
  expect(await log.evaluate(node => node.scrollTop)).toBe(120);
});

test("loading older history preserves the reader anchor and filters on the server", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: complete }));
  const queries: string[] = [];
  await page.route("**/api/v1/builds/build-main/logs?*", route => {
    const query = new URL(route.request().url()).searchParams;
    queries.push(query.toString());
    if (query.get("step") === "verify") return route.fulfill({ json: { logs: [{ ...latest, step: "verify", text: "verification history\n" }], next_before: null } });
    if (query.has("before")) return route.fulfill({ json: { logs: [{ ...latest, id: 101, text: "older output\n".repeat(100) }], next_before: null } });
    return route.fulfill({ json: { logs: [latest], next_before: 201 } });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.getByRole("log");
  await expect(log).toContainText("final history output");
  const older = page.getByRole("button", { name: "加载更早日志", exact: true });
  await older.scrollIntoViewIfNeeded();
  await log.evaluate(node => { node.scrollTop = 120; });
  const anchor = log.locator("[data-log-id='201']");
  const top = await anchor.evaluate(node => node.getBoundingClientRect().top);
  await older.click();
  await expect(log).toContainText("older output");
  expect(await anchor.evaluate(node => node.getBoundingClientRect().top)).toBeCloseTo(top, 0);
  await expect(page.getByRole("button", { name: "加载更早日志", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: /verify/ }).click();
  await expect(log).toContainText("verification history");
  await expect(log).not.toContainText("final history output");
  expect(queries).toContain("attempt_id=attempt-main&before=201");
  expect(queries).toContain("attempt_id=attempt-main&step=verify");
});

test("a terminal snapshot drains trailing events before ending the live subscription", async ({ page }) => {
  await fixtureAPI(page);
  await page.clock.install();
  const active = { ...build, goal: complete.goal };
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: active }));
  let plans = 0;
  await page.route("**/api/v1/builds/build-main/plans", route => {
    plans++;
    return route.fulfill({ json: { plans: [], runs: [], events: [] } });
  });
  await page.addInitScript(snapshot => {
    const original = window.fetch.bind(window);
    window.fetch = async (...args) => {
      if (typeof args[0] === "string" && args[0].endsWith("/builds/build-main/events")) {
        window.sessionStorage.setItem("stream-count", String(Number(window.sessionStorage.getItem("stream-count") || 0) + 1));
        const encoder = new TextEncoder();
        return new Response(new ReadableStream({ start(controller) {
          const send = (kind: string, data: unknown) => controller.enqueue(encoder.encode(`event: ${kind}\ndata: ${JSON.stringify(data)}\n\n`));
          send("snapshot", snapshot);
          send("log", { id: 1, attempt_id: "attempt-main", step: "compile", text: "live output\n" });
          window.addEventListener("message", event => {
            if (event.source !== window || event.data?.type !== "history-test-event") return;
            send(event.data.kind, event.data.data);
            if (event.data.kind === "complete") controller.close();
          });
        } }), { headers: { "Content-Type": "text/event-stream" } });
      }
      return original(...args);
    };
  }, active);
  await page.route("**/api/v1/builds/build-main/logs?*", route => route.fulfill({ json: { logs: [{ ...latest, text: "trailing output\n" }], next_before: null } }));
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await expect(page.getByRole("log")).toContainText("live output");
  await expect.poll(() => plans).toBe(1);
  await page.evaluate(data => window.postMessage({ type: "history-test-event", kind: "snapshot", data }, location.origin), complete);
  await expect.poll(() => plans).toBe(2);
  await page.evaluate(() => window.postMessage({ type: "history-test-event", kind: "log", data: { id: 2, attempt_id: "attempt-main", step: "compile", text: "trailing output\n" } }, location.origin));
  await expect(page.getByRole("log")).toContainText("trailing output");
  await page.evaluate(data => window.postMessage({ type: "history-test-event", kind: "complete", data }, location.origin), complete);
  await expect(page.getByText("历史日志", { exact: true })).toBeVisible();
  await page.clock.fastForward(65000);
  expect(await page.evaluate(() => sessionStorage.getItem("stream-count"))).toBe("1");
  expect(plans).toBe(2);
  await expect(page.getByRole("log")).toContainText("trailing output");
});

test("retrying a failed target starts a new subscription", async ({ page }) => {
  await fixtureAPI(page);
  const failed = { ...complete, status: "failed", publish_status: "not_requested", jobs: complete.jobs?.map(job => ({ ...job, status: "failed" })) };
  let retried = false;
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => route.fulfill({ json: retried ? build : failed }));
  await page.route("**/api/v1/builds/build-main/logs?*", route => route.fulfill({ json: { logs: [latest], next_before: null } }));
  await page.route("**/api/v1/jobs/job-macos/retry", route => {
    retried = true;
    return route.fulfill({ json: { ok: true } });
  });
  let streams = 0;
  await page.route("**/api/v1/builds/build-main/events", route => {
    streams++;
    return route.fulfill({ contentType: "text/event-stream", body: `retry: 600000\nevent: snapshot\ndata: ${JSON.stringify(build)}\n\nevent: log\ndata: ${JSON.stringify({ ...latest, text: "retry live output\n" })}\n\n` });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await expect(page.getByRole("log")).toContainText("final history output");
  expect(streams).toBe(0);
  await page.getByRole("button", { name: "以新 attempt 重试此目标", exact: true }).click();
  await expect(page.getByRole("log")).toContainText("retry live output");
  expect(streams).toBe(1);
});

test("publication polls only while pending and leaves historical logs untouched", async ({ page }) => {
  await fixtureAPI(page);
  await page.clock.install();
  let details = 0;
  let logs = 0;
  const statuses: string[] = [];
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => {
    details++;
    const publishStatus = details === 1 ? "pending" : "succeeded";
    statuses.push(publishStatus);
    return route.fulfill({ json: { ...complete, publish_status: publishStatus } });
  });
  await page.route("**/api/v1/builds/build-main/logs?*", route => {
    logs++;
    return route.fulfill({ json: { logs: [latest], next_before: null } });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  const log = page.getByRole("log");
  await expect(log).toContainText("final history output");
  await log.evaluate(node => { node.scrollTop = 120; });
  await page.clock.fastForward(3000);
  await expect.poll(() => details).toBe(2);
  expect(statuses).toEqual(["pending", "succeeded"]);
  await expect(page.locator(".page-actions .status-succeeded")).toContainText("成功");
  await page.clock.fastForward(65000);
  expect(details).toBe(2);
  expect(logs).toBe(1);
  expect(await log.evaluate(node => node.scrollTop)).toBe(120);
});
