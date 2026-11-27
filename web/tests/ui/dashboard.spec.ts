/**
 * [INPUT]: 依赖完整窗口概览夹具、真实导航、共享下拉和无障碍扫描。
 * [OUTPUT]: 验证双层概览、统计权威、范围切换、请求隔离及响应式图表。
 * [POS]: 概览聚合与项目目录拆分的浏览器验收，不从构建列表计算统计。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { dashboardFixture, fixtureAPI, openNavigation, project, release, tenantId, tenantSlug, tenantURL } from "./fixtures";
import { chooseOption } from "./select-helpers";

test.use({ timezoneId: "Asia/Shanghai" });

function metric(page: Page, label: string) {
  return page.locator(".dashboard-metrics").getByText(label, { exact: true }).locator("..").locator("dd");
}

function numberPattern(value: number) {
  return new RegExp(String(value).split("").join("[,.\\s]?"));
}

test("overview uses complete server aggregates and leaves all projects in their own catalogue", async ({ page }) => {
  await fixtureAPI(page);
  let buildListReads = 0;
  await page.route("**/api/v1/builds?**", route => {
    buildListReads++;
    return route.fulfill({ json: [] });
  });
  await page.goto(tenantURL("/"));
  await expect(page.getByRole("region", { name: "工作空间概览", exact: true })).toBeVisible();
  await expect(metric(page, "构建次数")).toContainText(numberPattern(1248));
  await expect(metric(page, "当前进行中")).toHaveText("14");
  await expect(page.getByRole("heading", { name: "每日构建结果", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "成功构建周转时长", exact: true })).toBeVisible();
  await expect(page.locator(".dashboard-charts .recharts-wrapper")).toHaveCount(2);
  await expect(page.locator(".project-card,.tenant-activity")).toHaveCount(0);
  expect(buildListReads).toBe(0);
  const recent = page.locator(".dashboard-activity");
  await expect(recent.getByRole("link", { name: project.name, exact: true })).toHaveCount(1);
  await recent.getByRole("link", { name: "全部项目", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/projects"));
  await expect(page.getByRole("heading", { name: "项目", level: 1, exact: true })).toBeVisible();
  await expect(page.locator(".dashboard")).toHaveCount(0);
  await expect(page.locator(".project-card-title")).toHaveText(project.name);
  await page.getByRole("textbox", { name: "搜索项目", exact: true }).fill("not-in-this-catalogue");
  await expect(page.locator(".project-card")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "接入项目", exact: true })).toBeVisible();
  expect(buildListReads).toBe(0);
});

test("reporting period sends browser timezone and discards a late earlier range", async ({ page }) => {
  await fixtureAPI(page);
  const queries: URLSearchParams[] = [];
  let started!: () => void;
  let releaseEarlier!: () => void;
  const earlierStarted = new Promise<void>(resolve => { started = resolve; });
  const earlier = new Promise<void>(resolve => { releaseEarlier = resolve; });
  await page.route("**/api/v1/dashboard?**", async route => {
    const query = new URL(route.request().url()).searchParams;
    queries.push(query);
    const days = Number(query.get("days"));
    if (days === 7) { started(); await earlier; }
    const response = dashboardFixture({ days, timezone: query.get("timezone")! });
    response.summary.builds_created = days * 100;
    await route.fulfill({ json: response }).catch(() => undefined);
  });
  try {
    await page.goto(tenantURL("/"));
    await expect(metric(page, "构建次数")).toContainText(numberPattern(3000));
    const period = page.getByRole("combobox", { name: "统计时间范围", exact: true });
    await expect(period).toHaveAttribute("data-value", "30");
    await chooseOption(period, "7");
    await earlierStarted;
    await chooseOption(period, "90");
    await expect(metric(page, "构建次数")).toContainText(numberPattern(9000));
    releaseEarlier();
    await expect(period).toHaveAttribute("data-value", "90");
    await page.waitForTimeout(150);
    await expect(metric(page, "构建次数")).toContainText(numberPattern(9000));
    expect(queries.map(query => query.get("days"))).toEqual(["30", "7", "90"]);
    expect(queries.every(query => query.get("timezone") === "Asia/Shanghai")).toBe(true);
    expect(queries.every(query => !query.has("project_id"))).toBe(true);
  } finally { releaseEarlier(); }
});

test("initial failure retries and background failures retain metrics without foreground progress", async ({ page }) => {
  await page.clock.install();
  await fixtureAPI(page);
  let reads = 0;
  let pollStarted!: () => void;
  let releasePoll!: () => void;
  const started = new Promise<void>(resolve => { pollStarted = resolve; });
  const pending = new Promise<void>(resolve => { releasePoll = resolve; });
  await page.route("**/api/v1/dashboard?**", async route => {
    reads++;
    if (reads === 1) return route.fulfill({ status: 503, json: { error: "Dashboard is temporarily unavailable" } });
    if (reads === 3) {
      pollStarted();
      await pending;
      return route.fulfill({ status: 503, json: { error: "Dashboard polling failed" } });
    }
    return route.fulfill({ json: dashboardFixture() });
  });
  try {
    await page.goto(tenantURL("/"));
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Dashboard is temporarily unavailable");
    await expect(page.locator(".dashboard-metrics dd")).toHaveCount(0);
    await page.locator(".dashboard").getByRole("button", { name: "重试", exact: true }).click();
    await expect(metric(page, "构建次数")).toContainText(numberPattern(1248));
    const original = await page.locator(".dashboard-metrics").elementHandle();
    await page.clock.fastForward(10050);
    await started;
    await page.clock.fastForward(350);
    await expect(page.getByRole("progressbar")).toBeHidden();
    await expect(metric(page, "构建次数")).toContainText(numberPattern(1248));
    releasePoll();
    await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Dashboard polling failed");
    await expect(metric(page, "构建次数")).toContainText(numberPattern(1248));
    expect(await page.locator(".dashboard-metrics").evaluate((node, previous) => node === previous, original)).toBe(true);
    await page.locator(".dashboard").getByRole("button", { name: "重试", exact: true }).click();
    await expect.poll(() => reads).toBe(4);
    await expect(page.locator('[data-notify][data-kind="error"]')).toHaveCount(0);
  } finally { releasePoll(); }
});

test("project overview requests its own aggregate and links recent activity to the build", async ({ page }) => {
  await fixtureAPI(page);
  const projects: (string | null)[] = [];
  await page.route("**/api/v1/dashboard?**", route => {
    projects.push(new URL(route.request().url()).searchParams.get("project_id"));
    return route.fulfill({ json: dashboardFixture({ projectId: project.id }) });
  });
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await expect(page.getByRole("region", { name: "项目构建概览", exact: true })).toBeVisible();
  await expect(metric(page, "构建次数")).toContainText(numberPattern(1248));
  await expect(metric(page, "当前进行中")).toHaveText("14");
  await expect(page.getByRole("heading", { name: "近期构建活动", exact: true })).toBeVisible();
  await expect(page.locator(".main table,.project-focus,.project-shortcuts")).toHaveCount(0);
  expect(projects).toEqual([project.id]);
  const url = tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/128`);
  await page.locator(`.dashboard-activity a[href="${url}"]`).first().click();
  await expect(page).toHaveURL(url);
  await expect(page.getByRole("heading", { name: "构建 #128", exact: true })).toBeVisible();
});

test("switching projects prevents the previous aggregate from replacing the new project", async ({ page }) => {
  await fixtureAPI(page);
  const other = { ...project, id: "project-other", name: "Server API", slug: "server-api" };
  await page.route("**/api/v1/projects", route => route.fulfill({ json: [project, other] }));
  await page.route("**/api/v1/projects/by-slug/server-api", route => route.fulfill({ json: other }));
  let started!: () => void;
  let releasePrevious!: () => void;
  const previousStarted = new Promise<void>(resolve => { started = resolve; });
  const previous = new Promise<void>(resolve => { releasePrevious = resolve; });
  await page.route("**/api/v1/dashboard?**", async route => {
    const id = new URL(route.request().url()).searchParams.get("project_id");
    const response = dashboardFixture();
    response.summary.builds_created = id === project.id ? 1111 : 2222;
    if (id === project.id) { started(); await previous; }
    await route.fulfill({ json: response }).catch(() => undefined);
  });
  try {
    await page.goto(tenantURL(`/projects/${project.slug}`));
    await previousStarted;
    await openNavigation(page);
    await page.locator(".space-selector.project summary").click();
    await page.getByRole("textbox", { name: "搜索项目", exact: true }).fill("Server");
    const otherOption = page.locator(".space-selector.project .space-option").filter({ hasText: other.name });
    await expect(otherOption).toHaveCount(1);
    await otherOption.click();
    await expect(page).toHaveURL(tenantURL(`/projects/${other.slug}`));
    await expect(metric(page, "构建次数")).toContainText(numberPattern(2222));
    releasePrevious();
    await page.waitForTimeout(150);
    await expect(metric(page, "构建次数")).toContainText(numberPattern(2222));
  } finally { releasePrevious(); }
});

test("switching tenants isolates a late dashboard response and sends the new tenant header", async ({ page }) => {
  await fixtureAPI(page);
  const current = { id: tenantId, slug: tenantSlug, name: "Build Team", role: "owner", owner_id: "user-main", status: "active", archived_at: null, created_at: "2026-11-10T00:00:00Z" };
  const other = { ...current, id: "22222222-2222-4222-8222-222222222222", slug: "beta-team", name: "Beta Team" };
  await page.route("**/api/v1/tenants", route => route.fulfill({ json: [current, other] }));
  await page.route("**/api/v1/tenants/beta-team", route => route.fulfill({ json: other }));
  await page.route("**/api/v1/tenant", route => route.fulfill({ json: {
    tenant: route.request().headers()["x-artifactory-tenant"] === other.id ? other : current,
    project_roles: { [project.id]: "admin" },
  } }));
  let started!: () => void;
  let releasePrevious!: () => void;
  const previousStarted = new Promise<void>(resolve => { started = resolve; });
  const previous = new Promise<void>(resolve => { releasePrevious = resolve; });
  const headers: string[] = [];
  await page.route("**/api/v1/dashboard?**", async route => {
    const id = route.request().headers()["x-artifactory-tenant"];
    headers.push(id);
    const response = dashboardFixture();
    response.summary.builds_created = id === tenantId ? 1111 : 2222;
    if (id === tenantId) { started(); await previous; }
    await route.fulfill({ json: response }).catch(() => undefined);
  });
  try {
    await page.goto(tenantURL("/"));
    await previousStarted;
    await page.locator(".space-selector.tenant summary").click();
    await page.locator(".space-selector.tenant .space-option").filter({ hasText: "Beta Team" }).click();
    await expect(page).toHaveURL(tenantURL("/projects", other.slug));
    await openNavigation(page);
    await page.locator(".sidebar").getByRole("link", { name: "概览", exact: true }).click();
    await expect(page).toHaveURL(tenantURL("/", other.slug));
    await expect(metric(page, "构建次数")).toContainText(numberPattern(2222));
    releasePrevious();
    await page.waitForTimeout(150);
    await expect(metric(page, "构建次数")).toContainText(numberPattern(2222));
    expect(headers).toEqual([tenantId, other.id]);
  } finally { releasePrevious(); }
});

test("empty periods preserve zero counts and unknown success rate without rendering axes", async ({ page }, info) => {
  const failures: string[] = [];
  page.on("pageerror", error => failures.push(error.message));
  await fixtureAPI(page);
  await page.route("**/api/v1/dashboard?**", route => route.fulfill({ json: dashboardFixture({ projectId: "not-visible" }) }));
  await page.goto(tenantURL("/"));
  await expect(metric(page, "构建次数")).toHaveText("0");
  await expect(metric(page, "成功率")).toHaveText("—");
  await expect(metric(page, "当前进行中")).toHaveText("0");
  await expect(page.locator(".dashboard-activity a").filter({ hasText: project.name })).toHaveCount(0);
  await expect(page.getByText("此时间范围内还没有新建构建", { exact: true })).toBeVisible();
  await expect(page.getByText("此时间范围内还没有成功构建", { exact: true })).toBeVisible();
  await expect(page.locator(".dashboard-charts .recharts-wrapper")).toHaveCount(0);
  await expect(page.locator(".dashboard").getByRole("link", { name: /项目/ }).first()).toHaveAttribute("href", tenantURL("/projects"));
  await page.locator(".dashboard-charts").screenshot({ path: info.outputPath("empty-charts.png") });
  expect(failures).toEqual([]);
});

test("turnaround distinguishes missing days from isolated samples", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/dashboard?**", route => {
    const days = Number(new URL(route.request().url()).searchParams.get("days"));
    const response = dashboardFixture({ days });
    for (const day of response.daily) day.median_turnaround_seconds = null;
    if (days === 90) {
      response.daily[10].median_turnaround_seconds = 0;
      response.daily[80].median_turnaround_seconds = 7200;
    }
    return route.fulfill({ json: response });
  });
  await page.goto(tenantURL("/"));
  const charts = page.locator(".dashboard-charts");
  await expect(charts.getByText("此时间范围内还没有成功构建", { exact: true })).toBeVisible();
  await expect(charts.locator(".recharts-wrapper")).toHaveCount(1);
  await chooseOption(page.getByRole("combobox", { name: "统计时间范围" }), "90");
  await expect(charts.locator(".recharts-wrapper")).toHaveCount(2);
  await expect(charts.locator(".dashboard-chart-sample")).toHaveCount(2);
  const curve = await charts.locator(".recharts-line-curve").getAttribute("d");
  expect(curve?.match(/M/g)).toHaveLength(2);
  expect(curve).not.toContain("L");
});

for (const locale of ["zh-CN", "en"] as const) for (const theme of ["light", "dark"] as const) {
  test(`${locale} ${theme}: dashboards keep charts readable and pass WCAG AA`, async ({ page }, info) => {
    test.setTimeout(90000);
    const failures: string[] = [];
    page.on("pageerror", error => failures.push(error.message));
    await fixtureAPI(page);
    await page.context().addCookies([{ name: "artifactory-locale", value: locale, url: String(info.project.use.baseURL) }]);
    await page.addInitScript(value => localStorage.setItem("artifactory-theme", value), theme);
    if (info.project.name === "mobile" && locale === "zh-CN" && theme === "light") {
      await page.setViewportSize({ width: 320, height: 844 });
    }
    for (const [name, route] of [["tenant", "/"], ["project", `/projects/${project.slug}`]]) {
      await page.goto(tenantURL(route));
      await expect(page.locator(".dashboard-metrics dd")).toHaveCount(4);
      await expect(page.locator(".dashboard-charts .recharts-wrapper")).toHaveCount(2);
      await page.evaluate(() => document.fonts.ready);
      const panels = page.locator(".dashboard-chart-panel");
      const [left, right] = await panels.evaluateAll(nodes => nodes.map(node => node.getBoundingClientRect().toJSON()));
      expect(Math.abs(left.width - right.width)).toBeLessThanOrEqual(1);
      const contentWidth = await page.locator(".dashboard").evaluate(node => node.getBoundingClientRect().width);
      if (contentWidth < 780) {
        expect(right.x).toBe(left.x);
        expect(right.top).toBeGreaterThan(left.bottom);
      } else {
        expect(right.left).toBeGreaterThan(left.right);
        expect(right.top).toBe(left.top);
        expect(right.bottom).toBe(left.bottom);
        const [firstPlot, secondPlot] = await page.locator(".dashboard-chart")
          .evaluateAll(nodes => nodes.map(node => node.getBoundingClientRect().toJSON()));
        expect(secondPlot.top).toBe(firstPlot.top);
        expect(secondPlot.bottom).toBe(firstPlot.bottom);
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true);
      const audit = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
      expect(audit.violations.map(item => ({ id: item.id, targets: item.nodes.map(node => node.target) }))).toEqual([]);
      await page.addStyleTag({ content: "nextjs-portal {display:none}" });
      await page.screenshot({ path: info.outputPath(`${locale}-${theme}-${name}-dashboard.png`), fullPage: true, animations: "disabled" });
    }
    expect(failures).toEqual([]);
  });
}

// 连续相同值形成水平路径，渐变坐标不得依赖零高包围盒。
test("constant turnaround samples retain a visible horizontal trend", async ({ page }) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/dashboard?**", route => {
    const data=dashboardFixture();
    data.daily.forEach(day => { day.median_turnaround_seconds=120; });
    return route.fulfill({json:data});
  });
  await page.goto(tenantURL("/"));
  const curve=page.locator(".dashboard-turnaround-line .recharts-line-curve");
  await expect(curve).toHaveCount(1);
  await expect(curve).toHaveCSS("visibility", "visible");
  await expect(curve).toHaveCSS("stroke-width", "3px");
  const gradientId=(await curve.getAttribute("stroke"))!.slice(5,-1);
  await expect(page.locator(`[id="${gradientId}"]`)).toHaveAttribute("gradientUnits","userSpaceOnUse");
  expect((await curve.getAttribute("d"))?.length).toBeGreaterThan(20);
});
