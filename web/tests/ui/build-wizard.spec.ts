/**
 * [INPUT]: 依赖独立 Release 说明、Build 目标、源码夹具和共享下拉。
 * [OUTPUT]: 验证版本创建、说明编辑、独立构建与轮询稳定的单构建交付。
 * [POS]: Release 与 Build 职责边界的浏览器回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL, openNavigation } from "./fixtures";
import { compareReleaseVersions, isReleaseVersion } from "../../src/lib/releases";
import { validBuildTargets } from "../../src/lib/build-targets";
import type { Release } from "../../src/lib/types";
const path = `/projects/${project.slug}/releases/${release.id}`;
const defaultTargets = [{kind:"native",key:"linux-amd64",os:"linux",arch:"amd64",application_platform:"linux",artifact_key:"app",format_suffix:".tar.gz"}];

test("strict SemVer rejects normalization and orders prerelease identifiers numerically", () => {
  for (const version of ["1.2.0","1.2.0-rc.1","0.0.0","1.0.0-0","1.0.0-01a"]) expect(isReleaseVersion(version),version).toBe(true);
  for (const version of ["v1.2.0","01.2.0","1.02.0","1.2.03","1.2.0-01","1.2.0+abc","1.2","1.2.0 ","1.2.0-中文","1.2.0-"+"a".repeat(100)]) expect(isReleaseVersion(version),version).toBe(false);
  const versions = ["2.0.0","1.2.0-rc.10","1.2.0-rc.2","1.2.0","1.2.0-alpha","1.2.0-rc.1"];
  expect(versions.sort(compareReleaseVersions)).toEqual(["1.2.0-alpha","1.2.0-rc.1","1.2.0-rc.2","1.2.0-rc.10","1.2.0","2.0.0"]);
});


test("build target validation rejects duplicate keys and mismatched hosts", () => {
  expect(validBuildTargets(build.target_requirements)).toBe(true);
  expect(validBuildTargets([...build.target_requirements, ...build.target_requirements])).toBe(false);
  expect(validBuildTargets([{kind:"native",key:"ios",os:"linux",arch:"arm64",application_platform:"ios"}])).toBe(false);
  expect(validBuildTargets([{kind:"image",key:"image",platforms:[]}])).toBe(false);
});

test("project navigation creates a Release using only version and multiline notes", async ({page}) => {
  const events = await fixtureAPI(page);
  let payload: unknown;
  await page.route(`**/api/v1/projects/${project.id}/releases`, async route => {
    payload = route.request().postDataJSON(); await route.fulfill({json:release});
  });
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await openNavigation(page);
  await expect(page.locator(".sidebar .nav-item")).toHaveText(["项目概览","Release 版本","项目设置","文档"]);
  if (page.viewportSize()!.width < 1024) await page.keyboard.press("Escape");
  await page.getByRole("link",{name:"新建 Release",exact:true}).click();
  await page.locator('input[name="version"]').fill("v1.2.0");
  await page.locator('textarea[name="notes"]').fill("Changes\nDelivery notes");
  await page.getByRole("button",{name:"新建 Release",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("严格 SemVer");
  expect(payload).toBeUndefined();
  await page.locator('input[name="version"]').fill("1.2.0-rc.1");
  await page.getByRole("button",{name:"新建 Release",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(path));
  expect(payload).toEqual({version:"1.2.0-rc.1",notes:"Changes\nDelivery notes"});
  expect(events.writes).toEqual([]);
});

for (const status of ["draft", "publishing", "published", "publish_failed"] as const) test(`${status} Release edits only notes`,async({page})=>{
  await fixtureAPI(page);
  let saved: Release = {...release,status};
  const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,async route=>{
    if(route.request().method()==="PATCH") { const body=route.request().postDataJSON(); writes.push(body); saved={...saved,...body}; }
    await route.fulfill({json:saved});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"编辑说明",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('input[name="version"]')).toHaveAttribute("readonly","");
  await expect(dialog.locator('input[name="ref"]')).toHaveCount(0);
  await dialog.locator('textarea[name="notes"]').fill("Updated notes\nAnother line");
  await dialog.getByRole("button",{name:"保存说明",exact:true}).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText("Updated notes\nAnother line",{exact:true})).toBeVisible();
  expect(writes).toEqual([{notes:"Updated notes\nAnother line"}]);
});

test("polling preserves unsaved notes and a failed save retains the input",async({page})=>{
  await fixtureAPI(page);
  let saved = release;
  const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,async route=>{
    if(route.request().method()==="PATCH") { writes.push(route.request().postDataJSON()); return route.fulfill({status:503,json:{error:"notes temporarily unavailable"}}); }
    await route.fulfill({json:saved});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"编辑说明",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await dialog.locator('textarea[name="notes"]').fill("Keep my unsaved notes");
  saved={...release,status:"published",notes:"Another editor saved this"};
  await expect(page.getByText("Another editor saved this",{exact:true})).toBeVisible({timeout:10000});
  await expect(dialog.locator('textarea[name="notes"]')).toHaveValue("Keep my unsaved notes");
  await dialog.getByRole("button",{name:"保存说明",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("notes temporarily unavailable");
  expect(writes).toEqual([{notes:"Keep my unsaved notes"}]);
  await expect(dialog.locator('textarea[name="notes"]')).toHaveValue("Keep my unsaved notes");
});

test("a build submits its independent tag, required goal, targets and protected parameters",async({page})=>{
  const events=await fixtureAPI(page); const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}/builds`,async route=>{
    if(route.request().method()==="POST") { writes.push(route.request().postDataJSON()); return route.fulfill({json:build}); }
    return route.fulfill({json:[build]});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('textarea[name="goal"]')).toHaveValue("");
  await dialog.locator('input[name="ref"]').fill("refs/tags/v1.2.0");
  await page.keyboard.press("Escape");
  await dialog.getByRole("textbox",{name:"目标键",exact:true}).fill("linux-production");
  await dialog.locator('textarea[name="goal"]').fill("Build from the selected tag");
  await dialog.locator('textarea[name="parameters"]').fill('{"ARTIFACTORY_RELEASE_VERSION":"2.0.0"}');
  await dialog.getByRole("button",{name:"发起构建",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("参数必须为字符串键值 JSON 对象，且不能覆盖平台 Release 变量。");
  expect(writes).toEqual([]);
  await dialog.locator('textarea[name="parameters"]').fill('{"PROFILE":"production"}');
  await dialog.getByRole("button",{name:"发起构建",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(`${path}/builds/${build.number}`));
  expect(writes).toEqual([{ref:"refs/tags/v1.2.0",goal:"Build from the selected tag",targets:["linux-production"],target_requirements:defaultTargets.map(target => ({...target,key:"linux-production"})),parameters:{PROFILE:"production"}}]);
  expect(events.writes).toEqual([]);
});

test("publication selects one successful build across independent source and target contracts",async({page})=>{
  await fixtureAPI(page);
  const current={...build,id:"build-current",number:131,status:"succeeded",requested_ref:"refs/heads/release/v1"};
  const previous={...build,id:"build-old",number:130,status:"succeeded",requested_ref:"refs/tags/v1.1.0",target_requirements:[{kind:"image",key:"image",platforms:["linux/amd64"]}],selected_targets:["image"]};
  let saved: Release=release; const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:saved}));
  await page.route(`**/api/v1/releases/${release.id}/builds`,route=>route.fulfill({json:[current,previous,{...build,id:"failed-current",number:132,status:"failed"}]}));
  await page.route(`**/api/v1/releases/${release.id}/publish`,async route=>{writes.push(route.request().postDataJSON());saved={...saved,status:"published",selected_build_id:previous.id};await route.fulfill({json:saved});});
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"正式发布",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await dialog.getByRole("combobox").click();
  const options=page.getByRole("listbox").getByRole("option");
  await expect(options).toHaveCount(2);
  await expect(options.nth(0)).toContainText(`refs/heads/release/v1 · ${build.commit_sha} · macos (darwin/arm64)`);
  await expect(options.nth(1)).toContainText("refs/tags/v1.1.0");
  await expect(options.nth(1)).toContainText("image (linux/amd64)");
  await options.nth(1).click();
  await dialog.getByRole("button",{name:"确认正式发布",exact:true}).click();
  await expect(page.getByRole("button",{name:"编辑说明",exact:true})).toBeVisible();
  await expect(page.getByRole("button",{name:"发起构建",exact:true})).toHaveCount(0);
  expect(writes).toEqual([{build_id:previous.id}]);
});

test("failed publication retries the same selected delivery",async({page})=>{
  await fixtureAPI(page); const writes:unknown[]=[];
  let failed: Release={...release,status:"publish_failed",selected_build_id:"build-frozen",publish_error:"registry temporarily unavailable"};
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:failed}));
  await page.route(`**/api/v1/releases/${release.id}/publish`,async route=>{writes.push(route.request().postDataJSON());failed={...failed,status:"publishing"};await route.fulfill({json:failed});});
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"重试正式发布",exact:true}).click();
  await expect(page.getByRole("dialog").getByRole("combobox")).toHaveCount(0);
  await page.getByRole("button",{name:"确认正式发布",exact:true}).click();
  await expect(page.getByText("正在冻结并发布所选构建的交付物。说明仍可编辑。",{exact:true})).toBeVisible();
  expect(writes).toEqual([{build_id:"build-frozen"}]);
});

test("Release list searches notes and sorts SemVer",async({page})=>{
  await fixtureAPI(page);
  await page.route("**/api/v1/releases?*",route=>route.fulfill({json:[{...release,id:"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",version:"1.2.0-rc.10",notes:"older candidate"},{...release,id:"cccccccc-cccc-4ccc-8ccc-cccccccccccc",version:"1.2.0",notes:"needle notes"},{...release,version:"1.2.0-rc.2"}]}));
  await page.goto(tenantURL(`/projects/${project.slug}/releases`));
  await expect(page.locator("tbody tr td:first-child")).toHaveText(["1.2.0","1.2.0-rc.10","1.2.0-rc.2"]);
  await page.locator('.workspace-filter input').fill("needle");
  await expect(page.locator("tbody tr td:first-child")).toHaveText(["1.2.0"]);
});

for (const role of ["viewer","archived"] as const) test(`${role} contexts expose no release mutation`,async({page})=>{
  const events=await fixtureAPI(page,role === "viewer" ? "viewer" : "admin");
  if(role === "archived") await page.route(`**/api/v1/projects/by-slug/${project.slug}`,route=>route.fulfill({json:{...project,status:"archived"}}));
  await page.goto(tenantURL(path));
  for(const name of ["编辑说明","发起构建","正式发布"]) await expect(page.getByRole("button",{name,exact:true})).toHaveCount(0);
  await page.goto(tenantURL(`/projects/${project.slug}/releases/new`));
  await expect(page.locator('input[name="version"]')).toHaveCount(0);
  expect(events.writes).toEqual([]);
});
test("Release business fields are version and notes only", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL(`/projects/${project.slug}/releases/new`));
  await expect(page.locator('input[name="version"]')).toBeVisible();
  await expect(page.locator('textarea[name="notes"]')).toBeVisible();
  await expect(page.locator('input[name="ref"]')).toHaveCount(0);
  await expect(page.locator('textarea[name="goal"]')).toHaveCount(0);
});

test("each build chooses its own source and target contract", async ({ page }) => {
  await fixtureAPI(page);
  await page.goto(tenantURL(path));
  await page.getByRole("button", { name: "发起构建", exact: true }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await expect(dialog.locator('textarea[name="goal"]')).toHaveAttribute("required", "");
  await expect(dialog.getByRole("button", { name: "添加目标", exact: true })).toBeVisible();
});

test("published Release notes remain editable", async ({ page }) => {
  await fixtureAPI(page);
  await page.route(`**/api/v1/releases/${release.id}`, route => route.fulfill({ json: { ...release, status: "published" } }));
  await page.goto(tenantURL(path));
  await expect(page.getByRole("button", { name: "编辑说明", exact: true })).toBeVisible();
});


for (const status of ["draft", "publishing", "published", "publish_failed"] as const) test(`${status} controls execution retry while retaining build source and targets`, async ({ page }) => {
  await fixtureAPI(page);
  const failed = {...build, status:"failed", jobs:build.jobs?.map(job => ({...job,status:"failed"}))};
  await page.route(`**/api/v1/releases/${release.id}`, route => route.fulfill({json:{...release,status}}));
  await page.route(`**/api/v1/projects/by-slug/${project.slug}/builds/${build.number}`, route => route.fulfill({json:failed}));
  await page.goto(tenantURL(`${path}/builds/${build.number}`));
  await expect(page.locator(".build-meta")).toContainText(build.requested_ref);
  await expect(page.locator(".build-meta")).toContainText(build.commit_sha!);
  await expect(page.locator(".build-meta")).toContainText("macos (darwin/arm64)");
  await expect(page.getByRole("button", {name:"以新 attempt 重试此目标",exact:true})).toHaveCount(status === "draft" ? 1 : 0);
});


for (const invalidates of [false, true]) test(`publication polling ${invalidates ? "disables an invalid selected build" : "keeps its initial successful build"}`, async ({page}) => {
  await fixtureAPI(page);
  const first = {...build,id:"build-first",number:130,status:"succeeded",requested_ref:"refs/heads/stable"};
  const newer = {...build,id:"build-newer",number:131,status:"running",requested_ref:"refs/tags/newer"};
  let polled = false;
  let refreshed = false;
  const writes: unknown[] = [];
  await page.route(`**/api/v1/releases/${release.id}/builds`, route => {
    if (polled) refreshed = true;
    return route.fulfill({json:[{...newer,status:polled ? "succeeded" : "running"},{...first,status:polled && invalidates ? "failed" : "succeeded"}]});
  });
  await page.route(`**/api/v1/releases/${release.id}/publish`, route => {
    writes.push(route.request().postDataJSON());
    return route.fulfill({json:{...release,status:"published",selected_build_id:first.id}});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button", {name:"正式发布",exact:true}).click();
  const dialog = page.getByRole("dialog");
  const selection = dialog.getByRole("combobox");
  await expect(selection).toHaveAttribute("data-value",first.id);
  await expect(selection).toContainText("refs/heads/stable");
  polled = true;
  await expect.poll(() => refreshed).toBe(true);
  const confirm = dialog.getByRole("button",{name:"确认正式发布",exact:true});
  if (invalidates) {
    await expect(confirm).toBeDisabled();
    await expect(selection).toHaveAttribute("data-value", "");
    await expect(selection).not.toContainText("refs/tags/newer");
    expect(writes).toEqual([]);
    await selection.click();
    await page.getByRole("option",{name:/#131/}).click();
    await expect(confirm).toBeEnabled();
    return;
  }
  await expect(selection).toHaveAttribute("data-value",first.id);
  await expect(selection).toContainText("refs/heads/stable");
  await confirm.click();
  expect(writes).toEqual([{build_id:first.id}]);
});
