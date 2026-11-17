/**
 * [INPUT]: 依赖镜像执行、发布和凭据 HTTP 夹具。
 * [OUTPUT]: 验证完整外部引用复制、发布重试和秘密输入清空。
 * [POS]: 镜像控制台的浏览器流程回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { build, fixtureAPI, project, tenantURL } from "./fixtures";
import type { ImageExecution } from "../../src/lib/types";
const digest = "sha256:" + "a".repeat(64);
const execution: ImageExecution = {
  id: "image-main", job_id: "image-job", parent_id: null, operation: "build",
  status: "succeeded", spec: { name: "application", platforms: ["linux/amd64", "linux/arm64"] },
  destination_repository: "team/project/images/application", destination_credential: null,
  tags: ["v1"], root_digest: digest, error: null,
};
const credential = { id: "registry-credential", name: "External", kind: "registry", enabled: true };

test("image outputs publish and retry by digest without rebuilding", async ({ page }, info) => {
  await fixtureAPI(page);
  const external = { ...execution, id: "publication-failed", parent_id: execution.id, operation: "publish", status: "publish_failed", destination_repository: "release/application", destination_credential: credential.id, destination_registry: "https://external.example.com:5443" };
  const result = { ...build, status: "succeeded", finished_at: build.created_at, artifacts: [], image_executions: [execution, external], image_registry: "https://images.example.com" };
  for (const path of ["projects/by-slug/desktop-client/builds/128", "builds/build-main"])
    await page.route(`**/api/v1/${path}`, route => route.fulfill({ json: result }));
  await page.route("**/api/v1/projects/project-main/image-credentials", route => route.fulfill({ json: [credential] }));
  await page.route("**/api/v1/builds/build-main/logs?*", route => route.fulfill({ json: { logs: [], next_before: null } }));
  await page.route("**/api/v1/image-executions/*/logs?*", route => route.fulfill({ json: [{ id: 1, platform: "linux/arm64", text: "verified platform result\n" }] }));
  const publications: unknown[] = [];
  await page.route("**/api/v1/image-executions/image-main/publish", async route => {
    publications.push(route.request().postDataJSON());
    await route.fulfill({ json: { id: "new-publication" } });
  });
  await page.goto(tenantURL("/projects/desktop-client/builds/128"));
  await page.getByRole("tab", { name: "制品", exact: true }).click();
  await expect(page.getByText(`images.example.com/${execution.destination_repository}@${digest}`, { exact: false })).toBeVisible();
  await expect(page.getByText(`external.example.com:5443/release/application@${digest}`, { exact: false })).toBeVisible();
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: async (value: string) => { document.documentElement.dataset.copiedImage = value; } },
  }));
  await page.locator("article").filter({ hasText: "external.example.com:5443/release/application" }).getByRole("button", { name: "复制", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied-image", `external.example.com:5443/release/application@${digest}`);
  await expect(page.getByText(`images.example.com/release/application`, { exact: false })).toHaveCount(0);
  await page.getByText("平台构建日志", { exact: true }).first().click();
  await expect(page.getByText("[linux/arm64] verified platform result", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "推送到外部仓库", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("combobox", { name: "仓库凭据", exact: true }).selectOption(credential.id);
  await dialog.getByLabel("目标仓库路径").fill("release/application");
  await dialog.getByLabel("标签（逗号分隔）").fill("v1,stable");
  await dialog.getByRole("button", { name: "推送到外部仓库", exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await page.getByRole("button", { name: "重试发布", exact: true }).click();
  await expect.poll(() => publications.length).toBe(2);
  expect(publications).toEqual([
    { credential_id: credential.id, repository: "release/application", tags: ["v1", "stable"] },
    { credential_id: credential.id, repository: "release/application", tags: ["v1"] },
  ]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: info.outputPath("image-outputs.png"), fullPage: true });
});

test("registry credential creation clears private inputs and supports revocation", async ({ page }) => {
  await fixtureAPI(page);
  let saved = false, enabled = true;
  await page.route("**/api/v1/projects/project-main/image-credentials", async route => {
    if (route.request().method() === "POST") {
      expect(route.request().postDataJSON()).toEqual({ kind: "registry", name: "External", login: { endpoint: "https://external.example.com", username: "builder", password: "fixture-private-password" } });
      saved = true;
      return route.fulfill({ json: { id: credential.id } });
    }
    return route.fulfill({ json: saved ? [{ ...credential, enabled }] : [] });
  });
  await page.route("**/api/v1/projects/project-main/image-credentials/registry-credential", async route => {
    expect(route.request().method()).toBe("DELETE"); enabled = false;
    await route.fulfill({ json: { ok: true } });
  });
  await page.goto(tenantURL(`/projects/${project.slug}/settings/resources`));
  const section = page.locator("section").filter({ has: page.getByRole("heading", { name: "镜像与构建凭据", exact: true }) }).last();
  await section.getByLabel("名称", { exact: true }).fill("External");
  await section.getByLabel("仓库地址", { exact: true }).fill("https://external.example.com");
  await section.getByLabel("用户名", { exact: true }).fill("builder");
  await section.getByLabel("密码或访问令牌", { exact: true }).fill("fixture-private-password");
  await section.getByRole("button", { name: "新增凭据", exact: true }).click();
  await expect(section.locator('input[name="password"]')).toHaveValue("");
  await expect(section.getByText("registry-credential", { exact: true })).toBeVisible();
  await section.getByRole("button", { name: "停用", exact: true }).click();
  await expect(section.getByText("已停用", { exact: true })).toBeVisible();
});

test("project image catalog and immutable release expose digest references", async ({ page }) => {
  await fixtureAPI(page);
  const image = { id: "verified-image", project_id: project.id, name: "application", target: "container", repository: execution.destination_repository, digest,
    platforms: [{ platform: "linux/amd64", digest }, { platform: "linux/arm64", digest }], project_slug: project.slug, build_number: build.number, registry: "https://images.example.com", created_at: build.created_at };
  await page.route("**/api/v1/images?*", route => {
    expect(new URL(route.request().url()).searchParams.get("project_id")).toBe(project.id);
    return route.fulfill({ json: [image] });
  });
  await page.route("**/api/v1/releases/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", route => route.fulfill({ json: {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", project_id: project.id, project_name: project.name, project_slug: project.slug,
    build_number: build.number, build_id: build.id, version: "v2", commit_sha: build.commit_sha,
    created_at: build.created_at, artifact_count: 1, artifacts: [], images: [image], image_registry: image.registry,
  } }));
  await page.goto(tenantURL(`/projects/${project.slug}/artifacts`));
  await expect(page.getByRole("heading", { name: "镜像结果", exact: true })).toBeVisible();
  await expect(page.getByText(`images.example.com/${image.repository}@${digest}`, { exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "#128", exact: true })).toHaveAttribute("href", tenantURL(`/projects/${project.slug}/builds/128`));
  await page.goto(tenantURL(`/projects/${project.slug}/releases/aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa`));
  await expect(page.getByRole("heading", { name: "镜像结果", exact: true })).toBeVisible();
  await expect(page.getByText(`images.example.com/${image.repository}@${digest}`, { exact: false })).toBeVisible();
});
