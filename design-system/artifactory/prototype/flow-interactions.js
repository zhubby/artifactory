/**
 * [INPUT]: 依赖构建、审计筛选、发布状态、页面重绘与本地通知。
 * [OUTPUT]: 导出交付动作绑定，保留证据、草稿、焦点和滚动。
 * [POS]: 交付领域本地控制器；不发送网络请求，不更改真实产品。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { buildView, logView } from './flow-build.js';
import { releaseView } from './flow-release.js';
import { startDistribution } from './flow-distribution.js';
import { applyAuditFilter, auditRawText, auditReading } from './flow-audit.js';

const attached = new WeakMap();
function currentView(root,state) {
  const owner=root.querySelector('[data-flow-view]');
  if(!owner)return null;
  const key=owner.dataset.flowStateKey;
  return owner.dataset.flowView==='build' ? state.flowBuildViews?.[key] || buildView(state) : state.flowReleaseViews?.[key] || releaseView(state);
}
export function captureFlowState(root,state) {
  const view=currentView(root,state);
  if(!view)return;
  root.querySelectorAll('[data-flow-scroll]').forEach(node=>{view.scroll[node.dataset.flowScroll]={top:node.scrollTop,left:node.scrollLeft,height:node.scrollHeight};});
  root.querySelectorAll('[data-flow-detail]').forEach(node=>{view.details[node.dataset.flowDetail]=node.open;});
  root.querySelectorAll('[data-flow-draft]').forEach(node=>{view[node.dataset.flowDraft]=node.value;});
}
function restore(root,state) {
  const view=currentView(root,state);
  if(!view)return;
  root.querySelectorAll('[data-flow-scroll]').forEach(node=>{
    const saved=view.scroll[node.dataset.flowScroll];
    if(saved){node.scrollTop=saved.top;node.scrollLeft=saved.left;}
  });
  root.querySelectorAll('[data-flow-detail]').forEach(node=>{if(Object.hasOwn(view.details,node.dataset.flowDetail))node.open=view.details[node.dataset.flowDetail];});
}
export function bindFlowInteractions(root,state,hooks={}) {
  attached.get(root)?.abort();
  const controller=new AbortController(); attached.set(root,controller);
  const {signal}=controller;
  restore(root,state);
  const notify=(message,error=false)=>hooks.notify?.(message,error);
  const redraw=(el,anchor)=>{
    const action=el?.dataset.flowAction,value=el?.dataset.flowValue;
    captureFlowState(root,state); hooks.persist?.(); hooks.render?.();
    requestAnimationFrame(()=>{
      restore(root,state);
      if(['logs-append','logs-follow'].includes(action)&&logView(buildView(state)).follow){const reader=root.querySelector('.df-log-reader');if(reader)reader.scrollTop=reader.scrollHeight;}
      if(anchor){const node=root.querySelector(`[data-flow-scroll="${anchor.key}"]`);if(node)node.scrollTop=anchor.top+node.scrollHeight-anchor.height;}
      const controls=Array.from(root.querySelectorAll('[data-flow-action]'));
      const target=controls.find(node=>node.dataset.flowAction===action&&node.dataset.flowValue===value);
      if(el?.matches('[data-flow-candidate]'))root.querySelector(`[data-flow-candidate][value="${el.value}"]`)?.focus({preventScroll:true});
      else if(target)target.focus({preventScroll:true});
      else if(action==='publish-open')root.querySelector('[data-flow-publish-ack]')?.focus({preventScroll:true});
      else if(action==='publish-close')root.querySelector('[data-flow-action="publish-open"]')?.focus({preventScroll:true});
      else if(action==='publish-confirm'||action==='publish-outcome')root.querySelector('[data-flow-action="publish-outcome"],[data-flow-action="release-tab"][aria-selected="true"]')?.focus({preventScroll:true});
      else if(action?.startsWith('logs-'))root.querySelector('[data-flow-action="logs-follow"]')?.focus({preventScroll:true});
      else if(action==='audit-retry')root.querySelector('[data-flow-action="audit-round"][aria-pressed="true"]')?.focus({preventScroll:true});
      else if(action?.startsWith('external-'))root.querySelector('[data-flow-action="external-reset"],[data-flow-action="external-retry"],[data-flow-action="external-outcome"],[data-flow-action="external-start"]')?.focus({preventScroll:true});
    });
  };
  root.addEventListener('input',event=>{
    if(event.target.matches('[data-audit-search]')){
      const view=currentView(root,state);if(!view)return;
      auditReading(view).reading.query=event.target.value;
      applyAuditFilter(root,view);hooks.persist?.();return;
    }
    const el=event.target.closest('[data-flow-draft]');if(!el)return;
    const view=currentView(root,state);if(view)view[el.dataset.flowDraft]=el.value;hooks.persist?.();
  },{signal});
  root.addEventListener('change',event=>{
    const el=event.target;
    if(el.matches('[data-flow-candidate]')){releaseView(state).selected=el.value;releaseView(state).error='';redraw(el);}
    if(el.matches('[data-flow-publish-ack]')){releaseView(state).acknowledged=el.checked;hooks.persist?.();}
    if(el.matches('[data-flow-external-credential]')){releaseView(state).externalCredential=el.value;hooks.persist?.();}
  },{signal});
  root.addEventListener('click',event=>{
    const el=event.target.closest('[data-flow-action]');if(!el)return;
    event.preventDefault();event.stopPropagation();
    const name=el.dataset.flowAction,value=el.dataset.flowValue||'',view=buildView(state),release=releaseView(state);
    let anchor;
    if(name==='build-tab'){view.tab=value;state.buildTab=value;}
    else if(name==='process-stage')view.stage=view.stage===value?'':value;
    else if(name==='execution-target')view.target=value;
    else if(name==='execution-attempt')view.attempt=value;
    else if(name==='execution-step')view.step=value;
    else if(name==='logs-follow')logView(view).follow=!logView(view).follow;
    else if(name==='logs-scenario'){const log=logView(view);if(value==='error')log.error=true;else log.connection=value;}
    else if(name==='logs-retry'){const log=logView(view);log.error=false;log.connection='connected';notify('本地日志连接已恢复；未请求真实执行器。');}
    else if(name==='logs-append'){
      const log=logView(view);
      if(log.connection!=='connected'||log.error){notify('请先恢复本地连接再新增演示日志。');return;}
      log.tick++;
    }
    else if(name==='logs-older'){
      if(logView(view).error){notify('请先重试恢复日志加载。');return;}
      const node=root.querySelector('.df-log-reader');
      if(node)anchor={key:node.dataset.flowScroll,top:node.scrollTop,height:node.scrollHeight};
      const key=`${view.target}/${view.attempt}/${view.step}`;
      view.logPagesByKey[key]=(view.logPagesByKey[key]||1)+1;
    }
    else if(name==='plan-revision')view.revision=value;
    else if(name==='plan-target')view.planTarget=value;
    else if(name==='build-scenario'){view.scenario=value;if(value==='needs_input')view.tab='plan';}
    else if(name==='clarification-save'){
      if(view.scenario!=='needs_input')return;
      const draft=root.querySelector('[data-flow-draft="answer"]');
      view.answer=draft?.value||view.answer;
      if(!view.answer.trim()){draft?.setCustomValidity('请填写澄清回答。');draft?.reportValidity();draft?.addEventListener('input',()=>draft.setCustomValidity(''),{once:true});return;}
      if(view.answer.length>8000){draft?.setCustomValidity('回答不能超过 8000 个字符。');draft?.reportValidity();draft?.addEventListener('input',()=>draft.setCustomValidity(''),{once:true});return;}
      view.answerSubmitted=true;view.scenario='running';notify('演示回答已保存在本地；未提交真实规划。');
    }
    else if(name==='audit-round')view.auditRound=value;
    else if(name==='audit-entry'){const {reading}=auditReading(view);reading.entry=value;reading.tab='result';}
    else if(name==='audit-tab')auditReading(view).reading.tab=value;
    else if(name==='audit-filter'){
      auditReading(view).reading.filter=value;applyAuditFilter(root,view);hooks.persist?.();
      const menu=el.closest('.select'),summary=menu.querySelector('summary');
      summary.querySelector('span').textContent=el.textContent.trim();
      summary.setAttribute('aria-label',`审计动作筛选：${el.textContent.trim()}`);
      menu.querySelectorAll('[role=option]').forEach(option=>option.setAttribute('aria-selected',String(option===el)));
      menu.open=false;summary.focus({preventScroll:true});return;
    }
    else if(name==='audit-copy'){
      const raw=auditRawText(view);
      (async()=>{try{await navigator.clipboard.writeText(raw);notify('已复制当前原始演示记录。');}catch{notify('无法复制记录。请切换到原始记录后选择文本复制。',true);}})();return;
    }
    else if(name==='audit-detail-jump'||name==='audit-feed-jump'){
      const target=root.querySelector(name==='audit-detail-jump'?'.df-audit-evidence':'.df-audit-feed');
      target?.scrollIntoView({block:'start',behavior:'instant'});target?.focus({preventScroll:true});return;
    }
    else if(name==='audit-more'){if(view.auditScenario==='error'){notify('请先重试恢复审计读取。',true);return;}auditReading(view).reading.page++;}
    else if(name==='audit-scenario')view.auditScenario=value;
    else if(name==='audit-retry'){view.auditScenario='normal';notify('已恢复本地审计样例；未调用模型。');}
    else if(name==='release-tab')release.tab=value;
    else if(name==='external-start'){
      captureFlowState(root,state);startDistribution(release);
    }
    else if(name==='external-outcome'&&release.status==='published'&&release.externalStatus==='publishing'){
      release.externalStatus=value;release.externalError=value==='failed'?'本地外仓连接失败，正式交付保持已发布。':'';
    }
    else if(name==='external-retry'&&release.status==='published'&&release.externalFrozen){release.externalStatus='publishing';release.externalError='';}
    else if(name==='external-reset'&&release.status==='published'){release.externalStatus='idle';release.externalError='';release.externalFrozen=null;}
    else if(name==='candidate-toggle'){
      release.invalidated=!release.invalidated;
      if(release.invalidated){release.selected='';release.error='当前候选已失效。请重新选择有效的完整成功构建。';}
      else release.error='';
    }
    else if(name==='publish-open'){
      if(release.status==='draft'&&(!release.selected||release.invalidated)){release.error='请选择一个当前有效的完整成功构建。';}
      else {release.confirmation=true;release.acknowledged=false;release.error='';}
    }
    else if(name==='publish-close'){release.confirmation=false;release.error='';}
    else if(name==='publish-confirm'){
      if(!release.acknowledged){release.error='请先确认正式版本不可覆盖。';}
      else if(release.status==='draft'&&(!release.selected||release.invalidated)){release.selected='';release.confirmation=false;release.error='候选在确认前失效。请重新选择。';}
      else {release.frozen=release.status==='publish_failed'?release.frozen:release.selected;release.status='publishing';release.confirmation=false;release.error='';notify('已进入本地发布中演示。');}
    }
    else if(name==='publish-outcome'){
      release.status=value;release.error=value==='publish_failed'?'镜像推广失败：示例仓库暂不可达。重试会继续使用构建 #'+release.frozen+'。':'';
      if(value==='published')state.published=true;
    }
    else if(name==='release-scenario'){
      release.status=value;release.frozen=value==='draft'?'':'1047';release.selected='';release.confirmation=false;release.error=value==='publish_failed'?'本地失败样例：推广阶段连接中断。':'';state.published=value==='published';
    }
    else return;
    redraw(el,anchor);
  },{signal});
  root.addEventListener('keydown',event=>{
    const el=event.target.closest('[role="tab"][data-flow-action]');
    if(!el||!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;
    const tabs=Array.from(el.closest('[role="tablist"]').querySelectorAll('[role="tab"]'));
    const index=tabs.indexOf(el),next=event.key==='Home'?0:event.key==='End'?tabs.length-1:(index+(event.key==='ArrowRight'?1:-1)+tabs.length)%tabs.length;
    event.preventDefault();event.stopPropagation();tabs[next].click();
  },{signal});
}
