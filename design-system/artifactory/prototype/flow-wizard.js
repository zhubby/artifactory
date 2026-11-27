/**
 * [INPUT]: 依赖构建引导草稿、表单数据与独立目标编辑层。
 * [OUTPUT]: 导出四步内容、草稿捕获、合同校验与构建请求样例。
 * [POS]: app.js 弹窗的无状态适配；版本创建不复用源码和目标字段。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';

import {presets,initWizardTargets,renderTargetPicker,captureWizardTargets,bindWizardTargets} from './flow-targets.js';
const editable=(label,step)=>`<button type="button" class="icon-button" data-wizard-edit="${step}" aria-label="编辑${label}">${U.icon('file',16)}</button>`;
export function renderBuildWizardBody(wizard) {
  initWizardTargets(wizard);
  const s=wizard.step||1;
  let body=`<div class="wizard-steps" role="list" aria-label="构建四步引导">${['选择源码','构建要求','交付目标','确认发起'].map((label,index)=>`<div role="listitem" class="wizard-step ${s===index+1?'current':s>index+1?'complete':''}" ${s===index+1?'aria-current="step"':''}><span>${s>index+1?U.icon('check',12):index+1}</span>${label}</div>`).join('')}</div><form id="wizard-form" data-form="wizard"><div id="wizard-error" role="alert"></div>`;
  if(s===1)body+=`<h3 class="wizard-question" tabindex="-1">从哪一份源码开始？</h3><p class="wizard-desc">本次构建独立解析 Git 引用，固定提交、子模块和 LFS 源码清单。</p><div class="wizard-summary mb">${U.kv('项目',U.esc(wizard.project))}${U.kv('版本',`${U.esc(wizard.version)} · 只读`)}</div>${U.field('Git 仓库',`<input class="input mono" value="${U.esc(wizard.repository||'https://git.example/orbital/desktop.git')}" readonly>`)}${U.field('分支、标签或提交',`<input class="input" name="ref" value="${U.esc(wizard.ref||'')}" placeholder="refs/heads/main" maxlength="255" required>`,`凭据使用 ${U.esc(wizard.credential||'git/orbital-readonly')}，不放入仓库 URL。`)}<div class="tag-list">${['main','release/2.4','v2.3.9'].map(value=>U.btn(value,'choose-ref','small','branch',`data-value="${value}"`)).join('')}</div>`;
  if(s===2)body+=`<h3 class="wizard-question" tabindex="-1">这次，你想交付什么？</h3><p class="wizard-desc">构建要求面向本次执行。版本说明面向读者。</p><div class="row wrap df-goal-templates">${[['Build','构建并归档'],['Test','运行测试'],['Custom','自定义']].map(([id,label])=>`<button type="button" class="btn secondary" data-wizard-template="${id}">${label}</button>`).join('')}</div>${U.field('构建要求',`<textarea name="requirements" rows="6" maxlength="16000" required placeholder="描述要构建的应用、验证过程和交付结果。">${U.esc(wizard.requirements||'')}</textarea>`,'AI 只读分析固定源码，执行命令与证据保存在平台。')}`;
  if(s===3)body+=`<h3 class="wizard-question" tabindex="-1">让产品抵达哪些平台？</h3><p class="wizard-desc">原生目标与镜像架构独立选择。逻辑 key 在本次构建内唯一。</p>${renderTargetPicker(wizard)}`;
  if(s===4)body+=`<h3 class="wizard-question" tabindex="-1">一切就绪，开始构建。</h3><p class="wizard-desc">确认项目、版本和本次源码、要求、目标。可返回修改并继续确认。</p><div class="wizard-summary">${U.kv('项目 / Release',`${U.esc(wizard.project)} · ${U.esc(wizard.version)}`)}${U.kv('源码',`<code>${U.esc(wizard.ref)}</code>${editable('源码',1)}`)}${U.kv('构建要求',`<span>${U.esc(wizard.requirements)}</span>${editable('构建要求',2)}`)}${U.kv('交付目标',`<span>${(wizard.targets||[]).map(value=>`<span class="chip">${U.esc(value)}</span>`).join(' ')}</span>${editable('交付目标',3)}`)}</div><details class="mt df-wizard-parameters"><summary>高级构建参数</summary>${U.field('参数 JSON',`<textarea name="parameters" rows="5" spellcheck="false">${U.esc(wizard.parameters||'{}')}</textarea>`,'仅接受字符串键值。ARTIFACTORY_RELEASE_ 为保留前缀。')}</details><div class="notice mt">${U.icon('eye',19)}<p>确认后仅在浏览器保存本地构建快照。原型不会执行构建或调用真实服务。</p></div>`;
  return body+'</form>';
}
export function captureBuildWizard(form,wizard) {
  if(!form)return;
  const data=new FormData(form),step=wizard.step;
  if(step===1)wizard.ref=String(data.get('ref')||'');
  if(step===2)wizard.requirements=String(data.get('requirements')||'');
  if(step===3)captureWizardTargets(form,wizard);
  if(step===4)wizard.parameters=String(data.get('parameters')||'{}');
  wizard.dirty=true;
}
export function validateBuildWizard(wizard,all=false) {
  const steps=all?[1,2,3,4]:[wizard.step],bytes=value=>new TextEncoder().encode(value).length,safeKey=/^[a-z0-9]+(?:-[a-z0-9]+)*$/;
  if(steps.includes(1)&&(!wizard.ref||wizard.ref!==wizard.ref.trim()||wizard.ref.startsWith('-')||/[\0\r\n]/.test(wizard.ref)||bytes(wizard.ref)>255))return '请输入有效的 Git 引用，长度不超过 255 字节。';
  if(steps.includes(2)&&(!wizard.requirements?.trim()||bytes(wizard.requirements)>16000))return '请填写构建要求，长度不超过 16000 字节。';
  if(steps.includes(3)){
    const targets=wizard.targetRequirements||[];
    if(!targets.length||targets.length>32||new Set(targets.map(target=>target.key)).size!==targets.length)return '请选择 1 至 32 个目标，并为每个目标使用不同的逻辑 key。';
    for(const target of targets){
      if(!safeKey.test(target.key)||target.key.length>63)return '目标 key 只接受小写字母、数字和连字符，长度不超过 63。';
      if(target.kind==='image'){if(!target.platforms.length)return '容器镜像至少选择一个架构。';continue;}
      if(!presets.some(preset=>preset.id===target.preset&&preset.authorized&&preset.os===target.os&&preset.arch===target.arch))return '项目尚未授权此原生平台的对应设备。';
      const app=target.application_platform;
      if(app&&((app==='macos'||app==='ios')?target.os!=='darwin':app==='android'?target.os==='windows':app!==target.os))return '应用平台与已授权的宿主系统不匹配。';
      if(target.artifact_key&&(!safeKey.test(target.artifact_key)||target.artifact_key.length>63))return '制品 key 必须使用有效的小写逻辑标识。';
      if(target.format_suffix&&(target.format_suffix.length>32||!/^\.[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*$/.test(target.format_suffix)))return '文件后缀应采用 .zip 或 .tar.gz 格式。';
    }
  }
  if(steps.includes(4)){
    let value;try{value=JSON.parse(wizard.parameters||'{}');}catch{return '高级参数必须是有效 JSON。';}
    if(!value||typeof value!=='object'||Array.isArray(value)||bytes(JSON.stringify(value))>16384||Object.entries(value).some(([key,item])=>key.startsWith('ARTIFACTORY_RELEASE_')||typeof item!=='string'))return '高级参数只接受字符串键值，不能使用 ARTIFACTORY_RELEASE_ 保留前缀。';
  }
  return '';
}
export function buildWizardPayload(wizard) {
  const target_requirements=(wizard.targetRequirements||[]).map(({preset,uid,customKey,...target})=>target);
  return {ref:wizard.ref,goal:wizard.requirements,target_requirements,targets:target_requirements.map(target=>target.key),parameters:JSON.parse(wizard.parameters||'{}')};
}
export function bindBuildWizard(root,wizard,showWizard) {
  bindWizardTargets(root,wizard,showWizard,()=>captureBuildWizard(root.querySelector('#wizard-form'),wizard));
  root.querySelector('#wizard-form')?.addEventListener('input',()=>{const error=root.querySelector('#wizard-error');if(error)error.textContent='';});
  root.querySelectorAll('[data-wizard-edit]').forEach(button=>button.addEventListener('click',()=>{captureBuildWizard(root.querySelector('#wizard-form'),wizard);wizard.returnToReview=true;wizard.step=Number(button.dataset.wizardEdit);showWizard();}));
  root.querySelectorAll('[data-wizard-template]').forEach(button=>button.addEventListener('click',()=>{wizard.requirements=button.dataset.wizardTemplate==='Test'?'运行项目测试并保留测试结果。':button.dataset.wizardTemplate==='Build'?'构建所选平台，校验并归档交付文件。':'';wizard.dirty=true;showWizard();root.querySelector('textarea')?.focus();}));
}
