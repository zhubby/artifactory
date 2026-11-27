/**
 * [INPUT]: 依赖工作空间权限、项目详情、路由切换和可控延迟响应。
 * [OUTPUT]: 验证侧栏首读骨架、失败重试、范围隔离和静默轮询。
 * [POS]: 授权完成前的导航边界；不替代服务端权限验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { fixtureAPI, openNavigation, project, tenantId, tenantSlug, tenantURL } from "./fixtures";

const tenant = { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", status: "active", archived_at: null, owner_id: "user-main", created_at: "2026-11-10T08:30:00Z" };

for (const path of ["/", "/projects/desktop-client", "/settings/credentials", "/projects/desktop-client/settings/resources"]) {
  test(`${path}: sidebar waits for current permissions and exposes no premature business links`, async ({ page }) => {
    await fixtureAPI(page);
    let release!: () => void;
    const pending = new Promise<void>(resolve => { release = resolve; });
    await page.route("**/api/v1/tenant", async route => {
      await pending;
      await route.fulfill({ json: { tenant, project_roles: { [project.id]: "admin" } } });
    });
    try {
      await page.goto(tenantURL(path));
      await openNavigation(page);
      const sidebar = page.locator(".sidebar");
      await expect(sidebar.locator(".loading-state")).toBeVisible();
      await expect(sidebar.locator(".skeleton-block").first()).toBeVisible();
      await expect(sidebar.locator(".nav-item")).toHaveCount(0);
      await expect(sidebar.locator(".space-selector.project")).toHaveCount(0);
      release();
      await expect(sidebar.locator(".nav-item").first()).toBeVisible();
      await expect(sidebar.locator(".loading-state")).toHaveCount(0);
    } finally { release(); }
  });
}

test("failed permissions end the sidebar skeleton and retry restores authorized navigation", async ({ page }) => {
  await fixtureAPI(page);
  let failed = true;
  await page.route("**/api/v1/tenant", route => failed
    ? route.fulfill({ status: 503, json: { error: "权限读取暂不可用" } })
    : route.fulfill({ json: { tenant, project_roles: { [project.id]: "admin" } } }));
  await page.goto(tenantURL("/projects/desktop-client/settings/resources"));
  const errorNotice = page.locator('[data-notify][data-kind="error"]');
  await expect(errorNotice).toContainText("权限读取暂不可用");
  await errorNotice.getByRole("button", { name: "关闭通知", exact: true }).click();
  await expect(errorNotice).toHaveCount(0);
  await openNavigation(page);
  const sidebar = page.locator(".sidebar");
  await expect(sidebar.locator(".loading-state")).toHaveCount(0);
  await expect(sidebar.locator(".nav-item")).toHaveCount(0);
  failed = false;
  await sidebar.locator(".sidebar-load-error").getByRole("button", { name: "重试", exact: true }).click();
  await openNavigation(page);
  await expect(sidebar.getByRole("link", { name: "资源与凭据", exact: true })).toBeVisible();
  await expect(sidebar.locator(".loading-state")).toHaveCount(0);
});

test("switching projects hides the previous administrator navigation until the new scope resolves", async ({ page }) => {
  await fixtureAPI(page);
  const other = { ...project, id: "project-viewer", slug: "viewer-app", name: "Viewer App", effective_role: "viewer" };
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [project, other] }));
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route(`**/api/v1/projects/by-slug/${other.slug}`, async route => {
    await pending;
    await route.fulfill({ json: other });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}`));
    await openNavigation(page);
    await expect(page.locator(".sidebar").getByRole("link", { name: "项目设置", exact: true })).toBeVisible();
    await page.locator(".space-selector.project summary").click();
    await page.locator(".space-selector.project").getByRole("link", { name: /^Viewer App(?:\s|$)/ }).click();
    await expect(page).toHaveURL(tenantURL(`/projects/${other.slug}`));
    await openNavigation(page);
    const sidebar = page.locator(".sidebar");
    await expect(sidebar.locator(".loading-state")).toBeVisible();
    await expect(sidebar.getByRole("link", { name: "项目设置", exact: true })).toHaveCount(0);
    await expect(sidebar.locator(".space-selector.project")).toHaveCount(0);
    release();
    await expect(sidebar.getByRole("link", { name: "项目概览", exact: true })).toBeVisible();
    await expect(sidebar.getByRole("link", { name: "项目设置", exact: true })).toHaveCount(0);
    await expect(sidebar.locator(".space-selector.project summary")).toContainText(other.name);
  } finally { release(); }
});

test("ready navigation survives page changes and delayed permission polling without a skeleton", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let reads = 0;
  let release!: () => void;
  const pending = new Promise<void>(resolve => { release = resolve; });
  await page.route("**/api/v1/tenant", async route => {
    if (++reads > 1) await pending;
    await route.fulfill({ json: { tenant, project_roles: { [project.id]: "admin" } } });
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}`));
    await openNavigation(page);
    const sidebar = page.locator(".sidebar");
    const releases = sidebar.getByRole("link", { name: "Release 版本", exact: true });
    await expect(releases).toBeVisible();
    await releases.evaluate(node => Object.assign(window, { stableNavigationLink: node }));
    await releases.click();
    await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/releases`));
    await page.clock.fastForward(5100);
    await expect.poll(() => reads).toBeGreaterThan(1);
    await openNavigation(page);
    await expect(releases).toBeVisible();
    expect(await releases.evaluate(node => node === (window as unknown as { stableNavigationLink: Element }).stableNavigationLink)).toBe(true);
    await expect(sidebar.locator(".loading-state")).toHaveCount(0);
    await expect(page.getByRole("progressbar", { name: "页面加载", exact: true })).toBeHidden();
  } finally { release(); }
});
