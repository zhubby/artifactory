/**
 * [INPUT]: 依赖项目设备平台接口和真实构建引导。
 * [OUTPUT]: 验证授权设备约束、在线默认、失败重试与镜像独立选择。
 * [POS]: 目标选择的设备边界行为回归。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { expect, test, type Page } from "@playwright/test";
import { fixtureAPI, project, release, tenantURL } from "./fixtures";

const platforms = [
  {os:"windows",arch:"amd64",device_count:1,online_count:0},
  {os:"darwin",arch:"arm64",device_count:1,online_count:1},
];
async function targets(page:Page) {
  await page.goto(tenantURL(`/projects/${project.slug}/releases/${release.id}`));
  await page.getByRole("button",{name:"发起构建",exact:true}).click();
  const dialog=page.getByRole("dialog");
  await expect(dialog.locator('input[name="ref"]')).toHaveValue("refs/heads/main");
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await dialog.getByRole("radio",{name:"构建并收集产物",exact:true}).check();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  return dialog;
}
test("native choices require an authorized matching device, including advanced controls",async({page})=>{
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/${project.id}/build-platforms`,route=>route.fulfill({json:platforms}));
  const dialog=await targets(page);
  await expect(dialog.getByRole("radio",{name:"macOS · ARM64",exact:true})).toBeChecked();
  await expect(dialog.getByRole("radio",{name:"Linux · x64",exact:true})).toBeDisabled();
  await expect(dialog.getByRole("radio",{name:"Linux · ARM64",exact:true})).toBeDisabled();
  await expect(dialog.getByRole("radio",{name:"macOS · Intel x64",exact:true})).toBeDisabled();
  await expect(dialog.getByRole("radio",{name:"Windows · x64",exact:true})).toBeEnabled();
  await dialog.getByText("高级目标设置",{exact:true}).click();
  await dialog.getByRole("combobox",{name:"构建宿主系统",exact:true}).click();
  await expect(page.getByRole("option",{name:"Linux",exact:true})).toBeDisabled();
  await page.getByRole("option",{name:"Windows",exact:true}).click();
  await dialog.getByRole("combobox",{name:"架构",exact:true}).click();
  await expect(page.getByRole("option",{name:"arm64",exact:true})).toBeDisabled();
  await page.keyboard.press("Escape");
  await dialog.getByRole("radio",{name:/^OCI 镜像/}).check();
  await expect(dialog.getByRole("checkbox",{name:"linux/amd64",exact:true})).toBeEnabled();
  await expect(dialog.getByRole("checkbox",{name:"linux/arm64",exact:true})).toBeEnabled();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"确认后即可发起构建",exact:true})).toBeVisible();
});

test("device query failures block native continuation and recover through retry",async({page})=>{
  const events=await fixtureAPI(page);let calls=0;
  await page.route(`**/api/v1/projects/${project.id}/build-platforms`,route=>++calls===1?route.fulfill({status:503,json:{error:"device query unavailable"}}):route.fulfill({json:platforms}));
  const dialog=await targets(page);
  await expect(dialog.getByRole("radio",{name:"Linux · x64",exact:true})).toBeDisabled();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"需要哪些交付目标？",exact:true})).toBeVisible();
  expect(events.writes).toEqual([]);
  await dialog.getByRole("button",{name:"重试",exact:true}).click();
  await expect(dialog.getByRole("radio",{name:"macOS · ARM64",exact:true})).toBeChecked();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"确认后即可发起构建",exact:true})).toBeVisible();
});

test("late device responses preserve an explicitly selected image target",async({page})=>{
  await fixtureAPI(page);let respond!:()=>void;
  const response=new Promise<void>(resolve=>{respond=resolve;});
  await page.route(`**/api/v1/projects/${project.id}/build-platforms`,async route=>{await response;return route.fulfill({json:platforms});});
  const dialog=await targets(page);
  await expect(dialog.getByRole("radio",{name:"macOS · ARM64",exact:true})).toBeDisabled();
  await dialog.getByRole("radio",{name:/^OCI 镜像/}).check();
  respond();
  await expect(dialog.getByText("正在读取项目设备",{exact:true})).toHaveCount(0);
  await expect(dialog.getByRole("radio",{name:/^OCI 镜像/})).toBeChecked();
  await dialog.getByRole("radio",{name:/^原生/}).check();
  await expect(dialog.getByRole("radio",{name:"macOS · ARM64",exact:true})).toBeChecked();
});

test("projects without native devices can select image platforms independently",async({page})=>{
  await fixtureAPI(page);
  await page.route(`**/api/v1/projects/${project.id}/build-platforms`,route=>route.fulfill({json:[]}));
  const dialog=await targets(page);
  await expect(dialog.getByRole("radio",{name:/^原生/})).toBeDisabled();
  await expect(dialog.getByRole("radio",{name:/^OCI 镜像/})).toBeChecked();
  await expect(dialog.getByRole("checkbox",{name:"linux/arm64",exact:true})).toBeEnabled();
  await dialog.getByRole("button",{name:"继续",exact:true}).click();
  await expect(dialog.getByRole("heading",{name:"确认后即可发起构建",exact:true})).toBeVisible();
});
