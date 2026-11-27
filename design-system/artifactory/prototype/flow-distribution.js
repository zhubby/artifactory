/**
 * [INPUT]: 依赖已发布 Release、冻结镜像身份与本地外仓凭据样例。
 * [OUTPUT]: 导出外仓分发配置、进行中、失败重试和成功记录。
 * [POS]: 正式镜像交付的本地演示；不分发候选，不连接外部仓库。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import * as U from './ui.js';
const button=(label,name,value='',kind='secondary')=>U.btn(label,'',kind,'',`data-flow-action="${name}" data-flow-value="${value}"`);
export const distributionCredentials=[{id:'ghcr-demo',name:'GHCR 发布凭据',host:'ghcr.io',enabled:true},{id:'harbor-demo',name:'Harbor 团队凭据',host:'harbor.example',enabled:true},{id:'disabled-demo',name:'旧发布凭据 · 已停用',host:'registry.example',enabled:false}];
export function renderDistribution(view){
  if(view.status!=='published')return '';
  const status=view.externalStatus||'idle',frozen=view.externalFrozen,locked=['publishing','failed','succeeded'].includes(status);
  return U.panel(U.sectionTitle('分发正式镜像',U.badge('仅本地演示'))+`<p class="muted small">只分发已发布镜像。标签固定为 2.4.0，根摘要与构建 #1047 一致。不会导入或显示真实凭据。</p>
    ${locked?`<div class="callout"><strong>${U.esc(frozen?.host||'ghcr.io')}/${U.esc(frozen?.repository||'orbital/sync-service')}:2.4.0</strong><p>sha256:84fb26c7…a102e9d4 · ${U.esc(frozen?.credentialName||'GHCR 发布凭据')}</p></div>`:`<fieldset class="df-distribution-credentials"><legend>项目 registry 凭据</legend>${distributionCredentials.map(item=>`<label class="df-platform-option ${item.enabled?'':'df-target-disabled'}"><input type="radio" name="external-credential" value="${item.id}" data-flow-external-credential ${view.externalCredential===item.id?'checked':''} ${item.enabled?'':'disabled'}><span><strong>${item.name}</strong><small>${item.host} · ${item.enabled?'已启用':'不可选择'}</small></span></label>`).join('')}</fieldset>${U.field('仓库路径',`<input class="input mono" data-flow-draft="externalRepository" value="${U.esc(view.externalRepository||'')}" placeholder="team/application">`,'只填写仓库路径。固定标签由 Release 版本生成。')}${U.field('固定版本标签','<input class="input mono" value="2.4.0" readonly>')}`}
    ${view.externalError?`<p class="df-inline-error" role="alert">${U.esc(view.externalError)}</p>`:''}
    ${status==='publishing'?`<p class="notice" role="status">本地分发进行中：读取固定摘要 → 推送清单 → 校验版本标签。</p><div class="row wrap">${button('模拟分发成功','external-outcome','succeeded')}${button('模拟分发失败','external-outcome','failed','ghost')}</div>`:status==='failed'?`<p class="soft-note">分发失败样例：外仓连接中断。重试保留同一凭据、路径、标签和摘要。</p>${button('重试同一分发','external-retry','','primary')}${button('重新配置','external-reset','','ghost')}`:status==='succeeded'?`<p class="callout success" role="status">正式镜像分发成功样例。远端摘要与冻结摘要一致。</p>${button('创建另一条分发','external-reset','','secondary')}`:button('确认分发正式镜像','external-start','','primary')}
    <p class="df-caption">所有状态均在当前浏览器演示。未发送网络请求。</p>`);
}
export function startDistribution(view){
  if(view.status!=='published')return false;
  const credential=distributionCredentials.find(item=>item.id===view.externalCredential&&item.enabled),repository=(view.externalRepository||'').trim();
  if(!credential){view.externalError='请选择启用的项目 registry 凭据。';return false;}
  if(repository.length>255||!/^([a-z0-9]+(?:[._-][a-z0-9]+)*\/)*[a-z0-9]+(?:[._-][a-z0-9]+)*$/.test(repository)){view.externalError='请输入有效的小写仓库路径，例如 team/application，不含标签或协议。';return false;}
  view.externalFrozen={credential:credential.id,credentialName:credential.name,host:credential.host,repository,tag:'2.4.0',digest:'sha256:84fb26c7…a102e9d4',build:'1047'};view.externalStatus='publishing';view.externalError='';return true;
}
