/**
 * [INPUT]: 依赖控制台浏览器夹具和签名公开接口。
 * [OUTPUT]: 验证侧栏签名分区、凭据导入、默认方案和制品证据。
 * [POS]: 托管签名的用户流程验收，后端密码学另行验证。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { artifact, fixtureAPI, project, tenantURL, openNavigation } from "./fixtures";
const metadata = {
  identity: "Android Release",
  subject: "CN=Release",
  fingerprint_sha256: "a".repeat(64),
  not_before: 1,
  not_after: 1900000000,
  alias: "release",
  purpose: "application",
  notarization_available: false,
};
const binding = {
  profile: "managed:release",
  platform: "android",
  credential_id: "credential-signing",
  version_id: "version-frozen",
  metadata,
  notarize: false,
  enabled: true,
  credential_enabled: true,
  available: true,
};

test("credential import submits private material once and clears the form", async ({
  page,
}, info) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/signing/profiles", (route) =>
    route.fulfill({ json: [] }),
  );
  let imported = false;
  await page.route("**/api/v1/signing/credentials", async (route) => {
    if (route.request().method() === "POST") {
      expect(route.request().postDataJSON()).toEqual({
        name: "Release",
        material: {
          kind: "android_keystore",
          file_base64: "dGVzdA==",
          store_password: "private-store-password",
          key_password: "private-key-password",
          alias: "release",
          purpose: "application",
        },
      });
      imported = true;
      return route.fulfill({ json: { id: binding.credential_id } });
    }
    return route.fulfill({
      json: {
        configured: true,
        credentials: imported
          ? [
              {
                id: binding.credential_id,
                name: "Release",
                platform: "android",
                enabled: true,
                versions: [{ id: binding.version_id, metadata, enabled: true }],
              },
            ]
          : [],
      },
    });
  });
  await page.goto(tenantURL("/settings/general"));
  await openNavigation(page);
  await page.getByRole("navigation", { name: "管理分区", exact: true })
    .getByRole("link", { name: "签名管理", exact: true }).click();
  await expect(page).toHaveURL(tenantURL("/settings/signing"));
  await expect(page.locator('.sidebar a[href$="/settings/signing"]'))
    .toHaveAttribute("aria-current", "page");
  const form = page.locator("form").first();
  await form.getByLabel("凭据名称").fill("Release");
  await form.locator('input[name="file"]').setInputFiles({
    name: "release.p12",
    mimeType: "application/x-pkcs12",
    buffer: Buffer.from("test"),
  });
  await form
    .getByLabel("文件密码", { exact: true })
    .fill("private-store-password");
  await form
    .getByLabel("私钥密码", { exact: true })
    .fill("private-key-password");
  await form.getByLabel("密钥别名").fill("release");
  await form.getByRole("button", { name: "保存", exact: true }).click();
  await expect(page.getByText("最新版本", { exact: false })).toBeVisible();
  await expect(page.locator('input[name="password"]')).toHaveValue("");
  await expect(
    page.getByText("private-store-password", { exact: true }),
  ).toHaveCount(0);
  expect(imported).toBe(true);
  await page.screenshot({
    path: info.outputPath("signing-credentials.png"),
    fullPage: true,
  });
});

test("project defaults save profile names and display the fixed identity preview", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/signing/profiles", (route) =>
    route.fulfill({ json: [binding] }),
  );
  await page.route(
    `**/api/v1/projects/${project.id}/signing-preview`,
    (route) => route.fulfill({ json: [binding] }),
  );
  let saved = false;
  await page.route(
    `**/api/v1/projects/${project.id}/signing-defaults`,
    (route) => {
      expect(route.request().postDataJSON()).toEqual({
        android: "managed:release",
      });
      saved = true;
      return route.fulfill({ json: { ok: true } });
    },
  );
  await page.goto(tenantURL(`/projects/${project.slug}/settings/general`));
  await openNavigation(page);
  await page.getByRole("navigation", { name: "管理分区", exact: true })
    .getByRole("link", { name: "签名默认值", exact: true }).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/settings/signing`));
  await expect(page.locator('.sidebar a[href$="/settings/signing"]'))
    .toHaveAttribute("aria-current", "page");
  await expect(
    page.getByText("Android Release", { exact: false }),
  ).toBeVisible();
  await page.locator('select[name="android"]').selectOption("managed:release");
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect.poll(() => saved).toBe(true);
});

test("artifact evidence separates certificate verification and notarization", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route("**/api/v1/artifacts*", (route) =>
    route.fulfill({
      json: [
        {
          ...artifact,
          signing: {
            path: artifact.path,
            sha256: artifact.sha256,
            profile: binding.profile,
            version_id: binding.version_id,
            fingerprint_sha256: metadata.fingerprint_sha256,
            notarized: false,
          },
        },
      ],
    }),
  );
  await page.goto(tenantURL(`/projects/${project.slug}/artifacts`));
  await expect(page.getByText("签名已验证", { exact: true })).toBeVisible();
  await expect(page.getByText("未要求公证", { exact: true })).toBeVisible();
});

test("existing defaults survive an asynchronously loaded profile list", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, (route) =>
    route.fulfill({
      json: { ...project, signing_defaults: { android: binding.profile } },
    }),
  );
  let releaseProfiles!: () => void;
  const pending = new Promise<void>((resolve) => {
    releaseProfiles = resolve;
  });
  await page.route("**/api/v1/signing/profiles", async (route) => {
    await pending;
    await route.fulfill({ json: [binding] });
  });
  await page.route(
    `**/api/v1/projects/${project.id}/signing-preview`,
    (route) => route.fulfill({ json: [binding] }),
  );
  let saved: unknown;
  await page.route(
    `**/api/v1/projects/${project.id}/signing-defaults`,
    (route) => {
      saved = route.request().postDataJSON();
      return route.fulfill({ json: { ok: true } });
    },
  );
  await page.goto(tenantURL(`/projects/${project.slug}/settings/signing`));
  await expect(
    page.getByRole("heading", { name: "默认签名方案", exact: true }),
  ).toBeVisible();
  releaseProfiles();
  await expect(page.locator('select[name="android"]')).toHaveValue(
    binding.profile,
  );
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect.poll(() => saved).toEqual({ android: binding.profile });
});

test("an unavailable current default stays explicit until the user clears it", async ({
  page,
}) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/by-slug/${project.slug}`, (route) =>
    route.fulfill({
      json: { ...project, signing_defaults: { android: binding.profile } },
    }),
  );
  const unavailable = { ...binding, available: false };
  await page.route("**/api/v1/signing/profiles", (route) =>
    route.fulfill({ json: [unavailable] }),
  );
  await page.route(
    `**/api/v1/projects/${project.id}/signing-preview`,
    (route) => route.fulfill({ json: [unavailable] }),
  );
  let saved: unknown;
  await page.route(
    `**/api/v1/projects/${project.id}/signing-defaults`,
    (route) => {
      saved = route.request().postDataJSON();
      return route.fulfill({ json: { ok: true } });
    },
  );
  await page.goto(tenantURL(`/projects/${project.slug}/settings/signing`));
  await expect(
    page.getByText("当前方案不可用，请检查凭据、版本及证书有效期。", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(page.locator('select[name="android"]')).toHaveValue(
    binding.profile,
  );
  await page.getByRole("button", { name: "保存", exact: true }).click();
  await expect.poll(() => saved).toEqual({ android: binding.profile });
});
