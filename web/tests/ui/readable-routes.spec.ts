import { expect, test } from "@playwright/test";
import { build, fixtureAPI, project, tenantId } from "./fixtures";

const tenantSlug = "build-team";
const projectSlug = "desktop-client";
const buildNumber = 128;
const home = `/tenants/${tenantSlug}`;
const projectURL = `${home}/projects/${projectSlug}`;
const buildURL = `${projectURL}/builds/${buildNumber}`;
const tenant = {
  id: tenantId,
  slug: tenantSlug,
  name: "Build Team",
  role: "owner",
  owner_id: "user-main",
  status: "active",
  archived_at: null,
  created_at: "2026-11-15T00:00:00Z",
};

async function readableAPI(page: Parameters<typeof fixtureAPI>[0]) {
  await fixtureAPI(page);
  await page.route(`**/api/v1/tenants/${tenantSlug}`, (route) =>
    route.fulfill({ json: tenant }),
  );
  await page.route("**/api/v1/projects", (route) =>
    route.fulfill({ json: [{ ...project, slug: projectSlug }] }),
  );
  await page.route(`**/api/v1/projects/by-slug/${projectSlug}`, (route) =>
    route.fulfill({ json: { ...project, slug: projectSlug } }),
  );
  await page.route(
    `**/api/v1/projects/by-slug/${projectSlug}/builds/${buildNumber}`,
    (route) =>
      route.fulfill({
        json: { ...build, number: buildNumber, project_slug: projectSlug },
      }),
  );
  await page.route("**/api/v1/builds", (route) =>
    route.fulfill({
      json: [{ ...build, number: buildNumber, project_slug: projectSlug }],
    }),
  );
}

test("readable deep links retain UUID API authority and browser navigation", async ({
  page,
}, info) => {
  await readableAPI(page);
  const headers: string[] = [];
  page.on("request", (request) => {
    if (new URL(request.url()).pathname.startsWith("/api/v1/projects"))
      headers.push(request.headers()["x-artifactory-tenant"]);
  });
  await page.goto(home);
  await page
    .locator(".project-card-title")
    .filter({ hasText: project.name })
    .click();
  await expect(page).toHaveURL(projectURL);
  await expect(
    page.getByRole("heading", { name: project.name, exact: true }),
  ).toBeVisible();
  await page
    .getByRole("link", { name: `查看构建 ${buildNumber}`, exact: true })
    .click();
  await expect(page).toHaveURL(buildURL);
  await expect(
    page.getByText(`构建 #${buildNumber}`, { exact: true }),
  ).toBeVisible();
  await page.reload();
  await expect(
    page.getByRole("heading", { name: project.name, exact: true }),
  ).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(projectURL);
  await page.goForward();
  await expect(page).toHaveURL(buildURL);
  expect(headers.length).toBeGreaterThan(0);
  expect(headers.every((value) => value === tenantId)).toBe(true);
  await page.screenshot({
    path: info.outputPath("readable-build-route.png"),
    fullPage: true,
  });
});

test("obsolete and malformed page routes return 404 without redirects", async ({
  request,
}) => {
  for (const path of [
    `/t/${tenantId}`,
    "/builds/build-main",
    `${home}/builds/build-main`,
    `${projectURL}/builds/0`,
    `${projectURL}/builds/01`,
    `${projectURL}/builds/-1`,
    `${projectURL}/builds/9007199254740992`,
    `${home}/unknown`,
    "/tenants/UPPERCASE",
  ]) {
    const response = await request.get(path, { maxRedirects: 0 });
    expect(response.status(), path).toBe(404);
  }
});

test("a delayed tenant lookup cannot send requests after switching tenants", async ({
  page,
}) => {
  await readableAPI(page);
  const otherId = "22222222-2222-4222-8222-222222222222";
  let release!: () => void;
  const delayed = new Promise<void>((resolve) => {
    release = resolve;
  });
  let started!: () => void;
  const lookup = new Promise<void>((resolve) => {
    started = resolve;
  });
  await page.route(`**/api/v1/tenants/${tenantSlug}`, async (route) => {
    started();
    await delayed;
    await route.fulfill({ json: tenant }).catch(() => undefined);
  });
  await page.route("**/api/v1/tenants/beta-team", (route) =>
    route.fulfill({
      json: { ...tenant, id: otherId, slug: "beta-team", name: "Beta Team" },
    }),
  );
  await page.route("**/api/v1/tenant", (route) =>
    route.fulfill({
      json: {
        tenant: {
          ...tenant,
          id: otherId,
          slug: "beta-team",
          name: "Beta Team",
        },
        project_roles: { [project.id]: "admin" },
      },
    }),
  );
  const sent: string[] = [];
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (
      path.startsWith("/api/v1/") &&
      !/^\/api\/v1\/(?:auth|tenants|operations)(?:\/|$)/.test(path)
    )
      sent.push(request.headers()["x-artifactory-tenant"]);
  });
  await page.goto(home);
  await lookup;
  await page.evaluate(() =>
    window.history.pushState(null, "", "/tenants/beta-team"),
  );
  await expect(
    page.locator(".project-card-title").filter({ hasText: project.name }),
  ).toBeVisible();
  const finished = page.waitForResponse(`**/api/v1/tenants/${tenantSlug}`);
  release();
  await finished;
  // Give the released fetch time to consume its body and attempt dependent requests.
  await page.waitForTimeout(150);
  await expect(page).toHaveURL(/\/tenants\/beta-team$/);
  expect(sent.length).toBeGreaterThan(0);
  expect(sent.every((id) => id === otherId)).toBe(true);
});
