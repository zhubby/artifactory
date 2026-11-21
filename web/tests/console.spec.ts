/**
 * [INPUT]: 依赖 Playwright、真实控制面与活跃快照夹具和共享 Select 交互助手。
 * [OUTPUT]: 验证业务链路、通知反馈、节点导航与脱敏焦点证据。
 * [POS]: 需要可用后端的端到端浏览器入口。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "./ui/select-helpers";
import { expect, request, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

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
    page.getByRole("link", { name: "新建 Release", exact: true }),
  ).toHaveCount(0);
  const denied = await page.request.post(
    `/api/v1/releases/${evidence.release_id}/builds`,
    {
      headers: tenantHeaders,
      data: { parameters: {} },
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
    page.getByRole("link", { name: "新建 Release", exact: true }),
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
    ["nodes", "构建节点"],
    [`projects/${evidence.project_slug}/artifacts`, "制品"],
    [`projects/${evidence.project_slug}/releases`, "Release 版本"],
    ["settings", "基本信息"],
  ]) {
    await page.goto(tenantURL(`/${route}`));
    await expect(
      page.getByRole("heading", { name: title, level: 1, exact: true }),
    ).toBeVisible();
    await expect(page.locator('[data-notify][data-kind="error"]')).toHaveCount(0);
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
  await page.getByRole("tab",{name:"交付物",exact:true}).click();
  const delivery=await (await page.request.get(`/api/v1/releases/${evidence.release_id}`,{headers:tenantHeaders})).json();
  for(const artifact of delivery.artifacts) await expect(page.getByText(artifact.delivery_filename,{exact:true})).toBeVisible();
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
  try {
    await expect(trigger).toBeFocused();
  } catch (error) {
    const focus = await trigger.evaluate((element) => {
      const active = document.activeElement;
      return {
        documentHasFocus: document.hasFocus(),
        activeElement: active
          ? { tag: active.tagName, id: active.id, role: active.getAttribute("role") }
          : null,
        triggerIsConnected: element.isConnected,
        triggerIsActiveElement: active === element,
      };
    }).catch(() => ({ diagnosticUnavailable: true }));
    await testInfo.attach("dialog-focus-state", {
      body: JSON.stringify(focus, null, 2),
      contentType: "application/json",
    }).catch(() => {});
    throw error;
  }
  if (testInfo.project.name === "mobile") {
    await page.getByRole("button", { name: "打开导航", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "关闭导航", exact: true }),
    ).toBeVisible();
    await page.getByRole("link", { name: "构建节点", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "构建节点", exact: true }),
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
  await expect(page.getByRole("link", { name: "新建 Release" })).toHaveCount(0);
  const denied = await page.request.post(
    `/api/v1/releases/${evidence.release_id}/builds`,
    { headers: tenantHeaders, data: { parameters: {} } },
  );
  expect(denied.status()).toBe(403);
  await page.goto(tenantURL(`/projects/${evidence.project_slug}/releases/new`));
  await expect(
    page.getByText("当前账号没有创建 Release 的权限。", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "继续", exact: true }),
  ).toHaveCount(0);
  await page.goto(tenantURL("/nodes"));
  await expect(
    page.getByRole("heading", { name: "构建节点", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("button", { name: "接入节点" })).toHaveCount(0);
});

test("SSE reconnect and failed publication never imply overall success", async ({
  page,
}, testInfo) => {
  await login(page);
  // 验收构建已终态；首个活跃快照开启订阅，恢复后读取真实终态事件。
  await page.route(
    `**/api/v1/projects/by-slug/${evidence.project_slug}/builds/${evidence.build_number}`,
    async (route) => {
      const response = await route.fetch();
      expect(response.ok()).toBeTruthy();
      const snapshot = await response.json();
      expect(snapshot.status).toBe("succeeded");
      await route.fulfill({
        response,
        json: { ...snapshot, status: "running", finished_at: null },
      });
    },
    { times: 1 },
  );
  let interruptedStreams = 0;
  await page.route("**/api/v1/builds/*/events", (route) => {
    interruptedStreams++;
    return route.abort("connectionfailed");
  });
  await page.goto(buildURL(evidence.build_number));
  await expect.poll(() => interruptedStreams).toBeGreaterThan(0);
  await expect(
    page.getByText("等待连接 / 重连中", { exact: true }),
  ).toBeVisible();
  await page.unroute("**/api/v1/builds/*/events");
  await expect(page.getByRole("log")).toContainText(
    "native compilation complete",
    { timeout: 25000 },
  );
  await expect(page.getByText("历史日志", { exact: true })).toBeVisible();
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
    const build=await (await page.request.get(`/api/v1/builds/${evidence.collision_build_id}`,{headers:tenantHeaders})).json();
    await page.goto(tenantURL(`/projects/${evidence.project_slug}/releases/${build.release_id}`));
    await expect(page.locator(".page-actions .status-publish_failed")).toBeVisible();
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
  await chooseOption(page.locator('[role="combobox"][data-name="credential_id"]'), source.credential_id);
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
  await page.getByRole("link", { name: "新建 Release", exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/[^/]+\/releases\/new$/);
  await page.locator('input[name="version"]').fill(`0.0.${Date.now()}`);
  await page.locator('textarea[name="notes"]').fill("Native C release notes");
  await page.getByRole("button",{name:"新建 Release",exact:true}).click();
  await expect(page).toHaveURL(/\/releases\/[0-9a-f-]{36}$/);
  const referenceResponse = page.waitForResponse(response => new URL(response.url()).pathname.endsWith("/refs"));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  expect((await referenceResponse).status()).toBe(200);
  await expect(page.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await page.locator('textarea[name="goal"]').fill("Build the native C application and collect dist/app");
  await page.getByRole("dialog").getByRole("button",{name:"发起构建",exact:true}).click();
  // This regression server seeds pre-migration builds and has no model key.
  // Real-provider execution is covered by scripts/ai_acceptance.py.
  await expect(page).toHaveURL(/\/builds\/[^/]+$/);
  await expect(page.getByRole("status").filter({ hasText: /model profile/i })).toBeVisible();
  await page.getByRole("tab", { name: "计划", exact: true }).click();
  await expect(
    page.getByText("Build the native C application and collect dist/app", {
      exact: true,
    }),
  ).toBeVisible();
});

test("published Release workbench exposes immutable delivery names and version manifests", async ({page})=>{
  await login(page);
  const response=await page.request.get(`/api/v1/releases/${evidence.release_id}`,{headers:tenantHeaders});
  expect(response.ok()).toBeTruthy();
  const release=await response.json();
  expect(release.status).toBe("published");
  expect(release.selected_build_id).toBeTruthy();
  await page.goto(tenantURL(`/projects/${evidence.project_slug}/releases/${evidence.release_id}`));
  await expect(page.getByRole("button",{name:"编辑说明",exact:true})).toBeVisible();
  await expect(page.getByRole("button",{name:"发起构建",exact:true})).toHaveCount(0);
  await page.getByRole("tab",{name:"交付物",exact:true}).click();
  for(const artifact of release.artifacts) {
    expect(artifact.delivery_filename).toContain(release.version);
    const downloading=page.waitForEvent("download");
    await page.getByRole("link",{name:`下载 ${artifact.delivery_filename}`,exact:true}).click();
    const download=await downloading;
    expect(await download.failure()).toBeNull();
    expect(download.suggestedFilename()).toBe(artifact.delivery_filename);
  }
  for(const name of ["版本 Manifest","SHA256SUMS"]){const downloading=page.waitForEvent("download");await page.getByRole("button",{name,exact:true}).click();expect(await(await downloading).failure()).toBeNull();}
});
