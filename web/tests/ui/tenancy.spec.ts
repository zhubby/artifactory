import { expect, test } from "@playwright/test";
import {
  build,
  fixtureAPI,
  nodes,
  project,
  tenantId,
  tenantSlug,
  tenantURL,
} from "./fixtures";

const secondTenant = "22222222-2222-4222-8222-222222222222";
const secondSlug = "beta-team";
const tenants = [
  {
    id: tenantId,
    slug: tenantSlug,
    name: "Alpha Team",
    role: "owner",
    owner_id: "user-main",
    created_at: new Date().toISOString(),
  },
  {
    id: secondTenant,
    slug: secondSlug,
    name: "Beta Team",
    role: "member",
    owner_id: "other",
    created_at: new Date().toISOString(),
  },
];

test("login always offers tenant choice even for one tenant", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/auth/login", (route) =>
    route.fulfill({ json: { id: "user-main", username: "alex" } }),
  );
  await page.goto("/login");
  await page.locator('input[name="username"]').fill("alex");
  await page.locator('input[name="password"]').fill("test-account-password");
  await page.getByRole("button", { name: "进入工作空间", exact: true }).click();
  await expect(page).toHaveURL(/\/tenants$/);
  await expect(page.getByRole("heading", { name: "选择租户" })).toBeVisible();
  await page.getByRole("link", { name: /Build Team/ }).click();
  await expect(page).toHaveURL(new RegExp(`/tenants/${tenantSlug}$`));
  await expect(
    page.getByRole("heading", { name: "构建，签名，交付。" }),
  ).toBeVisible();
});

test("tenant creation retries with the same idempotency key", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/tenants", async (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: [] });
    const input = route.request().postDataJSON();
    keys.push(input.idempotency_key);
    if (keys.length === 1) return route.abort("failed");
    return route.fulfill({ json: { id: tenantId, slug: tenantSlug } });
  });
  const keys: string[] = [];
  await page.goto("/tenants");
  await page.locator('input[name="name"]').fill("New Team");
  await page.getByRole("button", { name: "创建租户", exact: true }).click();
  await expect(page.locator(".error-box[role=alert]")).toBeVisible();
  await page.getByRole("button", { name: "创建租户", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/tenants/${tenantSlug}$`));
  expect(keys).toHaveLength(2);
  expect(keys[0]).toBe(keys[1]);
});

test("two tabs keep tenant headers independent and switching clears dialogs and late data", async ({
  page,
  context,
}, info) => {
  const other = await context.newPage();
  for (const tab of [page, other]) {
    await fixtureAPI(tab);
    await tab.route("**/api/v1/tenants/*", route => route.fulfill({json:tenants.find(t => t.slug === new URL(route.request().url()).pathname.split("/").at(-1))}));
    await tab.route("**/api/v1/tenants", (route) =>
      route.fulfill({ json: tenants }),
    );
    await tab.route("**/api/v1/tenant", (route) => {
      const id = route.request().headers()["x-artifactory-tenant"];
      return route.fulfill({
        json: {
          tenant: tenants.find((t) => t.id === id),
          project_roles: { [project.id]: "admin" },
        },
      });
    });
    await tab.route("**/api/v1/projects", (route) => {
      const id = route.request().headers()["x-artifactory-tenant"];
      return route.fulfill({
        json: [
          {
            ...project,
            name: id === tenantId ? "Alpha Project" : "Beta Project",
          },
        ],
      });
    });
  }
  await page.goto(tenantURL("/projects"));
  await other.goto(tenantURL("/projects", secondSlug));
  await expect(
    page.getByRole("link", { name: "Alpha Project", exact: true }),
  ).toBeVisible();
  await expect(
    other.getByRole("link", { name: "Beta Project", exact: true }),
  ).toBeVisible();
  await page.goto(tenantURL("/nodes"));
  let finish: (() => void) | undefined;
  const pending = new Promise<void>((resolve) => {
    finish = resolve;
  });
  await page.route("**/api/v1/nodes/*/metadata", async (route) => {
    await pending;
    await route
      .fulfill({
        json: {
          startup_id: "old",
          metadata: { "custom.private": "ALPHA_ONLY" },
        },
      })
      .catch(() => undefined);
  });
  await page
    .getByRole("button", { name: "查看 Mac Studio 的详情", exact: true })
    .click();
  await page.getByRole("tab", { name: "元数据", exact: true }).click();
  await page.evaluate(() => window.history.pushState(null, "", "/tenants"));
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page.getByRole("link", { name: /Beta Team/ }).click();
  finish!();
  await expect(page).toHaveURL(new RegExp(`/tenants/${secondSlug}$`));
  await expect(page.getByText("ALPHA_ONLY")).toHaveCount(0);
  await expect(other).toHaveURL(new RegExp(`/tenants/${secondSlug}/projects$`));
  await expect(
    other.getByRole("link", { name: "Beta Project", exact: true }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth + 1,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("tenant-switch.png"),
    fullPage: true,
  });
  await other.close();
});

test("project members without grants get an authorization prompt", async ({
  page,
}, info) => {
  await fixtureAPI(page, "viewer");
  await page.route(`**/api/v1/tenants/${secondSlug}`, route => route.fulfill({json:tenants[1]}));
  await page.route("**/api/v1/projects", (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route("**/api/v1/tenant", (route) =>
    route.fulfill({ json: { tenant: tenants[1], project_roles: {} } }),
  );
  await page.goto(tenantURL("/projects", secondSlug));
  await expect(page.getByText("暂无项目权限", { exact: true })).toBeVisible();
  await expect(
    page.getByText("请联系租户管理员申请项目授权。", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "接入项目", exact: true }),
  ).toHaveCount(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth + 1,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("tenant-no-project-access.png"),
    fullPage: true,
  });
});

test("invited registration confirms membership without creating an empty tenant", async ({
  page,
}) => {
  await fixtureAPI(page);
  let signedIn = false;
  let createdTenants = 0;
  let accepted = 0;
  await page.route("**/api/v1/auth/me", (route) =>
    route.fulfill(
      signedIn
        ? { json: { id: "new-user", username: "invitee" } }
        : { status: 401, json: { error: "authentication required" } },
    ),
  );
  await page.route("**/api/v1/invitations/test-invite", (route) =>
    route.fulfill({
      json: {
        tenant_id: tenantId,
        tenant_name: "Alpha Team",
        role: "member",
        expires_at: new Date(Date.now() + 86400000).toISOString(),
        available: true,
      },
    }),
  );
  await page.route("**/api/v1/auth/register", (route) => {
    signedIn = true;
    return route.fulfill({
      status: 201,
      json: { id: "new-user", username: "invitee" },
    });
  });
  await page.route("**/api/v1/tenants", (route) => {
    if (route.request().method() === "POST") createdTenants++;
    return route.fulfill({ json: [] });
  });
  await page.route("**/api/v1/invitations/test-invite/accept", (route) => {
    accepted++;
    return route.fulfill({ json: { tenant_id: tenantId, tenant_slug: tenantSlug } });
  });
  await page.goto("/join/test-invite");
  await page.getByRole("link", { name: "创建账号", exact: true }).click();
  await expect(page).toHaveURL(/\/register\?invite=test-invite$/);
  await page.locator('input[name="username"]').fill("invitee");
  await page.locator('input[name="password"]').fill("test-account-password");
  await page
    .locator('input[name="confirmPassword"]')
    .fill("test-account-password");
  await page
    .getByRole("button", { name: "注册并进入工作空间", exact: true })
    .click();
  await expect(page).toHaveURL(/\/join\/test-invite$/);
  await page.getByRole("button", { name: "确认加入", exact: true }).click();
  await expect(page).toHaveURL(new RegExp(`/tenants/${tenantSlug}$`));
  expect(accepted).toBe(1);
  expect(createdTenants).toBe(0);
});

test("tenant member settings support keyboard tabs and ownership confirmation", async ({
  page,
}, info) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/tenant/members", (route) =>
    route.fulfill({
      json: [
        {
          id: "user-main",
          user_id: "user-main",
          username: "alex",
          role: "owner",
          created_at: new Date().toISOString(),
        },
        {
          id: "member",
          user_id: "member",
          username: "Member",
          role: "member",
          created_at: new Date().toISOString(),
        },
      ],
    }),
  );
  await page.goto(tenantURL("/settings"));
  const tab = page.getByRole("tab", { name: "租户成员", exact: true });
  await tab.click();
  await tab.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(
    page.getByRole("tab", { name: "Git 凭据", exact: true }),
  ).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await page.getByRole("button", { name: "转移所有权", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("Member");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "转移所有权", exact: true }),
  ).toBeFocused();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth + 1,
    ),
  ).toBe(true);
  await page.screenshot({
    path: info.outputPath("tenant-members.png"),
    fullPage: true,
  });
});

test("device authorization preserves both modes and saves explicit empty grants", async ({
  page,
}) => {
  await fixtureAPI(page);
  let current = { ...project, node_ids: [] as string[] };
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, route => route.fulfill({json:current}));
  const writes: Record<string, unknown>[] = [];
  await page.route("**/api/v1/projects", (route) =>
    route.fulfill({ json: [current] }),
  );
  await page.route(`**/api/v1/projects/${project.id}`, (route) => {
    if (route.request().method() === "PATCH") {
      const input = route.request().postDataJSON();
      writes.push(input);
      current = {
        ...current,
        ...input,
        node_ids:
          input.node_access.mode === "nodes"
            ? input.node_access.ids
            : current.node_ids,
      };
      return route.fulfill({ json: { id: project.id } });
    }
    return route.fulfill({ json: current });
  });
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  const dialog = page.getByRole("dialog");
  const mode = dialog.getByRole("combobox", {
    name: /^设备授权方式/,
  });
  await mode.selectOption("nodes");
  await dialog
    .getByRole("checkbox", { name: nodes[0].name, exact: true })
    .check();
  await mode.selectOption("pools");
  await expect(
    dialog.getByRole("checkbox", { name: "default", exact: true }),
  ).toBeChecked();
  await mode.selectOption("nodes");
  await expect(
    dialog.getByRole("checkbox", { name: nodes[0].name, exact: true }),
  ).toBeChecked();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(writes[0].node_access).toEqual({ mode: "nodes", ids: [nodes[0].id] });
  expect(writes[0]).not.toHaveProperty("pool_ids");
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  await dialog
    .getByRole("checkbox", { name: nodes[0].name, exact: true })
    .uncheck();
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(writes[1].node_access).toEqual({ mode: "nodes", ids: [] });
});

test("new projects wait for the tenant default pool before initializing grants", async ({
  page,
}) => {
  await fixtureAPI(page);
  let release: (() => void) | undefined;
  const pending = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/pools", async (route) => {
    await pending;
    await route.fulfill({
      json: [{ id: "pool-main", name: "default", is_default: true }],
    });
  });
  await page.goto(tenantURL("/projects"));
  await page.getByRole("button", { name: "接入项目", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator('input[name="name"]')).toHaveCount(0);
  release!();
  await expect(
    dialog.getByRole("checkbox", { name: "default", exact: true }),
  ).toBeChecked();
});

test("project administrators can edit configuration while device and credential grants stay disabled", async ({
  page,
}) => {
  await fixtureAPI(page, "operator");
  await page.route("**/api/v1/tenant", (route) =>
    route.fulfill({
      json: {
        tenant: { ...tenants[0], role: "member" },
        project_roles: { [project.id]: "admin" },
      },
    }),
  );
  await page.route("**/api/v1/projects", (route) =>
    route.fulfill({ json: [project] }),
  );
  let saved: Record<string, unknown> | undefined;
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, route => route.fulfill({json:project}));
  await page.route(`**/api/v1/projects/${project.id}`, (route) => {
    if (route.request().method() === "PATCH")
      saved = route.request().postDataJSON();
    return route.fulfill({ json: project });
  });
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await page.getByRole("button", { name: "项目设置", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("combobox", { name: /^设备授权方式/ }),
  ).toBeDisabled();
  await expect(dialog.locator('select[name="credential_id"]')).toBeDisabled();
  await expect(dialog.locator('input[name="signing_profiles"]')).toBeDisabled();
  await dialog.locator('input[name="name"]').fill("Updated App");
  await dialog.getByRole("button", { name: "保存", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  expect(saved?.name).toBe("Updated App");
  expect(saved).not.toHaveProperty("node_access");
  expect(saved?.credential_id).toBe(project.credential_id);
});

test("direct build links use project permissions when the recent build list is empty", async ({
  page,
}) => {
  await fixtureAPI(page, "operator");
  await page.route("**/api/v1/builds", (route) => route.fulfill({ json: [] }));
  await page.goto(tenantURL(`/projects/${build.project_slug}/builds/${build.number}`));
  await expect(
    page.getByRole("button", { name: "取消构建", exact: true }),
  ).toBeVisible();

  await page.route("**/api/v1/tenant", (route) =>
    route.fulfill({
      json: {
        tenant: { ...tenants[0], role: "member" },
        project_roles: { [project.id]: "viewer" },
      },
    }),
  );
  await page.reload();
  await expect(
    page.getByRole("heading", { name: project.name, exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "取消构建", exact: true }),
  ).toHaveCount(0);
});

test("event reconnects carry tenant and cursor and stop after permission is revoked", async ({
  page,
}) => {
  await fixtureAPI(page);
  const headers: Record<string, string>[] = [];
  await page.route(`**/api/v1/builds/${build.id}/events`, (route) => {
    headers.push(route.request().headers());
    return route.fulfill(
      headers.length === 1
        ? {
            contentType: "text/event-stream",
            body: `retry: 25\nid: 17\nevent: snapshot\ndata: ${JSON.stringify(build)}\n\n`,
          }
        : { status: 403, json: { error: "permission revoked" } },
    );
  });
  await page.goto(tenantURL(`/projects/${build.project_slug}/builds/${build.number}`));
  await expect.poll(() => headers.length).toBe(2);
  expect(headers[0]["x-artifactory-tenant"]).toBe(tenantId);
  expect(headers[1]["last-event-id"]).toBe("17");
  await page.waitForTimeout(150);
  expect(headers).toHaveLength(2);
});
