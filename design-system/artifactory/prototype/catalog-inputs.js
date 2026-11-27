/**
 * [INPUT]: 依赖 ui.js 的图标与文本转义，以及浏览器表单接口。
 * [OUTPUT]: 导出四组输入组件目录与 mountInputs 局部交互入口。
 * [POS]: 组件展厅的操作、表单、选择与文件示例；不修改产品状态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, esc } from './ui.js';
const B = (text, act, extra = '', kind = '') => `<button type="button" class="btn ${kind || 'secondary'} cn-button" data-ci-act="${act}" ${extra}>${text}</button>`;
const wrap = content => `<div class="cn-demo" data-ci-demo>${content}<p class="cn-status" role="status" aria-live="polite"></p></div>`;
const input = (label, value = '', attrs = '') => `<label class="cn-label">${label}<input class="input" value="${value}" ${attrs}></label>`;
const row = content => `<div class="cn-row">${content}</div>`;
const options = (names, act = 'pick') => names.map(n => B(esc(n), act, `data-ci-value="${esc(n)}"`)).join('');
const menu = (label, content, cls = '') => `<details class="cn-popover ${cls}"><summary aria-expanded="false">${label}${icon('down', 15)}</summary><div class="cn-menu">${content}</div></details>`;
const check = (label, attrs = '', type = 'checkbox') => `<label class="cn-check"><input type="${type}" ${attrs}><span>${label}</span></label>`;
const item = (group, id, name, en, description, content, tags = []) => ({ id: `${group}-${id}`, name, en, description, tags: [group, en, name, ...tags], render: () => wrap(typeof content === 'function' ? content() : content) });
const days = (mode = 'date', selected = 18) => `<div class="cn-calendar" data-ci-calendar="${mode}"><div class="cn-calendar-head">${B(icon('arrowLeft', 15),'month','aria-label="上个月" data-ci-step="-1"')}<strong data-ci-month>2026 年 11 月</strong>${B(icon('chevron', 15),'month','aria-label="下个月" data-ci-step="1"')}</div><div class="cn-week">${['一','二','三','四','五','六','日'].map(t=>`<span>${t}</span>`).join('')}</div><div class="cn-days" data-ci-days>${Array.from({length:6},()=>'<span></span>').join('')}${Array.from({length:30},(_,i)=>`<button type="button" data-ci-act="day" data-ci-day="${i+1}" aria-label="2026 年 11 月 ${i+1} 日" aria-pressed="${i+1===selected}" class="${i+1===selected?'is-picked':''}">${i+1}</button>`).join('')}</div></div>`;
const fileRow = (name, size = '4.8 MB') => `<div class="cn-file"><span class="cn-file-icon">${icon('file',19)}</span><span><strong>${esc(name)}</strong><small>${size}</small></span>${B(icon('close',15),'remove-file',`aria-label="移除 ${esc(name)}"`)}</div>`;
const upload = (drop = false) => `<label class="cn-upload ${drop?'cn-dropzone':''}" ${drop?'data-ci-drop':''}><span class="cn-upload-emblem">${icon('upload',26)}</span><strong>${drop?'拖放文件，或点击选择':'选择演示文件'}</strong><small>仅展示名称，不上传或读取文件内容</small><input type="file" multiple data-ci-files aria-label="选择演示文件"></label><div class="cn-files" data-ci-file-list></div>`;

export const inputGroups = [
 {id:'actions',label:'操作与快捷',description:'从一次按压到连续批量操作。',items:[
  item('actions','button','按钮','Button','主次操作、危险与禁用状态采用一致高度。',row(B(`${icon('play',16)}发起构建`,'say','data-ci-value="演示构建已加入队列"','primary')+B('保存草稿','say','data-ci-value="演示草稿已保存"')+B('删除','say','data-ci-value="危险操作需要单独确认"','danger')+'<button type="button" class="btn" disabled>不可用</button>')),
  item('actions','icon-button','图标按钮','Icon button','图标操作始终带名称和按下状态。',row(B(icon('star',20),'toggle','aria-label="关注项目" aria-pressed="false"')+B(icon('bell',20),'toggle','aria-label="订阅通知" aria-pressed="true"')+B(icon('download',20),'say','aria-label="下载演示清单" data-ci-value="演示清单已准备"'))),
  item('actions','split-button','分裂按钮','Split button','主操作与备选动作保持同一操作分组。',`<div class="cn-split">${B(`${icon('play',16)}立即构建`,'say','data-ci-value="已选择立即构建"','primary')}${menu('<span class="sr-only">其他构建动作</span>',options(['仅生成方案','定时构建','另存模板']))}</div>`),
  item('actions','button-group','按钮组','Button group','互斥操作保持稳定位置和选择状态。',`<div class="cn-segment" role="group" aria-label="视图模式">${['卡片','列表','紧凑'].map((v,i)=>B(v,'segment',`aria-pressed="${!i}"`)).join('')}</div>`),
  item('actions','toggle-button','切换按钮','Toggle button','二态动作使用 aria-pressed 表达。',B(`${icon('heart',17)}关注项目`,'toggle','aria-pressed="false" data-ci-toggle-label="已关注项目|关注项目"')),
  item('actions','link','文本链接','Link','正文链接在悬停与焦点时显示下划线。',`<div class="cn-link-stack"><a href="#component-actions-link" data-ci-act="link">查看构建证据 ${icon('arrow',14)}</a><a href="#component-actions-link" data-ci-act="link">打开交付文档 ${icon('external',14)}</a></div>`),
  item('actions','fab','浮动操作','Floating action','单一新增入口使用突出的圆形操作钮。',`<div class="cn-fab-stage"><span class="cn-fab-copy"><small>WORKSPACE</small><span>当前工作空间</span><strong><b data-ci-counter>3</b> 个项目</strong></span>${B(icon('plus',24),'add-project','aria-label="新增演示项目"')}</div>`),
  item('actions','copy-button','复制按钮','Copy button','复制结果有文字反馈并保留原始内容。',`<div class="cn-copy"><span class="cn-copy-content"><small>制品摘要 · SHA-256</small><code>sha256:84fb26c7…a102e9d4</code></span>${B(icon('copy',17),'copy','aria-label="复制演示摘要" data-ci-copy="sha256:84fb26c7-demo-a102e9d4"')}</div>`),
  item('actions','loading-button','加载按钮','Loading button','操作执行时禁用重复触发，并说明结果。',B('检查连接','loading','data-ci-loading-label="正在检查…"')),
  item('actions','keyboard-shortcut','快捷键','Keyboard shortcut','按钮与键盘路径执行同一动作。',`<div data-ci-shortcut tabindex="0" class="cn-shortcut"><span>聚焦此区域后按</span><kbd>Alt</kbd><kbd>B</kbd>${B('运行快捷操作','say','data-ci-value="快捷操作已执行"')}</div>`),
 ]},
 {id:'forms',label:'文本与表单',description:'让输入、验证和修正都有清晰边界。',items:[
  item('forms','input','文本输入','Input','标签、提示和只读状态明确区分。',input('项目名称','Orbital Desktop','placeholder="输入项目名称"')+input('固定项目标识','orbital-desktop','readonly')),
  item('forms','textarea','多行输入','Textarea','长文本有字符计数和稳定的阅读宽度。',`<label class="cn-label">构建要求<textarea rows="3" maxlength="240" data-ci-count>构建签名安装包，保留完整交付证据。</textarea><small><output data-ci-count-value>20</output> / 240 字</small></label>`),
  item('forms','password','密码输入','Password input','显隐按钮不清空内容或改变焦点顺序。',`<label class="cn-label">演示密码<div class="cn-input-action"><input class="input" type="password" value="prism-demo-only" autocomplete="off" data-ci-password>${B(icon('eye',17),'password','aria-label="显示演示密码" aria-pressed="false"')}</div></label><small class="cn-help">请使用虚构内容。</small>`),
  item('forms','number','步进数字','Number input','范围限制与增减操作共同表达边界。',`<label class="cn-label">并发任务数<div class="cn-stepper">${B('−','step','data-ci-step="-1" aria-label="减少并发任务"')}<input type="number" value="4" min="1" max="16" aria-label="并发任务数">${B('+','step','data-ci-step="1" aria-label="增加并发任务"')}</div><small>1–16 个任务</small></label>`),
  item('forms','search','即时搜索','Search input','搜索保留查询，并展示匹配与空结果。',`<label class="cn-label">查找项目<input class="input" type="search" data-ci-filter placeholder="输入 Orbital 或 Atlas"></label><ul class="cn-results" data-ci-filter-list>${['Orbital Desktop','Atlas API','Beacon Mobile'].map(n=>`<li>${n}</li>`).join('')}</ul><p class="cn-help" data-ci-empty hidden>没有匹配结果。</p>`),
  item('forms','otp','验证码输入','OTP input','连续输入、粘贴和退格保持自然顺序。',`<fieldset class="cn-fieldset"><legend>六位演示验证码</legend><div class="cn-otp" data-ci-otp>${Array.from({length:6},(_,i)=>`<input class="input" type="text" inputmode="numeric" maxlength="1" aria-label="第 ${i+1} 位验证码" autocomplete="off">`).join('')}</div><small>输入任意六位数字以体验。</small></fieldset>`),
  item('forms','unit-input','单位输入','Unit input','数值与单位组成单一输入语义。',`<div class="cn-row cn-align-end">${input('构建超时','30','type="number" min="1" max="3600"')}${menu('分钟',options(['秒','分钟','小时']))}</div>`),
  item('forms','validation','字段验证','Field validation','离开字段后说明错误和修正方法。',`<label class="cn-label">版本号<input class="input" value="2.4" data-ci-semver aria-describedby="cn-version-error" aria-invalid="true"><small id="cn-version-error" class="cn-error" data-ci-validation>请输入完整 SemVer，例如 2.4.0。</small></label>`),
  item('forms','inline-edit','行内编辑','Inline edit','阅读与编辑模式不改变对象位置。',`<div class="cn-inline-edit"><input class="input" value="Orbital Desktop" readonly aria-label="项目显示名称" data-ci-inline>${B('编辑','inline')}</div>`),
  item('forms','code-input','代码输入','Code input','配置文本使用等宽字体和显式保存。',`<label class="cn-label">构建环境变量<textarea class="cn-code" rows="4" spellcheck="false" data-ci-code>NODE_ENV=production\nBUILD_CHANNEL=stable\nTARGET_ARCH=arm64</textarea></label>${B('验证格式','validate-code')}`),
 ]},
 {id:'selection',label:'选择与调节',description:'单选、多选与连续数值各有明确语义。',items:[
  item('selection','checkbox','复选框','Checkbox','独立选项有文字标签和选择摘要。',`<fieldset class="cn-fieldset" data-ci-checks><legend>交付证据</legend>${check('源码清单','checked')}${check('签名报告','checked')}${check('SBOM 清单')}${check('组织策略要求','checked disabled')}</fieldset>`),
  item('selection','radio','单选组','Radio group','互斥选项使用方向键和原生选择语义。',`<fieldset class="cn-fieldset" data-ci-checks><legend>默认构建策略</legend>${check('平衡速度与资源','name="cn-strategy" value="balanced" checked','radio')}${check('优先完成时间','name="cn-strategy" value="speed"','radio')}${check('优先资源效率','name="cn-strategy" value="economy"','radio')}</fieldset>`),
  item('selection','switch','开关','Switch','即时二态设置保留名称与当前值。',`<label class="cn-switch"><input type="checkbox" role="switch" checked data-ci-switch><span class="cn-switch-track"></span><span class="cn-switch-copy"><strong>完成通知</strong><small>构建完成后通知我</small></span></label>`),
  item('selection','select','单选下拉','Select','自定义触发器与菜单保留键盘路径。',`<label class="cn-label">构建平台</label>${menu('macOS arm64',options(['macOS arm64','Windows amd64','Linux amd64']))}`),
  item('selection','combobox','可搜索建议','Combobox','输入筛选建议，选择后写回文本值。',`<div class="cn-combo"><label class="cn-label">选择构建池<input class="input" data-ci-combo role="combobox" aria-expanded="true" aria-haspopup="listbox" aria-autocomplete="list" placeholder="输入构建池名称" aria-controls="cn-combo-list"></label><div id="cn-combo-list" class="cn-combo-list" role="listbox" aria-label="构建池建议">${['Apple Native','Windows Native','Cloud BuildKit'].map(t=>`<button type="button" role="option" aria-selected="false" data-ci-act="combo" data-ci-value="${t}">${t}</button>`).join('')}</div></div>`),
  item('selection','multi-select','多选下拉','Multi select','可见摘要和选项数量同时更新。',menu('<span data-ci-multi-label>2 个平台已选</span>',`<fieldset class="cn-fieldset" data-ci-multi><legend>选择平台</legend>${check('macOS arm64','checked')}${check('Windows amd64','checked')}${check('Linux amd64')}${check('OCI arm64')}</fieldset>`)),
  item('selection','tag-input','标签输入','Tag input','Enter 创建标签，删除按钮带完整名称。',`<label class="cn-label">项目标签<div class="cn-tag-editor"><div data-ci-tags><span class="cn-tag">desktop${B('×','remove-tag','aria-label="移除 desktop 标签"')}</span><span class="cn-tag">stable${B('×','remove-tag','aria-label="移除 stable 标签"')}</span></div><input class="input" placeholder="输入后按 Enter" data-ci-tag-input maxlength="24"></div></label>`),
  item('selection','slider','滑块','Slider','连续值提供即时数字和键盘调节。',`<label class="cn-label">CPU 配额 <output data-ci-range-value>65%</output><input type="range" min="10" max="100" value="65" data-ci-range></label><div class="cn-between cn-help"><span>10%</span><span>100%</span></div>`),
  item('selection','range-slider','区间滑块','Range slider','区间两端分别命名，数值不能交叉。',`<fieldset class="cn-fieldset" data-ci-dual><legend>构建时间窗口 <output data-ci-dual-value>8:00 – 20:00</output></legend><label class="cn-label">开始时间<input type="range" min="0" max="23" value="8" aria-label="窗口开始小时" data-ci-low></label><label class="cn-label">结束时间<input type="range" min="0" max="23" value="20" aria-label="窗口结束小时" data-ci-high></label></fieldset>`),
  item('selection','color-picker','颜色选择','Color picker','颜色选项具备名称、勾选与文本摘要。',`<fieldset class="cn-fieldset"><legend>项目识别色</legend><div class="cn-swatches">${[['钴蓝','var(--primary)'],['松绿','var(--success)'],['琥珀','var(--warning)'],['紫罗兰','var(--purple)'],['玫瑰','var(--danger)']].map(([n,c],i)=>`<button type="button" class="cn-swatch" data-ci-act="color" style="--cn-color:${c}" aria-label="${n}" aria-pressed="${!i}">${icon('check',17)}</button>`).join('')}</div></fieldset>`),
 ]},
 {id:'files',label:'日期与文件',description:'选择时间、组织文件，并保留可撤回的结果。',items:[
  item('files','date-picker','日期选择','Date picker','日期有清晰月份、选中态和关闭路径。',menu('<span data-ci-date-label>2026-11-18</span>',days('date'),'cn-date-popover')),
  item('files','date-range','日期区间','Date range','先选开始日期，再选结束日期。',`<div data-ci-range-calendar>${menu('<span data-ci-date-label>11 月 10 日 – 18 日</span>',days('range'),'cn-date-popover')}<small class="cn-help">点击两个日期以选择区间。</small></div>`),
  item('files','time-picker','时间选择','Time picker','小时与分钟分开调节，并写回摘要。',menu('<span data-ci-time-label>14:30</span>',`<div class="cn-time"><div><strong>小时</strong>${[9,10,11,12,13,14,15,16,17,18].map(v=>B(String(v).padStart(2,'0'),'time',`data-ci-unit="hour" data-ci-value="${v}" aria-pressed="${v===14}"`)).join('')}</div><div><strong>分钟</strong>${[0,15,30,45].map(v=>B(String(v).padStart(2,'0'),'time',`data-ci-unit="minute" data-ci-value="${v}" aria-pressed="${v===30}"`)).join('')}</div></div>`)),
  item('files','calendar','日历','Calendar','内嵌日历用于计划，日期为独立按钮。',days('inline')),
  item('files','upload','文件上传','File upload','选择后只显示本地文件名称，不执行上传。',upload()),
  item('files','upload-list','文件列表','Upload list','文件状态与移除动作保持同一行。',`<div class="cn-files" data-ci-file-list>${fileRow('orbital-2.4.0-arm64.dmg','128 MB')}${fileRow('checksums.sha256','1.2 KB')}${fileRow('build-evidence.json','24 KB')}</div>${B('添加示例文件','add-file')}`),
  item('files','dropzone','拖放区域','Dropzone','拖放与点击选择具备同等操作路径。',upload(true)),
  item('files','folder-picker','文件夹选择','Folder picker','树状层级展开后选择目标文件夹。',`<div class="cn-folder-picker"><details open><summary>${icon('folder',17)} Orbital Desktop</summary><div class="cn-folder-children">${['releases/2.4.0','nightly','evidence','symbols'].map((v,i)=>B(`${icon('folder',16)}${v}`,'folder',`aria-pressed="${!i}" data-ci-value="/${v}"`)).join('')}</div></details></div><small class="cn-help" data-ci-folder-path>目标：/releases/2.4.0</small>`),
  item('files','transfer','穿梭选择','Transfer','显式按钮移动对象，保留两侧数量。',`<div class="cn-transfer"><fieldset class="cn-fieldset" data-ci-transfer-from><legend>可用目标</legend>${check('Windows amd64')}${check('Linux amd64')}${check('OCI arm64')}</fieldset><div class="cn-transfer-actions">${B('加入 →','transfer','data-ci-direction="to"')}${B('← 移回','transfer','data-ci-direction="from"')}</div><fieldset class="cn-fieldset" data-ci-transfer-to><legend>已选目标</legend>${check('macOS arm64')}</fieldset></div>`),
  item('files','cascader','级联选择','Cascader','层级逐步展开，完整路径保持可见。',menu('<span data-ci-cascade-label>Apple / macOS / arm64</span>',`<div class="cn-cascade"><div>${options(['Apple','Microsoft','Linux'],'cascade')}</div><div data-ci-cascade-options>${options(['macOS / arm64','macOS / amd64'],'cascade-leaf')}</div></div>`)),
 ]},
];

export function mountInputs(root, signal) {
 const on = (type, fn) => root.addEventListener(type, fn, { signal });
 const syncRange = el => el.style.setProperty('--cn-range', `${(Number(el.value)-Number(el.min))/(Number(el.max)-Number(el.min))*100}%`);
 const syncTransfer = demo => demo.querySelectorAll('.cn-transfer>.cn-fieldset').forEach((field,i)=>field.querySelector('legend').textContent=`${i?'已选':'可用'}目标 · ${field.querySelectorAll('input').length}`);
 root.querySelectorAll('input[type=range]').forEach(syncRange);
 root.querySelectorAll('[data-component=files-transfer] [data-ci-demo]').forEach(syncTransfer);
 root.addEventListener('toggle', e=>{if(e.target.matches?.('.cn-popover'))e.target.querySelector('summary').setAttribute('aria-expanded',String(e.target.open));},{signal,capture:true});
 const say = (el, text) => { const status=el.closest('[data-ci-demo]')?.querySelector('.cn-status'); if(status) status.textContent=text; };
 const closeMenu = el => {const detail=el.closest('details');if(detail){detail.open=false;detail.querySelector('summary').focus();}};
 const picked = (el, selector = 'button') => { el.parentElement.querySelectorAll(selector).forEach(b=>b.setAttribute('aria-pressed',String(b===el))); };
 const listFiles = (container, files) => {
  for(const f of Array.from(files||[]).slice(0,8)) container.insertAdjacentHTML('beforeend',fileRow(f.name, f.size<1048576?`${Math.max(1,Math.round(f.size/1024))} KB`:`${(f.size/1048576).toFixed(1)} MB`));
 };
 on('click', async e => {
  const el=e.target.closest('[data-ci-act]'); if(!el || el.disabled) return;
  const demo=el.closest('[data-ci-demo]'),act=el.dataset.ciAct;
  if(act==='say') say(el,el.dataset.ciValue||'演示操作已完成。');
  if(act==='link') {e.preventDefault();say(el,'演示链接已选择；工作空间保持当前位置。');}
  if(act==='toggle') {const next=el.getAttribute('aria-pressed')!=='true';el.setAttribute('aria-pressed',String(next));const labels=el.dataset.ciToggleLabel?.split('|');if(labels)el.innerHTML=`${icon('heart',17)}${esc(labels[next?0:1])}`;say(el,next?'已开启。':'已关闭。');}
  if(act==='segment') {picked(el);say(el,`已切换为${el.textContent}视图。`);}
  if(act==='add-project') {const n=demo.querySelector('[data-ci-counter]');n.textContent=Number(n.textContent)+1;say(el,'已新增一个演示项目。');}
  if(act==='copy') {try {await navigator.clipboard.writeText(el.dataset.ciCopy);say(el,'已复制演示摘要。');}catch{say(el,`演示摘要：${el.dataset.ciCopy}`);}}
  if(act==='loading') {const text=el.innerHTML;el.disabled=true;el.textContent=el.dataset.ciLoadingLabel;const timer=setTimeout(()=>{if(signal?.aborted)return;el.disabled=false;el.innerHTML=text;say(el,'连接检查通过，仅为本地演示。');},850);signal?.addEventListener('abort',()=>clearTimeout(timer),{once:true});}
  if(act==='pick') {const detail=el.closest('details');detail.querySelector('summary').innerHTML=`${esc(el.dataset.ciValue)}${icon('down',15)}`;detail.open=false;detail.querySelector('summary').focus();say(el,`已选择 ${el.dataset.ciValue}。`);}
  if(act==='password') {const field=demo.querySelector('[data-ci-password]'),show=field.type==='password';field.type=show?'text':'password';el.setAttribute('aria-pressed',String(show));el.setAttribute('aria-label',show?'隐藏演示密码':'显示演示密码');}
  if(act==='step') {const field=demo.querySelector('input[type=number]');field.value=Math.max(Number(field.min),Math.min(Number(field.max),Number(field.value)+Number(el.dataset.ciStep)));say(el,`当前并发数 ${field.value}。`);}
  if(act==='inline') {const field=demo.querySelector('[data-ci-inline]');field.readOnly=!field.readOnly;el.textContent=field.readOnly?'编辑':'保存';if(!field.readOnly){field.focus();field.select();}else say(el,`已保存 ${field.value}。`);}
  if(act==='validate-code') {const lines=demo.querySelector('[data-ci-code]').value.split('\n').filter(Boolean),valid=lines.every(v=>/^[A-Z_][A-Z0-9_]*=.+$/.test(v));say(el,valid?`${lines.length} 行配置格式有效。`:'每行使用大写变量名=值，例如 NODE_ENV=production。');}
  if(act==='combo') {const c=el.closest('.cn-combo');c.querySelector('input').value=el.dataset.ciValue;c.querySelectorAll('[role=option]').forEach(o=>o.setAttribute('aria-selected',String(o===el)));c.querySelector('[role=listbox]').hidden=true;c.querySelector('input').setAttribute('aria-expanded','false');c.querySelector('input').focus();say(el,`已选择 ${el.dataset.ciValue}。`);}
  if(act==='remove-tag') {const name=el.parentElement.textContent.replace('×','');el.parentElement.remove();say(demo,`已移除 ${name}。`);}
  if(act==='color') {picked(el);say(el,`项目颜色：${el.getAttribute('aria-label')}。`);}
  if(act==='month') {
   const c=el.closest('[data-ci-calendar]'),offset=Number(c.dataset.offset||0)+Number(el.dataset.ciStep);c.dataset.offset=offset;
   const d=new Date(2026,10+offset,1),year=d.getFullYear(),month=d.getMonth(),count=new Date(year,month+1,0).getDate(),blank=(d.getDay()+6)%7;
   c.querySelector('[data-ci-month]').textContent=`${year} 年 ${month+1} 月`;
   c.querySelector('[data-ci-days]').innerHTML=Array.from({length:blank},()=>'<span></span>').join('')+Array.from({length:count},(_,i)=>`<button type="button" data-ci-act="day" data-ci-day="${i+1}" aria-pressed="false" aria-label="${year} 年 ${month+1} 月 ${i+1} 日">${i+1}</button>`).join('');
  }
  if(act==='day') {
   const c=el.closest('[data-ci-calendar]'),offset=Number(c.dataset.offset||0),d=new Date(2026,10+offset,Number(el.dataset.ciDay)),value=`${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;
   if(c.dataset.ciCalendar==='range') {
    const r=demo.querySelector('[data-ci-range-calendar]');if(!r.dataset.start||r.dataset.end){r.dataset.start=value;delete r.dataset.end;c.querySelectorAll('[data-ci-day]').forEach(b=>{b.classList.remove('is-picked');b.setAttribute('aria-pressed','false');});el.classList.add('is-picked');el.setAttribute('aria-pressed','true');demo.querySelector('[data-ci-date-label]').textContent=`${value} → 选择结束日期`;say(el,'已选择开始日期。请选择结束日期。');}
    else {const [start,end]=[r.dataset.start,value].sort();r.dataset.start=start;r.dataset.end=end;demo.querySelector('[data-ci-date-label]').textContent=`${start} → ${end}`;el.classList.add('is-picked');el.setAttribute('aria-pressed','true');say(el,`已选择 ${start} 至 ${end}。`);closeMenu(el);}
   } else {c.querySelectorAll('[data-ci-day]').forEach(b=>{b.classList.toggle('is-picked',b===el);b.setAttribute('aria-pressed',String(b===el));});const label=demo.querySelector('[data-ci-date-label]');if(label){label.textContent=value;closeMenu(el);}say(el,`已选择 ${value}。`);}
  }
  if(act==='time') {picked(el);const m=el.closest('.cn-menu'),hour=m.querySelector('[data-ci-unit=hour][aria-pressed=true]')?.dataset.ciValue||14,minute=m.querySelector('[data-ci-unit=minute][aria-pressed=true]')?.dataset.ciValue||30;demo.querySelector('[data-ci-time-label]').textContent=`${String(hour).padStart(2,'0')}:${String(minute).padStart(2,'0')}`;say(el,'时间选择已更新。');}
  if(act==='remove-file') {el.closest('.cn-file').remove();say(demo,'已移除演示文件。');}
  if(act==='add-file') {demo.querySelector('[data-ci-file-list]').insertAdjacentHTML('beforeend',fileRow(`build-report-${demo.querySelectorAll('.cn-file').length+1}.json`,'12 KB'));say(el,'已添加示例文件。');}
  if(act==='folder') {picked(el);demo.querySelector('[data-ci-folder-path]').textContent=`目标：${el.dataset.ciValue}`;say(el,`已选择 ${el.dataset.ciValue}。`);}
  if(act==='transfer') {const from=demo.querySelector(el.dataset.ciDirection==='to'?'[data-ci-transfer-from]':'[data-ci-transfer-to]'),to=demo.querySelector(el.dataset.ciDirection==='to'?'[data-ci-transfer-to]':'[data-ci-transfer-from]'),checked=from.querySelectorAll('input:checked');checked.forEach(i=>{i.checked=false;to.append(i.closest('label'));});syncTransfer(demo);say(el,`已移动 ${checked.length} 个目标。`);}
  if(act==='cascade') {const platforms={Apple:['macOS / arm64','macOS / amd64','iOS / arm64'],Microsoft:['Windows / amd64','Windows / arm64'],Linux:['Linux / amd64','Linux / arm64']};picked(el);el.closest('.cn-menu').dataset.family=el.dataset.ciValue;demo.querySelector('[data-ci-cascade-options]').innerHTML=options(platforms[el.dataset.ciValue],'cascade-leaf');say(el,`请选择 ${el.dataset.ciValue} 的系统与架构。`);}
  if(act==='cascade-leaf') {const m=el.closest('.cn-menu');demo.querySelector('[data-ci-cascade-label]').textContent=`${m.dataset.family||'Apple'} / ${el.dataset.ciValue}`;closeMenu(el);say(el,'完整平台路径已选择。');}
 });
 on('input',e=>{
  const el=e.target,demo=el.closest('[data-ci-demo]');if(!demo)return;
  if(el.matches('input[type=range]'))syncRange(el);
  if(el.matches('[data-ci-count]'))demo.querySelector('[data-ci-count-value]').textContent=el.value.length;
  if(el.matches('[data-ci-filter]')) {let matches=0;demo.querySelectorAll('[data-ci-filter-list] li').forEach(li=>{li.hidden=!li.textContent.toLowerCase().includes(el.value.toLowerCase());if(!li.hidden)matches++;});demo.querySelector('[data-ci-empty]').hidden=!!matches;}
  if(el.closest('[data-ci-otp]')) {el.value=el.value.replace(/\D/g,'').slice(-1);if(el.value)el.nextElementSibling?.focus();const values=[...el.parentElement.querySelectorAll('input')].map(i=>i.value);if(values.every(Boolean))say(el,`演示验证码 ${values.join('')} 已填满。`);}
  if(el.matches('[data-ci-combo]')) {demo.querySelector('[role=listbox]').hidden=false;el.setAttribute('aria-expanded','true');demo.querySelectorAll('[role=option]').forEach(o=>o.hidden=!o.textContent.toLowerCase().includes(el.value.toLowerCase()));}
  if(el.matches('[data-ci-range]'))demo.querySelector('[data-ci-range-value]').textContent=`${el.value}%`;
  if(el.closest('[data-ci-dual]')) {const d=el.closest('[data-ci-dual]'),low=d.querySelector('[data-ci-low]'),high=d.querySelector('[data-ci-high]');if(Number(low.value)>Number(high.value)){if(el===low)high.value=low.value;else low.value=high.value;}[low,high].forEach(syncRange);d.querySelector('[data-ci-dual-value]').textContent=`${low.value}:00 – ${high.value}:00`;}
 });
 on('change',e=>{
  const el=e.target,demo=el.closest('[data-ci-demo]');if(!demo)return;
  if(el.matches('[data-ci-files]')){listFiles(demo.querySelector('[data-ci-file-list]'),el.files);say(el,`已选择 ${el.files.length} 个文件，仅展示名称。`);el.value='';}
  if(el.closest('[data-ci-multi]')){const n=demo.querySelectorAll('[data-ci-multi] input:checked').length;demo.querySelector('[data-ci-multi-label]').textContent=`${n} 个平台已选`;say(el,`已选择 ${n} 个平台。`);}
  if(el.closest('[data-ci-checks]'))say(el,`已选择 ${demo.querySelectorAll('input:checked').length} 项。`);
  if(el.matches('[data-ci-switch]'))say(el,el.checked?'完成通知已开启。':'完成通知已关闭。');
 });
 on('focusout',e=>{
  const el=e.target;if(!el.matches('[data-ci-semver]'))return;const valid=/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(el.value);el.setAttribute('aria-invalid',String(!valid));const status=el.closest('[data-ci-demo]').querySelector('[data-ci-validation]');status.textContent=valid?'版本号格式有效。':'请输入完整 SemVer，例如 2.4.0。';status.classList.toggle('cn-error',!valid);
 });
 on('keydown',e=>{
  const el=e.target;
  if(el.matches('[data-ci-tag-input]')&&e.key==='Enter') {e.preventDefault();const value=el.value.trim();if(!value)return;const holder=el.parentElement.querySelector('[data-ci-tags]');if([...holder.children].some(c=>c.textContent.replace('×','')===value)){say(el,'此标签已存在。');return;}holder.insertAdjacentHTML('beforeend',`<span class="cn-tag">${esc(value)}${B('×','remove-tag',`aria-label="移除 ${esc(value)} 标签"`)}</span>`);el.value='';say(el,`已添加 ${value}。`);}
  if(el.matches('[data-ci-combo]')&&e.key==='ArrowDown'){el.closest('.cn-combo').querySelector('[role=listbox]').hidden=false;el.setAttribute('aria-expanded','true');const first=[...el.closest('.cn-combo').querySelectorAll('[role=option]')].find(o=>!o.hidden);if(first){e.preventDefault();first.focus();}}
  if(el.matches('[role=option]')&&el.closest('.cn-combo')&&['ArrowDown','ArrowUp','Home','End'].includes(e.key)){const choices=[...el.parentElement.querySelectorAll('[role=option]')].filter(o=>!o.hidden),i=choices.indexOf(el);e.preventDefault();choices[e.key==='Home'?0:e.key==='End'?choices.length-1:(i+(e.key==='ArrowDown'?1:-1)+choices.length)%choices.length]?.focus();}
  if(e.key==='Escape'&&el.closest('.cn-combo')){const c=el.closest('.cn-combo');c.querySelector('[role=listbox]').hidden=true;c.querySelector('input').setAttribute('aria-expanded','false');c.querySelector('input').focus();e.preventDefault();e.stopPropagation();}
  if(el.closest('[data-ci-otp]')&&e.key==='Backspace'&&!el.value)el.previousElementSibling?.focus();
  if(el.closest('[data-ci-shortcut]')&&e.altKey&&e.key.toLowerCase()==='b'){e.preventDefault();say(el,'快捷操作已执行。');}
  if(e.key==='Escape'){const details=el.closest('.cn-popover[open]');if(details){details.open=false;details.querySelector('summary').focus();e.preventDefault();e.stopPropagation();}}
  if(el.closest('.cn-menu')&&['ArrowDown','ArrowUp'].includes(e.key)){const buttons=[...el.closest('.cn-menu').querySelectorAll('button:not(:disabled),input')].filter(x=>!x.hidden),index=buttons.indexOf(el);if(index>=0){e.preventDefault();buttons[(index+(e.key==='ArrowDown'?1:-1)+buttons.length)%buttons.length].focus();}}
 });
 on('paste',e=>{if(!e.target.closest('[data-ci-otp]'))return;const value=e.clipboardData.getData('text').replace(/\D/g,'').slice(0,6);if(!value)return;e.preventDefault();const inputs=[...e.target.parentElement.querySelectorAll('input')];inputs.forEach((i,n)=>i.value=value[n]||'');inputs[Math.min(value.length,5)].focus();say(e.target,`已粘贴 ${value.length} 位演示验证码。`);});
 on('dragover',e=>{const zone=e.target.closest('[data-ci-drop]');if(zone){e.preventDefault();zone.classList.add('cn-dragging');}});
 on('dragleave',e=>{e.target.closest('[data-ci-drop]')?.classList.remove('cn-dragging');});
 on('drop',e=>{const zone=e.target.closest('[data-ci-drop]');if(!zone)return;e.preventDefault();zone.classList.remove('cn-dragging');listFiles(zone.closest('[data-ci-demo]').querySelector('[data-ci-file-list]'),e.dataTransfer.files);say(zone,'已接收本地文件名称，未上传。');});
 root.querySelectorAll('[data-ci-count]').forEach(el=>el.closest('[data-ci-demo]').querySelector('[data-ci-count-value]').textContent=el.value.length);
}
