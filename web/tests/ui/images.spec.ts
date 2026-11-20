/**
 * [INPUT]: 依赖镜像执行、发布和凭据 HTTP 夹具和共享 Select 交互助手。
 * [OUTPUT]: 验证完整外部引用复制、发布重试和秘密输入清空。
 * [POS]: 镜像控制台的浏览器流程回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { chooseOption } from "./select-helpers";
import { expect, test } from "@playwright/test";
import { artifact, build, fixtureAPI, project, release, tenantURL } from "./fixtures";
import type { ImageExecution } from "../../src/lib/types";
const digest = "sha256:" + "a".repeat(64);
const execution: ImageExecution = {
  id: "image-main", job_id: "image-job", parent_id: null, operation: "build",
  status: "succeeded", spec: { name: "application", platforms: ["linux/amd64", "linux/arm64"] },
  destination_repository: "team/project/images/application", destination_credential: null,
  tags: ["v1"], root_digest: digest, error: null,
};
const credential = { id: "registry-credential", name: "External", kind: "registry", enabled: true };

test("published Release images distribute a fixed version and show independent publication state", async ({ page }, info) => {
  await fixtureAPI(page);
  const external = { ...execution, id: "publication-failed", parent_id: execution.id, operation: "publish", status: "publish_failed", destination_repository: "release/application", destination_credential: credential.id, destination_registry: "https://external.example.com:5443", tags:[release.version] };
  const result = { ...build, status: "succeeded", finished_at: build.created_at, artifacts: [], image_executions: [execution, external], image_registry: "https://images.example.com" };
  const image = {id:"official-image",execution_id:execution.id,name:"application",target:"container",repository:execution.destination_repository,digest,version:release.version,platforms:[{platform:"linux/amd64",digest},{platform:"linux/arm64",digest}]};
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:{...release,status:"published",selected_build_id:build.id,images:[image],image_registry:result.image_registry}}));
  for (const path of ["projects/by-slug/desktop-client/builds/128", "builds/build-main"])
    await page.route(`**/api/v1/${path}`, route => route.fulfill({ json: result }));
  await page.route("**/api/v1/projects/project-main/image-credentials", route => route.fulfill({ json: [credential] }));
  await page.route("**/api/v1/image-executions/*/logs?*", route => route.fulfill({ json: [{ id: 1, platform: "linux/arm64", text: "verified platform result\n" }] }));
  const publications: unknown[] = [];
  await page.route("**/api/v1/image-executions/image-main/publish", async route => {
    publications.push(route.request().postDataJSON()); await route.fulfill({ json: { id: "new-publication" } });
  });
  await page.goto(tenantURL(`/projects/desktop-client/releases/${release.id}`));
  await page.getByRole("tab",{name:"交付物",exact:true}).click();
  await expect(page.getByText(`images.example.com/${execution.destination_repository}:${release.version}`,{exact:false})).toBeVisible();
  await expect(page.getByText(`external.example.com:5443/release/application@${digest}`, { exact: false })).toBeVisible();
  await page.evaluate(() => Object.defineProperty(navigator, "clipboard", { configurable:true,value:{writeText:async(value:string)=>{document.documentElement.dataset.copiedImage=value;}} }));
  await page.locator("article").filter({ hasText: "external.example.com:5443/release/application" }).getByRole("button", { name: "复制", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-copied-image", `external.example.com:5443/release/application@${digest}`);
  await page.getByText("平台构建日志", { exact: true }).first().click();
  await expect(page.getByText("[linux/arm64] verified platform result", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "推送到外部仓库", exact: true }).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.getByLabel("版本标签（固定）")).toHaveValue(release.version);
  await expect(dialog.getByLabel("版本标签（固定）")).toHaveAttribute("readonly","");
  await chooseOption(dialog.getByRole("combobox",{name:"仓库凭据",exact:true}), credential.id);
  await dialog.getByLabel("目标仓库路径").fill("release/application");
  await dialog.getByRole("button",{name:"推送到外部仓库",exact:true}).click();
  await expect(dialog).not.toBeVisible();
  expect(publications).toEqual([{credential_id:credential.id,repository:"release/application"}]);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBeTruthy();
  await page.screenshot({path:info.outputPath("release-image-delivery.png"),fullPage:true});
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

test("formal delivery uses server filenames, version links and downloadable manifests", async ({page})=>{
 await fixtureAPI(page);
 const image={id:"verified-image",execution_id:execution.id,release_id:release.id,release_version:release.version,project_id:project.id,name:"application",target:"container",repository:execution.destination_repository,digest,platforms:[{platform:"linux/amd64",digest},{platform:"linux/arm64",digest}],project_slug:project.slug,build_number:build.number,registry:"https://images.example.com",created_at:build.created_at};
 await page.route("**/api/v1/images?*",route=>route.fulfill({json:[image]}));
 const filename=`desktop-client-app-${release.version}-macos-arm64.zip`;
 await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:{...release,status:"published",selected_build_id:build.id,artifacts:[{...artifact,delivery_filename:filename,release_id:release.id,release_version:release.version}],images:[image],image_registry:image.registry,delivery_manifest:{version:release.version}}}));
 await page.route(`**/api/v1/releases/${release.id}/manifest`,route=>route.fulfill({headers:{"Content-Disposition":`attachment; filename="desktop-client-${release.version}-release.json"`},json:{version:release.version}}));
 await page.route(`**/api/v1/releases/${release.id}/checksums`,route=>route.fulfill({headers:{"Content-Disposition":`attachment; filename="desktop-client-${release.version}-SHA256SUMS"`},contentType:"text/plain",body:`${artifact.sha256}  ${filename}\n`}));
 await page.goto(tenantURL(`/projects/${project.slug}/artifacts`));
 await expect(page.getByRole("link",{name:release.version,exact:true})).toHaveAttribute("href",tenantURL(`/projects/${project.slug}/releases/${release.id}`));
 await expect(page.getByRole("link",{name:"#128",exact:true})).toHaveAttribute("href",tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/128`));
 await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
 await page.getByRole("tab",{name:"交付物",exact:true}).click();
 await expect(page.getByText(filename,{exact:true})).toBeVisible();
 await expect(page.getByText(`images.example.com/${image.repository}@${digest}`,{exact:false})).toBeVisible();
 for(const name of ["版本 Manifest","SHA256SUMS"]){const download=page.waitForEvent("download");await page.getByRole("button",{name,exact:true}).click();const file=await download;expect(file.suggestedFilename()).toBe(name==="SHA256SUMS"?`desktop-client-${release.version}-SHA256SUMS`:`desktop-client-${release.version}-release.json`);}
});

test("candidate build images provide logs without external distribution actions",async({page})=>{
 await fixtureAPI(page);
 await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128",route=>route.fulfill({json:{...build,status:"succeeded",image_executions:[execution],image_registry:"https://images.example.com"}}));
 await page.route("**/api/v1/builds/build-main/logs?*",route=>route.fulfill({json:{logs:[],next_before:null}}));
 await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/128`));
 await page.getByRole("tab",{name:"制品",exact:true}).click();
 await expect(page.getByRole("button",{name:"推送到外部仓库",exact:true})).toHaveCount(0);
 await expect(page.getByRole("button",{name:"发布版本",exact:true})).toHaveCount(0);
});

for (const finishes of [false, true]) test(`slow image log ${finishes ? "completion reads" : "polling"} preserve output without foreground progress`, async ({ page }) => {
  await fixtureAPI(page);
  const publication = { ...execution, id: "publication-running", parent_id: execution.id, operation: "publish", status: "running" };
  let details = 0;
  await page.route("**/api/v1/projects/by-slug/desktop-client/builds/128", route => {
    const running = !finishes || ++details === 1;
    return route.fulfill({ json: {
      ...build, status: "succeeded", publish_status: finishes && running ? "pending" : "succeeded",
      image_executions: [{ ...publication, status: running ? "running" : "succeeded" }],
    } });
  });
  await page.route("**/api/v1/builds/build-main/logs?*", route => route.fulfill({ json: { logs: [], next_before: null } }));
  let finishInitial!: () => void;
  let finishPoll!: () => void;
  let pollStarted!: () => void;
  const initial = new Promise<void>(resolve => { finishInitial = resolve; });
  const pending = new Promise<void>(resolve => { finishPoll = resolve; });
  const started = new Promise<void>(resolve => { pollStarted = resolve; });
  let reads = 0;
  await page.route("**/api/v1/image-executions/publication-running/logs?*", async route => {
    const first = ++reads === 1;
    if (!first) pollStarted();
    await (first ? initial : pending);
    await route.fulfill({ json: [{ id: first ? 1 : 2, platform: "linux/amd64", text: first ? "existing image output\n" : "next image output\n" }] });
  });
  try {
    await page.goto(tenantURL(`/projects/desktop-client/releases/${release.id}/builds/128`));
    await page.getByRole("tab", { name: "制品", exact: true }).click();
    await page.getByText("平台构建日志", { exact: true }).click();
    const progress = page.getByRole("progressbar");
    await expect(progress).toBeVisible();
    finishInitial();
    await expect(page.getByText("[linux/amd64] existing image output", { exact: false })).toBeVisible();
    await expect(progress).toBeHidden();
    await started;
    if (finishes) await expect(page.locator("article .status-succeeded")).toBeVisible();
    await page.waitForTimeout(350);
    await expect(progress).toBeHidden();
    await expect(page.getByText("[linux/amd64] existing image output", { exact: false })).toBeVisible();
    finishPoll();
    await expect(page.getByText("next image output", { exact: false })).toBeVisible();
  } finally {
    finishInitial();
    finishPoll();
  }
});
