/**
 * [INPUT]: 依赖 Release、目标修订、构建夹具与全局通知。
 * [OUTPUT]: 验证版本入口、严格 SemVer、合同修订和冻结交付。
 * [POS]: Release 中心交付流程的浏览器回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test } from "@playwright/test";
import { build, fixtureAPI, project, release, tenantURL, openNavigation } from "./fixtures";
import { compareReleaseVersions, isReleaseVersion } from "../../src/lib/releases";
const path = `/projects/${project.slug}/releases/${release.id}`;

test("strict SemVer rejects normalization and orders prerelease identifiers numerically", () => {
  for (const version of ["1.2.0","1.2.0-rc.1","0.0.0","1.0.0-0","1.0.0-01a"]) expect(isReleaseVersion(version),version).toBe(true);
  for (const version of ["v1.2.0","01.2.0","1.02.0","1.2.03","1.2.0-01","1.2.0+abc","1.2","1.2.0 ","1.2.0-中文","1.2.0-"+"a".repeat(100)]) expect(isReleaseVersion(version),version).toBe(false);
  const versions = ["2.0.0","1.2.0-rc.10","1.2.0-rc.2","1.2.0","1.2.0-alpha","1.2.0-rc.1"];
  expect(versions.sort(compareReleaseVersions)).toEqual(["1.2.0-alpha","1.2.0-rc.1","1.2.0-rc.2","1.2.0-rc.10","1.2.0","2.0.0"]);
});

test("project navigation centers Releases and creates a version with explicit targets", async ({page}) => {
  const events = await fixtureAPI(page);
  let payload: Record<string,unknown> | undefined;
  await page.route(`**/api/v1/projects/${project.id}/releases`, async route => {
    payload = route.request().postDataJSON(); await route.fulfill({json:release});
  });
  await page.goto(tenantURL(`/projects/${project.slug}`));
  await openNavigation(page);
  await expect(page.locator(".sidebar .nav-item")).toHaveText(["项目概览","Release 版本","项目设置","文档"]);
  if (page.viewportSize()!.width < 1024) await page.keyboard.press("Escape");
  await page.getByRole("link",{name:"新建 Release",exact:true}).click();
  await expect(page.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await page.locator('input[name="version"]').fill("v1.2.0");
  await page.locator('textarea[name="goal"]').fill("Build signed packages");
  await page.getByRole("button",{name:"新建 Release",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("严格 SemVer");
  expect(payload).toBeUndefined();
  await page.locator('input[name="version"]').fill("1.2.0-rc.1");
  await page.getByRole("button",{name:"新建 Release",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(path));
  expect(payload).toEqual({version:"1.2.0-rc.1",ref:"refs/heads/main",goal:"Build signed packages",targets:["linux-amd64"],target_requirements:[{kind:"native",key:"linux-amd64",os:"linux",arch:"amd64",application_platform:"linux",artifact_key:"app",format_suffix:".tar.gz"}]});
  expect(events.writes).toEqual([]);
});

test("draft edits preserve version and ref and submit the expected target revision",async({page})=>{
  await fixtureAPI(page);
  let saved = release;
  const writes: unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,async route=>{
    if(route.request().method()==="PATCH") { const body=route.request().postDataJSON();writes.push(body);saved={...saved,...body,spec_revision:2}; }
    await route.fulfill({json:saved});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"编辑草稿",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('input[name="version"]')).toHaveAttribute("readonly","");
  await expect(dialog.locator('input[name="ref"]')).toHaveAttribute("readonly","");
  await dialog.locator('textarea[name="goal"]').fill("Deliver packages and verification evidence");
  await dialog.getByRole("button",{name:"保存草稿",exact:true}).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByText("目标修订 2",{exact:true}).first()).toBeVisible();
  expect(writes).toEqual([{spec_revision:1,goal:"Deliver packages and verification evidence",target_requirements:release.target_requirements,targets:release.targets}]);
});

test("polling a newer revision preserves an unsaved draft and submits its original revision",async({page})=>{
  await fixtureAPI(page);
  let saved = release;
  const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,async route=>{
    if(route.request().method()==="PATCH") {
      writes.push(route.request().postDataJSON());
      return route.fulfill({status:409,json:{error:"release draft revision changed"}});
    }
    await route.fulfill({json:saved});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"编辑草稿",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await dialog.locator('textarea[name="goal"]').fill("Keep my unsaved requirements");
  saved={...release,spec_revision:2,goal:"Another editor saved this"};
  await expect(page.getByText("目标修订 2",{exact:true}).first()).toBeVisible({timeout:10000});
  await expect(dialog.locator('textarea[name="goal"]')).toHaveValue("Keep my unsaved requirements");
  await dialog.getByRole("button",{name:"保存草稿",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("release draft revision changed");
  expect(writes).toEqual([{spec_revision:1,goal:"Keep my unsaved requirements",target_requirements:release.target_requirements,targets:release.targets}]);
  await expect(dialog.locator('textarea[name="goal"]')).toHaveValue("Keep my unsaved requirements");
});

test("a build inherits release identity and cannot override injected version parameters",async({page})=>{
  const events=await fixtureAPI(page);
  const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}/builds`,async route=>{
    if(route.request().method()==="POST") {writes.push(route.request().postDataJSON());return route.fulfill({json:build});}
    return route.fulfill({json:[build]});
  });
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('input[name="version"]')).toHaveCount(0);
  await expect(dialog.getByText(release.version,{exact:true})).toBeVisible();
  await dialog.locator('textarea[name="parameters"]').fill('{"ARTIFACTORY_RELEASE_VERSION":"2.0.0"}');
  await dialog.getByRole("button",{name:"发起构建",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("参数必须为字符串键值 JSON 对象，且不能覆盖平台 Release 变量。");
  expect(writes).toEqual([]);
  await dialog.locator('textarea[name="parameters"]').fill('{"PROFILE":"production"}');
  await dialog.getByRole("button",{name:"发起构建",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(`${path}/builds/${build.number}`));
  await expect(page.getByRole("link",{name:`返回 Release ${release.version}`,exact:true})).toHaveAttribute("href",tenantURL(path));
  expect(writes).toEqual([{parameters:{PROFILE:"production"}}]);
  expect(events.writes).toEqual([]);
});

test("publication accepts only a current-revision successful build and never a new version",async({page})=>{
  await fixtureAPI(page);
  const current={...build,id:"build-current",number:131,status:"succeeded",spec_revision:2};
  const previous={...build,id:"build-old",number:130,status:"succeeded",spec_revision:1};
  let saved={...release,spec_revision:2}; const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:saved}));
  await page.route(`**/api/v1/releases/${release.id}/builds`,route=>route.fulfill({json:[current,previous,{...build,id:"failed-current",number:132,status:"failed",spec_revision:2}]}));
  await page.route(`**/api/v1/releases/${release.id}/publish`,async route=>{writes.push(route.request().postDataJSON());saved={...saved,status:"published",selected_build_id:current.id};await route.fulfill({json:saved});});
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"正式发布",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.getByRole("option")).toHaveText(["#131 · 目标修订 2"]);
  await expect(dialog.getByRole("textbox")).toHaveCount(0);
  await dialog.getByRole("button",{name:"确认正式发布",exact:true}).click();
  await expect(page.getByRole("button",{name:"编辑草稿",exact:true})).toHaveCount(0);
  await expect(page.getByRole("button",{name:"发起构建",exact:true})).toHaveCount(0);
  expect(writes).toEqual([{build_id:current.id}]);
});

test("failed publication retries the same selected delivery",async({page})=>{
  await fixtureAPI(page); const writes:unknown[]=[];
  let failed={...release,status:"publish_failed",selected_build_id:"build-frozen",publish_error:"registry temporarily unavailable"};
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:failed}));
  await page.route(`**/api/v1/releases/${release.id}/publish`,async route=>{writes.push(route.request().postDataJSON());failed={...failed,status:"publishing"};await route.fulfill({json:failed});});
  await page.goto(tenantURL(path));
  await page.getByRole("button",{name:"重试正式发布",exact:true}).click();
  await expect(page.getByRole("dialog").getByRole("combobox")).toHaveCount(0);
  await page.getByRole("button",{name:"确认正式发布",exact:true}).click();
  await expect(page.getByText("正在冻结并发布交付物。版本、源码与目标合同已锁定。",{exact:true})).toBeVisible();
  expect(writes).toEqual([{build_id:"build-frozen"}]);
});

test("Release list sorts SemVer instead of creation dates",async({page})=>{
  await fixtureAPI(page);
  await page.route("**/api/v1/releases?*",route=>route.fulfill({json:[{...release,id:"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",version:"1.2.0-rc.10"},{...release,id:"cccccccc-cccc-4ccc-8ccc-cccccccccccc",version:"1.2.0"},{...release,version:"1.2.0-rc.2"}]}));
  await page.goto(tenantURL(`/projects/${project.slug}/releases`));
  await expect(page.locator("tbody tr td:first-child")).toHaveText(["1.2.0","1.2.0-rc.10","1.2.0-rc.2"]);
});

for (const role of ["viewer","archived"] as const) test(`${role} contexts expose no release mutation`,async({page})=>{
  const events=await fixtureAPI(page,role === "viewer" ? "viewer" : "admin");
  if(role === "archived") await page.route(`**/api/v1/projects/by-slug/${project.slug}`,route=>route.fulfill({json:{...project,status:"archived"}}));
  await page.goto(tenantURL(path));
  for(const name of ["编辑草稿","发起构建","正式发布"]) await expect(page.getByRole("button",{name,exact:true})).toHaveCount(0);
  await page.goto(tenantURL(`/projects/${project.slug}/releases/new`));
  await expect(page.locator('input[name="version"]')).toHaveCount(0);
  expect(events.writes).toEqual([]);
});
