/**
 * [INPUT]: 依赖真实构建引导、Release 轮询与构建接口夹具。
 * [OUTPUT]: 验证四步草稿、最终创建、多目标、主题与无障碍。
 * [POS]: Release 内分步弹窗的行为验收。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { build, fixtureAPI, project, release, tenantURL } from "./fixtures";

test("four guided steps preserve answers and create a build only after confirmation", async ({page}) => {
  await fixtureAPI(page);
  const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}/builds`,async route=>{
    if(route.request().method()==="POST") { writes.push(route.request().postDataJSON());return route.fulfill({json:build}); }
    return route.fulfill({json:[]});
  });
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.getByRole("heading",{name:"要构建哪份源码？",exact:true})).toBeFocused();
  await expect(dialog.locator('textarea[name="goal"]')).toHaveCount(0);
  await dialog.locator('input[name="ref"]').fill("refs/tags/v1.2.0");
  await page.keyboard.press("Escape");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"这次构建要完成什么？",exact:true})).toBeFocused();
  await dialog.getByRole("radio",{name:"构建、测试并收集产物",exact:true}).check();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"需要哪些交付目标？",exact:true})).toBeFocused();
  await dialog.getByRole("radio",{name:"macOS · ARM64",exact:true}).check();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"确认后即可发起构建",exact:true})).toBeFocused();
  await expect(dialog.getByText("refs/tags/v1.2.0",{exact:true})).toBeVisible();
  expect(writes).toEqual([]);
  await dialog.getByRole("button",{name:"修改源码",exact:true}).click();
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/tags/v1.2.0");
  await dialog.locator('input[name="ref"]').fill("refs/heads/release/v1");
  await page.keyboard.press("Escape");
  await dialog.getByRole("button",{name:"保存并返回确认",exact:true}).click();
  await expect(dialog.getByText("refs/heads/release/v1",{exact:true})).toBeVisible();
  await dialog.getByRole("button",{name:"确认发起构建",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ref:"refs/heads/release/v1",goal:"构建本项目，运行相关测试，并收集所选平台的可分发产物。",targets:["darwin-arm64"],target_requirements:[{kind:"native",key:"darwin-arm64",os:"darwin",arch:"arm64",application_platform:"macos"}],parameters:{}});
});

async function openGuide(page:Page){
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  return page.getByRole("dialog");
}
async function review(page:Page){
  const dialog=await openGuide(page);
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await dialog.getByRole("radio",{name:"构建并收集产物",exact:true}).check();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  return dialog;
}

test("validation stays on the current question and checks UTF8 byte limits",async({page})=>{
  const events=await fixtureAPI(page);
  const dialog=await openGuide(page);
  const source=dialog.locator('input[name="ref"]');
  await source.fill("-invalid");await page.keyboard.press("Escape");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("源码引用");
  await expect(dialog.getByRole("heading",{name:"要构建哪份源码？",exact:true})).toBeVisible();
  await source.fill("refs/heads/main");await page.keyboard.press("Escape");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await dialog.locator('textarea[name="goal"]').fill("汉".repeat(5400));
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("16000 字节");
  await expect(dialog.locator('textarea[name="goal"]')).toHaveValue("汉".repeat(5400));
  expect(events.writes).toEqual([]);
});

test("target cards preserve custom keys and support native and image deliveries together",async({page})=>{
  await fixtureAPI(page);const writes:unknown[]=[];
  await page.route(`**/api/v1/releases/${release.id}/builds`,async route=>{
    if(route.request().method()==="POST"){writes.push(route.request().postDataJSON());return route.fulfill({json:build});}
    return route.fulfill({json:[]});
  });
  const dialog=await review(page);
  await dialog.getByRole("button",{name:"修改交付目标",exact:true}).click();
  const first=dialog.getByRole("region",{name:"目标 1",exact:true});
  await first.getByText("高级目标设置",{exact:true}).click();
  await first.getByRole("textbox",{name:"目标键",exact:true}).fill("desktop-app");
  await first.getByRole("radio",{name:"macOS · ARM64",exact:true}).check();
  await expect(first.getByRole("textbox",{name:"目标键",exact:true})).toHaveValue("desktop-app");
  await dialog.getByRole("button",{name:"添加目标",exact:true}).click();
  const second=dialog.getByRole("region",{name:"目标 2",exact:true});
  await second.getByRole("radio",{name:/^OCI 镜像/}).check();
  await second.getByRole("checkbox",{name:"linux/arm64",exact:true}).uncheck();
  await dialog.getByRole("button",{name:"保存并返回确认",exact:true}).click();
  await dialog.getByRole("button",{name:"确认发起构建",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({targets:["desktop-app","image"],target_requirements:[{kind:"native",key:"desktop-app",os:"darwin",arch:"arm64"},{kind:"image",key:"image",platforms:["linux/amd64"]}]});
});

test("pending confirmation blocks dismissal and failed requests preserve the draft for retry",async({page})=>{
  await fixtureAPI(page);const writes:unknown[]=[];
  let fail!:()=>void;const firstResponse=new Promise<void>(resolve=>{fail=resolve;});
  await page.route(`**/api/v1/releases/${release.id}/builds`,async route=>{
    if(route.request().method()!=="POST")return route.fulfill({json:[]});
    writes.push(route.request().postDataJSON());
    if(writes.length===1){await firstResponse;return route.fulfill({status:503,json:{error:"Build temporarily unavailable"}});}
    return route.fulfill({json:build});
  });
  const dialog=await review(page);
  await dialog.getByText("高级参数（可选）",{exact:true}).click();
  await dialog.locator('textarea[name="parameters"]').fill('{"PROFILE":"stable"}');
  await dialog.getByRole("button",{name:"确认发起构建",exact:true}).click();
  await expect.poll(()=>writes.length).toBe(1);
  await dialog.locator("form").evaluate(form=>{(form as HTMLFormElement).requestSubmit();(form as HTMLFormElement).requestSubmit();});
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();await expect(dialog.getByRole("button",{name:"关闭对话框",exact:true})).toBeDisabled();
  expect(writes).toHaveLength(1);fail();
  await expect(page.locator('[data-notify][data-kind="error"]')).toContainText("Build temporarily unavailable");
  await expect(dialog.locator('textarea[name="parameters"]')).toHaveValue('{"PROFILE":"stable"}');
  await dialog.getByRole("button",{name:"确认发起构建",exact:true}).click();
  await expect(page).toHaveURL(tenantURL(`/projects/${project.slug}/releases/${release.id}/builds/${build.number}`));
  expect(writes).toHaveLength(2);expect(writes[1]).toEqual(writes[0]);
});

test("Release polling keeps answers when publication makes creation unavailable",async({page})=>{
  await fixtureAPI(page);let saved=release;
  await page.route(`**/api/v1/releases/${release.id}`,route=>route.fulfill({json:saved}));
  const dialog=await review(page);
  saved={...release,status:"published"};
  await expect(dialog.getByRole("status")).toContainText("已不可构建",{timeout:10000});
  await expect(dialog.getByText("refs/heads/main",{exact:true})).toBeVisible();
  await expect(dialog.getByRole("button",{name:"确认发起构建",exact:true})).toBeDisabled();
});

test("discard protection retains answers and restores the launch button focus",async({page})=>{
  await fixtureAPI(page);const dialog=await openGuide(page);
  await dialog.locator('input[name="ref"]').fill("refs/tags/unsaved");await page.keyboard.press("Escape");
  page.once("dialog",prompt=>prompt.dismiss());
  await dialog.getByRole("button",{name:"取消",exact:true}).click();
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/tags/unsaved");
  page.once("dialog",prompt=>prompt.accept());
  await dialog.getByRole("button",{name:"关闭对话框",exact:true}).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("button",{name:"发起构建",exact:true})).toBeFocused();
});

test("step navigation and confirmation actions stay visible while target content scrolls",async({page},testInfo)=>{
  await fixtureAPI(page);const dialog=await review(page);
  await dialog.getByRole("button",{name:"修改交付目标",exact:true}).click();
  for(let index=0;index<4;index++)await dialog.getByRole("button",{name:"添加目标",exact:true}).click();
  await dialog.locator('[data-modal-scroll]').evaluate(element=>element.scrollTop=element.scrollHeight);
  await expect(dialog.getByRole("list",{name:"创建构建步骤",exact:true})).toBeInViewport();
  await expect(dialog.getByRole("button",{name:"保存并返回确认",exact:true})).toBeInViewport();
  expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1)).toBe(true);
  await page.screenshot({path:testInfo.outputPath("guided-build-targets.png"),fullPage:true});
});

for(const theme of ["light","dark"]){
  test(`English ${theme} guide has accessible steps and a responsive review`,async({page},testInfo)=>{
    test.setTimeout(120000);
    const events=await fixtureAPI(page);
    await page.context().addCookies([{name:"artifactory-locale",value:"en",url:"http://127.0.0.1"}]);
    await page.addInitScript(value=>localStorage.setItem("artifactory-theme",value),theme);
    await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
    await page.getByRole("button",{name:"Start build",exact:true}).click();
    const dialog=page.getByRole("dialog");
    await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
    for(let step=0;step<4;step++){
      if(step===1)await dialog.getByRole("radio",{name:"Build and collect artifacts",exact:true}).check();
      const result=await new AxeBuilder({page}).withTags(["wcag2a","wcag2aa","wcag21aa"]).analyze();
      expect(result.violations.map(item=>({id:item.id,nodes:item.nodes.map(node=>node.target)}))).toEqual([]);
      expect(await dialog.evaluate(element=>element.scrollWidth<=element.clientWidth+1)).toBe(true);
      if(step<3)await dialog.getByRole("button",{name:"Continue",exact:true}).click();
    }
    await expect(dialog.getByRole("button",{name:"Confirm and start build",exact:true})).toBeVisible();
    expect(events.writes).toEqual([]);
    await page.screenshot({path:testInfo.outputPath(`guided-build-review-${theme}.png`),fullPage:true});
  });
}
