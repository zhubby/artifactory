/**
 * [INPUT]: 依赖四步构建草稿、共享表单组件与本地授权平台。
 * [OUTPUT]: 提供 1 至 32 个独立目标的新增、删除与平台草稿。
 * [POS]: flow-wizard 的目标编辑层；请求字段与生产目标合同一致。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
export const presets=[
  {id:'macos-arm64',label:'macOS arm64',os:'darwin',arch:'arm64',app:'macos',device:'Mac Studio · 已授权 · 在线',authorized:true},
  {id:'macos-amd64',label:'macOS amd64',os:'darwin',arch:'amd64',app:'macos',device:'Mac mini · 已授权 · 离线仍保留排队',authorized:true},
  {id:'windows-amd64',label:'Windows amd64',os:'windows',arch:'amd64',app:'windows',device:'Windows Builder · 已授权 · 在线',authorized:true},
  {id:'linux-amd64',label:'Linux amd64',os:'linux',arch:'amd64',app:'linux',device:'Linux Runner · 已授权 · 在线',authorized:true},
  {id:'linux-arm64',label:'Linux arm64',os:'linux',arch:'arm64',app:'linux',device:'项目未授权对应系统与架构设备',authorized:false},
];
const nextKey=(base,targets)=>{let key=base,index=2;while(targets.some(target=>target.key===key))key=`${base}-${index++}`;return key;};
const native=(preset,key=preset.id)=>({preset:preset.id,kind:'native',key,os:preset.os,arch:preset.arch,application_platform:preset.app,artifact_key:key,format_suffix:'.tar.gz'});
const image=key=>({preset:'image',kind:'image',key,platforms:['linux/amd64','linux/arm64']});
export function initWizardTargets(wizard){
  if(!wizard.targetRequirements){
    wizard.targetRequirements=[];
    for(const label of wizard.targets||[]){
      const preset=presets.find(item=>item.authorized&&(item.id===label||item.label===label));
      if(preset)wizard.targetRequirements.push(native(preset,nextKey(preset.id,wizard.targetRequirements)));
      else if(label.startsWith('容器镜像')&&!wizard.targetRequirements.some(target=>target.kind==='image'))wizard.targetRequirements.push(image('image'));
    }
    if(!wizard.targetRequirements.length)wizard.targetRequirements.push(native(presets[0]));
  }
  wizard.targetSequence ||= 0;wizard.targetDrafts ||= {};
  for(const target of wizard.targetRequirements){
    if(target.uid===undefined)target.uid=++wizard.targetSequence;
    else wizard.targetSequence=Math.max(wizard.targetSequence,target.uid);
  }
  syncTargetLabels(wizard);
}
function syncTargetLabels(wizard){wizard.targets=wizard.targetRequirements.map(target=>`${target.key} · ${target.kind==='image'?'容器镜像':presets.find(item=>item.id===target.preset)?.label||`${target.os}/${target.arch}`}`);}
export function renderTargetPicker(wizard){
  initWizardTargets(wizard);
  const targets=wizard.targetRequirements;
  return `<div class="notice mb">${U.icon('monitor',19)}<p>原生目标要求项目授权设备；离线设备继续排队。可为同一平台创建不同逻辑 key。全部为本地演示。</p></div><div class="df-target-count">${targets.length} / 32 个目标</div><div class="df-target-rows">${targets.map((target,index)=>{
    const id=target.uid,preset=presets.find(item=>item.id===target.preset);
    return `<section class="df-target-row" data-wizard-row="${id}" aria-label="目标 ${index+1}"><header class="row between"><h4>目标 ${index+1} <code>${U.esc(target.key)}</code></h4><button type="button" class="btn ghost small" data-wizard-remove="${id}" ${targets.length===1?'disabled':''}>${U.icon('close',15)}删除目标</button></header><fieldset class="df-kind-options"><legend>目标类型</legend>${[['native','原生文件'],['image','容器镜像']].map(([kind,label])=>`<label class="df-platform-option"><input type="radio" name="target-kind-${id}" value="${kind}" data-wizard-kind="${id}" ${target.kind===kind?'checked':''}><span>${label}</span></label>`).join('')}</fieldset>
    ${target.kind==='native'?`<fieldset class="df-platform-options"><legend>已授权平台</legend>${presets.map(item=>`<label class="df-platform-option ${item.authorized?'':'df-target-disabled'}"><input type="radio" name="target-platform-${id}" value="${item.id}" data-wizard-platform="${id}" ${target.preset===item.id?'checked':''} ${item.authorized?'':'disabled'}><span><strong>${item.label}</strong><small>${item.device}</small></span></label>`).join('')}</fieldset>`:`<fieldset class="df-image-platforms"><legend>镜像架构 · 与原生设备独立</legend>${['linux/amd64','linux/arm64'].map(platform=>`<label><input type="checkbox" name="target-platforms-${id}" value="${platform}" ${target.platforms.includes(platform)?'checked':''}>${platform}</label>`).join('')}</fieldset>`}
    <details class="df-target-config" data-wizard-config="${id}" ${wizard.targetDetails?.[id]?'open':''}><summary>高级参数</summary><div class="grid-2">${U.field('逻辑 key',`<input class="input" name="target-key-${id}" maxlength="63" value="${U.esc(target.key)}" required>`)}${target.kind==='native'?U.field('宿主系统 / 架构',`<input class="input" value="${target.os} / ${target.arch}" readonly>`)+U.field('应用平台',U.select(`target-app-${id}`,target.application_platform||'',[{value:'',label:'非应用目标'},...(target.os==='darwin'?['macos','ios','android']:target.os==='linux'?['linux','android']:['windows']).map(value=>({value,label:value}))]))+U.field('制品 key',U.input(`target-artifact-${id}`,target.artifact_key||'',target.key))+U.field('文件后缀',U.input(`target-suffix-${id}`,target.format_suffix||'','.tar.gz')):''}</div></details></section>`;
  }).join('')}</div><button type="button" class="btn secondary" data-wizard-add ${targets.length>=32?'disabled':''}>${U.icon('plus',16)}新增目标</button>`;
}
export function captureWizardTargets(form,wizard){
  const data=new FormData(form);wizard.targetDetails ||= {};
  for(const target of wizard.targetRequirements){
    const id=target.uid,key=String(data.get(`target-key-${id}`)??target.key);
    if(key!==target.key)target.customKey=true;
    target.key=key;
    if(target.kind==='image')target.platforms=data.getAll(`target-platforms-${id}`).map(String);
    else {target.application_platform=data.get(`target-app-${id}`)||null;target.artifact_key=String(data.get(`target-artifact-${id}`)||'');target.format_suffix=String(data.get(`target-suffix-${id}`)||'');}
    wizard.targetDrafts[id] ||= {};wizard.targetDrafts[id][target.kind]={...target};
    wizard.targetDetails[id]=!!form.querySelector(`[data-wizard-config="${id}"]`)?.open;
  }
  syncTargetLabels(wizard);
}
export function bindWizardTargets(root,wizard,showWizard,capture){
  const redraw=(id,selector)=>{syncTargetLabels(wizard);showWizard();queueMicrotask(()=>root.querySelector(selector||`[data-wizard-row="${id}"] input`)?.focus({preventScroll:true}));};
  root.querySelector('[data-wizard-add]')?.addEventListener('click',()=>{capture();if(wizard.targetRequirements.length>=32)return;const target={...native(presets[0],nextKey(presets[0].id,wizard.targetRequirements)),uid:++wizard.targetSequence};wizard.targetRequirements.push(target);redraw(target.uid);root.querySelector(`[data-wizard-row="${target.uid}"]`)?.scrollIntoView({block:'nearest'});});
  root.querySelectorAll('[data-wizard-remove]').forEach(button=>button.addEventListener('click',()=>{capture();if(wizard.targetRequirements.length===1)return;const index=wizard.targetRequirements.findIndex(target=>target.uid===Number(button.dataset.wizardRemove));wizard.targetRequirements.splice(index,1);redraw(wizard.targetRequirements[Math.min(index,wizard.targetRequirements.length-1)].uid);}));
  root.querySelectorAll('[data-wizard-kind]').forEach(input=>input.addEventListener('change',()=>{capture();const id=Number(input.dataset.wizardKind),index=wizard.targetRequirements.findIndex(target=>target.uid===id),old=wizard.targetRequirements[index];if(old.kind===input.value)return;const saved=wizard.targetDrafts[id]?.[input.value];const base=input.value==='image'?'image':presets[0].id;const target=saved?{...saved}:{...(input.value==='image'?image(nextKey(base,wizard.targetRequirements)):native(presets[0],nextKey(base,wizard.targetRequirements))),uid:id};wizard.targetRequirements[index]=target;redraw(id,`[data-wizard-kind="${id}"][value="${input.value}"]`);}));
  root.querySelectorAll('[data-wizard-platform]').forEach(input=>input.addEventListener('change',()=>{capture();const id=Number(input.dataset.wizardPlatform),target=wizard.targetRequirements.find(item=>item.uid===id),preset=presets.find(item=>item.id===input.value&&item.authorized);if(!preset)return;const oldKey=target.key;Object.assign(target,{preset:preset.id,os:preset.os,arch:preset.arch,application_platform:preset.app});if(!target.customKey){target.key=nextKey(preset.id,wizard.targetRequirements.filter(item=>item.uid!==id));if(target.artifact_key===oldKey)target.artifact_key=target.key;}redraw(id,`[data-wizard-platform="${id}"][value="${preset.id}"]`);}));
}
