/**
 * [INPUT]: 依赖 ui.js 的语义组件与 entry-data.js 的空间项目索引。
 * [OUTPUT]: 导出 renderTenants，呈现交付主张、玻璃雕塑和空间目录。
 * [POS]: 独立账户大厅；只派发动作，不修改空间或登录状态。
 * [PROTOCOL]: Update this header on change, then check AGENTS.md
 */
import { icon, btn, esc } from './ui.js';
import { getWorkspaces } from './entry-data.js';
import { renderHeaderTools } from './entry-header.js';

const enterWorkspace = (workspace, label = '进入工作空间') => btn(label, 'choose-workspace', 'secondary', 'arrow', `data-name="${esc(workspace.name)}" data-workspace="${esc(workspace.id)}"`);

function deliveryScene(workspace) {
    return `<div class="et-scene" data-entry-scene aria-hidden="true">
        <span class="et-scene-orbit"></span><span class="et-scene-orbit et-scene-orbit-back"></span>
        <div class="et-scene-floor"></div>
        <div class="et-scene-pane et-pane-back glass"><span>${icon('branch', 20)}</span><i></i><i></i><i></i></div>
        <div class="et-scene-pane et-pane-front glass"><span class="et-scene-emblem ${esc(workspace.color)}">${esc(workspace.initial)}</span><div class="et-scene-check">${icon('check', 14)}</div></div>
        <span class="et-scene-node et-node-one glass">${icon('box', 23)}</span>
        <span class="et-scene-node et-node-two glass">${icon('layers', 23)}</span>
        <div class="et-scene-caption"><span>SOURCE</span><i></i><span>BUILD</span><i></i><span>DELIVER</span></div>
    </div>`;
}

function workspaceCard(workspace, state, index) {
    return `<article class="et-workspace-card content-surface" style="--et-order:${index}">
        <div class="et-card-top"><div class="et-space-avatar ${esc(workspace.color)}">${esc(workspace.initial)}</div><span class="et-role">${icon('shield', 14)}${esc(workspace.role)}</span></div>
        <div class="et-card-copy"><h3>${esc(workspace.name)}</h3><p>项目、成员与构建资源，在此协作。</p></div>
        <div class="et-card-domain">${icon('folder', 14)}<span>/tenants/${esc(workspace.id)}/projects</span></div>
        <div class="et-card-bottom"><div><strong>项目</strong><span>构建与交付</span></div>${enterWorkspace(workspace, '进入空间')}</div>
    </article>`;
}

export function renderTenants(state) {
    const workspaces = getWorkspaces(state);
    const recent = workspaces.find(workspace => state.workspaceId ? workspace.id === state.workspaceId : workspace.name === state.workspace) || workspaces[0];
    const query = (state.tenantQuery || '').trim().toLowerCase();
    const filtered = workspaces.filter(workspace => `${workspace.name} ${workspace.id}`.toLowerCase().includes(query));
    return `<main id="main" class="et-page" tabindex="-1">
        <header class="et-topbar">
            <a class="et-brand" href="#tenants" data-route="tenants" title="工作空间大厅"><img src="assets/brand.svg" alt=""><span>Artifactory</span><sup>PRISM</sup></a>
            <div class="et-topbar-right"><span class="et-personal-label">个人工作台</span>${renderHeaderTools(state)}</div>
        </header>
        <div class="et-body">
            <section class="et-hero" aria-labelledby="et-title"><div class="et-hero-copy"><div class="eyebrow">${icon('spark',14)}欢迎回来，${esc(state.demoAccount?.display_name || '林知远')}</div><h1 id="et-title">每一次创造，<br><span>都有自己的空间。</span></h1><p>连接项目、成员与构建资源。<br>让想法从同一处开始，自由抵达。</p>${btn('创建工作空间', 'new-tenant', 'primary', 'plus')}<small>账户 → 工作空间 → 项目 → 版本 → 构建</small></div>${deliveryScene(recent)}</section>
            <section class="et-directory" aria-labelledby="et-directory-title"><div class="et-directory-head"><div class="et-directory-title"><h2 id="et-directory-title">你的工作空间</h2><span>${workspaces.length.toString().padStart(2, '0')}</span></div><div class="et-search glass" data-entry-glass role="search">${icon('search', 18)}<label class="sr-only" for="et-tenant-search">搜索工作空间</label><input id="et-tenant-search" type="search" data-entry-tenant-search value="${esc(state.tenantQuery || '')}" placeholder="查找工作空间" autocomplete="off">${query ? '<button type="button" class="et-clear" data-action="entry-clear-tenants" aria-label="清除空间搜索">'+icon('close', 16)+'</button>' : ''}</div></div>
                <div class="et-workspace-grid">${filtered.map((workspace, index) => workspaceCard(workspace, state, index)).join('')}</div>
                ${!filtered.length ? `<div class="et-empty content-surface"><span class="et-empty-symbol">${icon('search', 26)}</span><h3>没有找到这个工作空间</h3><p>试试团队名称，或清除搜索查看全部空间。</p>${btn('清除搜索', 'entry-clear-tenants', 'secondary', 'refresh')}</div>` : ''}
            </section>
            <div class="et-join-prompt"><span>${icon('users', 18)}已有团队发来邀请？请打开团队发送的邀请链接。</span></div>
            <footer class="et-footer"><span>属于你的空间，连接每一种可能。</span><span>ARTIFACTORY / DESIGN PROTOTYPE</span></footer>
        </div>
    </main>`;
}
