/**
 * [INPUT]: 依赖 Playwright 与真实控制面测试会话。
 * [OUTPUT]: 验证控制台登录、构建与租户访问。
 * [POS]: 需要可用后端的端到端浏览器入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, request, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { execFileSync } from "node:child_process";

const evidence = JSON.parse(
  readFileSync(
    process.env.ARTIFACTORY_BROWSER_EVIDENCE ||
      resolve(process.cwd(), "../.local/acceptance/latest.json"),
    "utf8",
  ),
) as {
  tenant_id: string;
  tenant_slug: string;
  project_slug: string;
  build_number: number;
  partial_build_number: number;
  collision_build_number: number;
  build_id: string;
  project_id: string;
  release_id: string;
  partial_build_id?: string;
  failed_build_id?: string;
  collision_build_id?: string;
};
const username = process.env.ARTIFACTORY_BOOTSTRAP_USERNAME;
const password = process.env.ARTIFACTORY_BOOTSTRAP_PASSWORD;
if (!username || !password)
  throw new Error(
    "Load .local/env and run scripts/acceptance.py --keep-alive first.",
  );
if (
  !evidence.tenant_id ||
  !evidence.tenant_slug ||
  !evidence.project_slug ||
  !evidence.build_number
)
  throw new Error(
    "Regenerate acceptance evidence with readable tenant, project and build identifiers.",
  );
const tenantHeaders = {
  "X-Artifactory-Tenant": evidence.tenant_id,
  "x-artifactory-csrf": "1",
};
const tenantURL = (path: string) =>
  `/tenants/${evidence.tenant_slug}${path === "/" ? "" : path}`;
const buildURL = (number: number) =>
  tenantURL(`/projects/${evidence.project_slug}/builds/${number}`);

async function chooseTenant(page: Page) {
  await expect(page).toHaveURL(/\/tenants$/);
  await page.locator(`a[href="${tenantURL("/")}"]`).click();
  await expect(page.locator(".project-grid")).toBeVisible();
}

async function grantViewer(page: Page, name: string, user: string) {
  const admin = await request.newContext({
    baseURL: new URL(page.url()).origin,
  });
  try {
    expect(
      (
        await admin.post("/api/v1/auth/login", {
          headers: { "x-artifactory-csrf": "1" },
          data: { username, password },
        })
      ).status(),
    ).toBe(200);
    expect(
      (
        await admin.post("/api/v1/tenant/members", {
          headers: tenantHeaders,
          data: { username: name, role: "member" },
        })
      ).status(),
    ).toBe(200);
    expect(
      (
        await admin.put(
          `/api/v1/projects/${evidence.project_id}/members/${user}`,
          { headers: tenantHeaders, data: { role: "viewer" } },
        )
      ).status(),
    ).toBe(200);
  } finally {
    await admin.dispose();
  }
}

async function login(page: Page, user = username!, secret = password!) {
  await page.goto("/login");
  await page.locator('input[name="username"]').fill(user);
  await page.locator('input[name="password"]').fill(secret);
  await page.getByRole("button", { name: "进入工作空间" }).click();
  await chooseTenant(page);
}

test("self-registration requires explicit tenant and project grants before artifact access", async ({
  page,
  context,
}, testInfo) => {
  const registeredUser = `registered-${Date.now()}-${Math.random().toString(16).slice(2, 6)}`;
  const registeredPassword = "registration-acceptance-only-2026";
  await page.goto("/login");
  const config = await page.request.get("/api/v1/auth/config");
  expect(config.status()).toBe(200);
  expect(await config.json()).toEqual({ registration_enabled: true });
  const invalidUsername = await page.request.post("/api/v1/auth/register", {
    headers: { "x-artifactory-csrf": "1" },
    data: { username: "invalid\u0000", password: registeredPassword },
  });
  expect(invalidUsername.status()).toBe(400);
  await page.getByRole("link", { name: "创建账号", exact: true }).click();
  await expect(page).toHaveURL(/\/register$/);
  await page.locator('input[name="username"]').fill(registeredUser);
  await page.locator('input[name="password"]').fill(registeredPassword);
  await page.locator('input[name="confirmPassword"]').fill(registeredPassword);
  await page.screenshot({
    path: testInfo.outputPath("registration.png"),
    fullPage: true,
  });
  let registrationIdentity: { id: string } | undefined;
  await page.route("**/api/v1/auth/register", async route => {
    const upstream = await route.fetch();
    registrationIdentity = await upstream.json();
    await route.fulfill({ response: upstream });
  });
  const registered = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname === "/api/v1/auth/register" &&
      response.request().method() === "POST",
  );
  await page
    .getByRole("button", { name: "注册并进入工作空间", exact: true })
    .click();
  const response = await registered;
  expect(response.status()).toBe(201);
  expect(response.request().postDataJSON()).toEqual({
    username: registeredUser,
    password: registeredPassword,
  });
  expect(registrationIdentity).toBeDefined();
  const registeredIdentity = registrationIdentity!;
  await expect(page).toHaveURL(/\/tenants$/);
  const session = (await context.cookies()).find(
    (cookie) => cookie.name === "artifactory_session",
  );
  expect(session).toMatchObject({
    httpOnly: true,
    sameSite: "Strict",
    path: "/",
  });
  expect(session!.expires * 1000).toBeGreaterThan(
    Date.now() + 23 * 60 * 60 * 1000,
  );
  const identity = await page.request.get("/api/v1/auth/me");
  expect(identity.status()).toBe(200);
  expect(await identity.json()).toEqual({
    id: registeredIdentity.id,
    username: registeredUser,
    display_name: registeredUser,
  });
  expect(await (await page.request.get("/api/v1/tenants")).json()).toEqual([]);
  expect((await page.request.get("/api/v1/projects")).status()).toBe(400);
  expect(
    (
      await page.request.get("/api/v1/projects", { headers: tenantHeaders })
    ).status(),
  ).toBe(403);
  await grantViewer(page, registeredUser, registeredIdentity.id);
  await page.reload();
  await chooseTenant(page);
  const projects = await page.request.get("/api/v1/projects", {
    headers: tenantHeaders,
  });
  expect(projects.status()).toBe(200);
  expect(
    (await projects.json()).some(
      (project: { id: string }) => project.id === evidence.project_id,
    ),
  ).toBe(true);
  await expect(
    page.getByRole("link", { name: "发起构建", exact: true }),
  ).toHaveCount(0);
  const denied = await page.request.post(
    `/api/v1/projects/${evidence.project_id}/builds`,
    {
      headers: tenantHeaders,
      data: { ref: "main" },
    },
  );
  expect(denied.status()).toBe(403);
  for (const path of ["users", "credentials"]) {
    expect(
      (
        await page.request.get(`/api/v1/${path}`, { headers: tenantHeaders })
      ).status(),
    ).toBe(403);
  }
  await page.goto(buildURL(evidence.build_number));
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  const downloading = page.waitForEvent("download");
  await page.getByRole("link", { name: "下载 dist/app", exact: true }).click();
  expect(await (await downloading).failure()).toBeNull();
  const logout = await page.request.post("/api/v1/auth/logout", {
    headers: { "x-artifactory-csrf": "1" },
  });
  expect(logout.status()).toBe(200);
  expect((await page.request.get("/api/v1/auth/me")).status()).toBe(401);
  await login(page, registeredUser, registeredPassword);
  await expect(
    page.getByRole("link", { name: "发起构建", exact: true }),
  ).toHaveCount(0);
});

test("real console, native logs, artifact download and immutable release", async ({
  page,
}, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await login(page);
  await page.screenshot({
    path: testInfo.outputPath("overview.png"),
    fullPage: true,
  });
  for (const [route, title] of [
    [`projects/${evidence.project_slug}/builds`, "构建记录"],
    ["nodes", "共享设备"],
    [`projects/${evidence.project_slug}/artifacts`, "制品仓库"],
    [`projects/${evidence.project_slug}/releases`, "内部发布"],
    ["settings", "租户管理"],
  ]) {
    await page.goto(tenantURL(`/${route}`));
    await expect(
      page.getByRole("heading", { name: title, exact: true }),
    ).toBeVisible();
    await expect(page.locator(".error-box[role=alert]")).toHaveCount(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth + 1,
      ),
    ).toBeTruthy();
  }
  await page.goto(buildURL(evidence.build_number));
  await expect(page.getByRole("log", { name: "构建日志" })).toContainText(
    "native compilation complete",
  );
  await expect(page.getByRole("log")).toContainText("artifactory-native-ok");
  await page.screenshot({
    path: testInfo.outputPath("native-build.png"),
    fullPage: true,
  });
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  const downloadEvent = page.waitForEvent("download");
  await page.getByRole("link", { name: "下载 dist/app", exact: true }).click();
  const download = await downloadEvent;
  expect(await download.failure()).toBeNull();
  await page.goto(
    tenantURL(
      `/projects/${evidence.project_slug}/releases/${evidence.release_id}`,
    ),
  );
  await expect(page.getByText("dist/app", { exact: true })).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath("release.png"),
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

test("dialogs are keyboard accessible and mobile navigation works", async ({
  page,
}, testInfo) => {
  await login(page);
  await page.goto(tenantURL("/"));
  await expect(page.locator(".project-grid")).toBeVisible();
  const trigger = page.getByRole("button", { name: "接入项目" });
  await trigger.click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("button", { name: "关闭对话框" })).toBeFocused();
  await expect(page.locator('input[name="name"]')).toBeVisible();
  await page.keyboard.press("Tab");
  await expect(page.locator('input[name="name"]')).toBeFocused();
  await page.screenshot({
    path: testInfo.outputPath("project-dialog.png"),
    fullPage: true,
  });
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(trigger).toBeFocused();
  if (testInfo.project.name === "mobile") {
    await page.getByRole("button", { name: "打开导航", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "关闭导航", exact: true }),
    ).toBeVisible();
    await page.getByRole("link", { name: "共享设备", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "共享设备", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "关闭导航", exact: true }),
    ).not.toBeVisible();
  }
});

test("readonly identity cannot trigger builds or modify nodes", async ({
  page,
}) => {
  await login(page);
  const readonlyUser = `browser-${Date.now()}-${Math.random().toString(16).slice(2, 6)}`;
  const readonlyPassword = "browser-acceptance-only-2026";
  const registration = await request.newContext({
    baseURL: new URL(page.url()).origin,
  });
  let reader: { id: string };
  try {
    const created = await registration.post("/api/v1/auth/register", {
      headers: { "x-artifactory-csrf": "1" },
      data: { username: readonlyUser, password: readonlyPassword },
    });
    expect(created.status()).toBe(201);
    reader = (await created.json()) as { id: string };
  } finally {
    await registration.dispose();
  }
  await grantViewer(page, readonlyUser, reader.id);
  await page.locator(".account-trigger").click();
  await page.getByRole("menuitem", { name: "退出登录", exact: true }).click();
  await expect(page).toHaveURL(/\/login$/);
  await login(page, readonlyUser, readonlyPassword);
  await expect(page.getByRole("link", { name: "发起构建" })).toHaveCount(0);
  const denied = await page.request.post(
    `/api/v1/projects/${evidence.project_id}/builds`,
    { headers: tenantHeaders, data: { ref: "main" } },
  );
  expect(denied.status()).toBe(403);
  await page.goto(tenantURL(`/projects/${evidence.project_slug}/builds/new`));
  await expect(
    page.getByText("当前账号没有发起构建的权限。", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "继续", exact: true }),
  ).toHaveCount(0);
  await page.goto(tenantURL("/nodes"));
  await expect(
    page.getByRole("heading", { name: "共享设备", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "接入节点" })).toHaveCount(0);
});

test("SSE reconnect and failed publication never imply overall success", async ({
  page,
}, testInfo) => {
  await login(page);
  await page.route("**/api/v1/builds/*/events", (route) =>
    route.abort("connectionfailed"),
  );
  await page.goto(buildURL(evidence.build_number));
  await expect(
    page.getByText("等待连接 / 重连中", { exact: true }),
  ).toBeVisible();
  await page.unroute("**/api/v1/builds/*/events");
  await expect(page.getByRole("log")).toContainText(
    "native compilation complete",
    { timeout: 25000 },
  );
  if (evidence.partial_build_id) {
    await page.goto(buildURL(evidence.partial_build_number));
    await expect(
      page.getByRole("heading", { name: "目标矩阵", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".target-button .status-succeeded")).toHaveCount(
      1,
    );
    await expect(page.locator(".target-button .status-failed")).toHaveCount(1);
    await page.screenshot({
      path: testInfo.outputPath("partial-failure.png"),
      fullPage: true,
    });
  }
  if (evidence.collision_build_id) {
    await page.goto(buildURL(evidence.collision_build_number));
    await expect(page.locator(".error-box[role=alert]")).toContainText(
      "发布失败（构建结果保留）",
    );
  }
});

test("real repository references and the build wizard preserve model configuration errors", async ({
  page,
}, testInfo) => {
  await login(page);
  const response = await page.request.get(
    `/api/v1/projects/${evidence.project_id}`,
    { headers: tenantHeaders },
  );
  const source = (await response.json()) as {
    repository_url: string;
    credential_id: string;
    signing_profiles: string[];
  };
  const projectName = `Browser native ${testInfo.project.name} ${Date.now()}`;
  await page.goto(tenantURL("/"));
  await page.getByRole("button", { name: "接入项目" }).click();
  await page.locator('input[name="name"]').fill(projectName);
  await page
    .locator('input[name="repository_url"]')
    .fill(source.repository_url);
  await page
    .locator('select[name="credential_id"]')
    .selectOption(source.credential_id);
  await page
    .locator('input[name="signing_profiles"]')
    .fill(source.signing_profiles.join(","));
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "保存", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page
    .locator(".project-card-title")
    .filter({ hasText: projectName })
    .click();
  const referenceResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith("/refs"),
  );
  await page.getByRole("link", { name: "发起构建", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/[^/]+\/builds\/new$/);
  const references = await referenceResponse;
  expect(references.status()).toBe(200);
  expect(await references.json()).toMatchObject({
    default_ref: "refs/heads/main",
    branches: [{ name: "main", ref: "refs/heads/main" }],
    tags: [],
  });
  await expect(
    page.getByRole("radio", { name: "分支 main", exact: true }),
  ).toBeChecked();
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page
    .locator('textarea[name="goal"]')
    .fill("Build the native C application and collect dist/app");
  await page.getByRole("button", { name: "继续", exact: true }).click();
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  // This regression server seeds pre-migration builds and has no model key.
  // Real-provider execution is covered by scripts/ai_acceptance.py.
  await expect(page).toHaveURL(/\/builds\/[^/]+$/);
  await expect(page.locator(".error-box[role=alert]")).toContainText(
    /model profile/i,
  );
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await expect(
    page.getByText("Build the native C application and collect dist/app", {
      exact: true,
    }),
  ).toBeVisible();
});

test("native fixture builds can be canceled and published through the interface", async ({ page }, info) => {
  test.skip(!process.env.DATABASE_URL, "requires the isolated historical-build fixture database");
  test.setTimeout(120000);
  await login(page);
  const seed = (mode: string) => JSON.parse(execFileSync("python3", [resolve("../scripts/legacy_fixture.py")], {
    input: JSON.stringify({ project: evidence.project_id, body: { ref: "main", targets: ["native"], parameters: { MODE: mode } } }),
    encoding: "utf8",
  })) as { id: string };
  const current = async (id: string) => (await (await page.request.get(`/api/v1/builds/${id}`, { headers: tenantHeaders })).json()) as { number: number; status: string; publish_status: string };
  const slow = seed("slow");
  await expect.poll(async () => (await current(slow.id)).status, { timeout: 45000 }).toBe("running");
  await page.goto(buildURL((await current(slow.id)).number));
  await page.getByRole("button", { name: "取消构建", exact: true }).click();
  await expect.poll(async () => (await current(slow.id)).status, { timeout: 30000 }).toBe("canceled");
  await expect(page.locator(".page-actions .status-canceled")).toBeVisible();
  const native = seed("normal");
  await expect.poll(async () => (await current(native.id)).status, { timeout: 45000 }).toBe("succeeded");
  await page.goto(buildURL((await current(native.id)).number));
  await page.getByRole("button", { name: "发布制品", exact: true }).click();
  await page.getByRole("textbox", { name: "版本名称", exact: true }).fill(`ux-${info.project.name}-${Date.now()}`);
  await page.getByRole("button", { name: "生成内部 Release", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect.poll(async () => (await current(native.id)).publish_status).toBe("succeeded");
  await expect(page.locator(".build-meta .status-succeeded")).toBeVisible();
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  const download = page.waitForEvent("download");
  await page.getByRole("link", { name: "下载 dist/app", exact: true }).click();
  expect(await (await download).failure()).toBeNull();
});
