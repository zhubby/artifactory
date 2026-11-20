/**
 * [INPUT]: 依赖 Playwright、工作区夹具和生命周期 API。
 * [OUTPUT]: 验证隐藏操作历史及当前范围的撤销、重试入口。
 * [POS]: 生命周期设置的浏览器回归；后端排空仍由服务测试验证。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import type { LifecycleOperation } from "../../src/lib/types";
import { fixtureAPI, project, tenantId, tenantURL } from "./fixtures";

function operation(overrides: Partial<LifecycleOperation> = {}): LifecycleOperation {
  return {
    id: "operation-current", tenant_id: tenantId, project_id: project.id,
    slug: project.slug, action: "delete", state: "waiting", phase: "draining",
    blockers: [], counts: {}, error: null,
    created_at: "2026-11-20T01:00:00Z", updated_at: "2026-11-20T01:00:00Z",
    ...overrides,
  };
}

test("entry pages and lifecycle settings hide operation history", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/operations", route => route.fulfill({ json: [operation({
    id: "finished", state: "succeeded", slug: "deleted-history-entry", phase: "complete",
  })] }));
  for (const path of ["/tenants", tenantURL("/"), tenantURL("/projects/desktop-client/settings/lifecycle")]) {
    await page.goto(path);
    await expect(page.getByRole("heading", { name: /选择租户|Build Team|归档与删除/, exact: true }).first()).toBeVisible();
    await expect(page.getByRole("heading", { name: "操作进度", exact: true })).toHaveCount(0);
    await expect(page.getByText("deleted-history-entry", { exact: true })).toHaveCount(0);
  }
});

test("recovery controls use the latest operation in the exact workspace", async ({ page }) => {
  await fixtureAPI(page);
  let current = operation();
  await page.route("**/api/v1/operations", route => route.fulfill({ json: [
    operation({ id: "tenant-operation", project_id: null, state: "failed", created_at: "2026-11-20T03:00:00Z" }),
    operation({ id: "other-project", project_id: "other-project", state: "failed", created_at: "2026-11-20T04:00:00Z" }),
    operation({ id: "older-project-operation", state: "failed", created_at: "2026-11-19T01:00:00Z" }),
    current,
  ] }));
  await page.route("**/api/v1/operations/operation-current/cancel", route => {
    current = operation({ state: "canceled" });
    return route.fulfill({ json: current });
  });
  await page.goto(tenantURL("/projects/desktop-client/settings/lifecycle"));
  await expect(page.getByRole("button", { name: "撤销等待", exact: true })).toHaveCount(1);
  await expect(page.getByRole("button", { name: "重试", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "撤销等待", exact: true }).click();
  await expect(page.getByRole("button", { name: "撤销等待", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "重试", exact: true })).toHaveCount(0);
});

test("failed operation retry keeps controls stable until the request finishes", async ({ page }) => {
  await fixtureAPI(page);
  let current = operation({ state: "failed", error: "cleanup unavailable" });
  await page.route("**/api/v1/operations", route => route.fulfill({ json: [current] }));
  let finish!: () => void;
  const pending = new Promise<void>(resolve => { finish = resolve; });
  await page.route("**/api/v1/operations/operation-current/retry", async route => {
    await pending;
    current = operation({ state: "running" });
    await route.fulfill({ json: current });
  });
  await page.goto(tenantURL("/projects/desktop-client/settings/lifecycle"));
  const retry = page.getByRole("button", { name: "重试", exact: true });
  await expect(retry).toHaveCount(1);
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("cleanup unavailable");
  await retry.click();
  try { await expect(retry).toBeDisabled(); } finally { finish(); }
  await expect(retry).toHaveCount(0);
});

test("accepted lifecycle requests report submission instead of completion", async ({ page }) => {
  await fixtureAPI(page);
  const current = operation({ action: "archive" });
  await page.route("**/api/v1/projects/project-main/lifecycle", route => route.request().method() === "GET"
    ? route.fulfill({ json: { projects: 1, builds: 12, artifacts: 4, devices: 0 } })
    : route.fulfill({ status: 202, json: current }));
  await page.goto(tenantURL("/projects/desktop-client/settings/lifecycle"));
  await page.getByRole("button", { name: "归档", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "归档", exact: true }).click();
  await expect(page.locator("[data-notify]")).toContainText("请求已提交");
  await expect(page.locator("[data-notify]")).not.toContainText("操作成功");
});

test("lifecycle recovery polling stops once the latest operation finishes", async ({ page }) => {
  await fixtureAPI(page);
  let reads = 0;
  let finished = false;
  await page.route("**/api/v1/operations", route => {
    reads += 1;
    return route.fulfill({ json: [operation({ state: finished ? "succeeded" : "waiting" })] });
  });
  await page.goto(tenantURL("/projects/desktop-client/settings/lifecycle"));
  const cancel = page.getByRole("button", { name: "撤销等待", exact: true });
  await expect(cancel).toBeVisible();
  finished = true;
  await expect(cancel).toHaveCount(0);
  const completedReads = reads;
  await page.waitForTimeout(2300);
  expect(reads).toBe(completedReads);
});
