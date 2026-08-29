/**
 * skin-list.js — 皮肤列表组件
 */
import API from './api.js';
import showToast from './toast.js';
import { t } from './i18n.js';
import { esc, escAttr, dispName, confirmDialog, bindEsc, closeOnMaskClick } from './dom.js';

export default class SkinList {
  constructor(container, { onSelect } = {}) {
    this.container = container;
    this.onSelect = onSelect;
    this.skins = [];
    this.selectedId = null;
    // 搜索查询词：在实例上常驻——外壳语言重绘只换容器与输入框，
    // app.js bindSearch 重绑时从这里回填，查询不丢
    this.query = '';
    // 预览图 URL 的缓存戳：值不变则 WebView2 直接命中缓存，避免每次
    // render 全量重解码；重新截取或皮肤版本更新时才 bump。
    this.previewVersions = new Map();
    this._lastVersions = new Map();
    // 皮肤分组：groups = [{id,name,collapsed}]（数组序 = 显示顺序）；
    // groupMap = {皮肤id: 组id}。app.js 启动时从 config.json 读入经
    // setGroups 下发；任何组操作经 _commitGroups 整体回写持久化。
    // 「未分组」是内置虚拟组（不落盘、不可删、恒在末尾）。
    // 组的改名/成员增删统一走「分组编辑」对话框（openGroupEditor），
    // 新建与编辑同一个入口
    this.groups = [];
    this.groupMap = {};
    this._groupMenuEl = null;    // 开着的组 ⋯ 菜单（body 级浮层）
    this._groupEditorEl = null;  // 开着的分组编辑对话框（body 级浮层）
    // 菜单的文档级关闭路径（实例外壳重绘不换 document，一次注册即可）：
    // 点菜单外 / Esc 关闭；⋯ 钮自身的点击已 stopPropagation，不走这里
    document.addEventListener('click', (e) => {
      if (this._groupMenuEl && !e.target.closest('.group-menu')) this.closeGroupMenu();
    });
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && this._groupMenuEl) this.closeGroupMenu();
    });
    this.bindScrollFade();
  }

  /** 滚动渐变淡出：列表滚出顶部后给容器挂 scrolled，CSS 在顶部做渐隐
      遮罩（卡片不再被生硬截断）。容器在外壳重绘后会被替换——app.js 的
      rerender 重绑后需再调一次本方法 */
  bindScrollFade() {
    this.container.addEventListener('scroll', () => {
      this.container.classList.toggle('scrolled', this.container.scrollTop > 2);
      // 滚动时组头位置已变，开着的 ⋯ 菜单锚点失效——随手关掉
      this.closeGroupMenu();
    });
  }

  bumpPreview(skinId) {
    this.previewVersions.set(skinId, (this.previewVersions.get(skinId) || 0) + 1);
  }

  // 搜索过滤入口（app.js 搜索框 input 事件调用）：就地重绘，
  // 选中的皮肤被滤掉时保留选中态（配置面板不动，清空查询后卡片回来）
  setQuery(q) {
    this.query = q;
    this.render();
  }

  async refresh() {
    try {
      const skins = await API.listSkins();
      // 皮肤更新（版本号变化）可能换了同路径的预览图：版本变化时 bust 一次
      for (const s of skins) {
        const prev = this._lastVersions.get(s.id);
        if (prev !== undefined && prev !== s.version) this.bumpPreview(s.id);
      }
      this._lastVersions = new Map(skins.map(s => [s.id, s.version]));
      this.skins = skins;
      this.render();
      return true;
    } catch (err) {
      // 失败保留旧列表（置空会让「数据还在」的列表误显「还没有皮肤」空态）
      this.showToast(t('list.loadFailed') + String(err), 'error');
      return false;
    }
  }

  select(skinId) {
    this.selectedId = skinId;
    this.render();
    if (this.onSelect) this.onSelect(skinId);
  }

  // 分组数据下发（app.js 启动时从 config 读入）。容错手改的配置：
  // 非数组/对象按空处理；map 指向不存在组的条目渲染时按未分组对待
  setGroups(groups, map) {
    this.groups = Array.isArray(groups)
      ? groups
          .filter(g => g && typeof g.id === 'string' && typeof g.name === 'string')
          .map(g => ({ id: g.id, name: g.name, collapsed: g.collapsed === true }))
      : [];
    this.groupMap = (map && typeof map === 'object') ? { ...map } : {};
  }

  // 皮肤所属组 id；未指派或指向已删除组 → null（未分组）
  _groupOf(skinId) {
    const gid = this.groupMap[skinId];
    return (gid && this.groups.some(g => g.id === gid)) ? gid : null;
  }

  // 组状态变更统一入口：先本地变更 + 重绘，再整体回写持久化；
  // 落盘失败回滚到变更前并弹错（与视图切换/设置页开关同语义）。
  // 返回是否保存成功（调用方决定要不要弹成功反馈）
  async _commitGroups(mutate) {
    const prevGroups = JSON.parse(JSON.stringify(this.groups));
    const prevMap = { ...this.groupMap };
    mutate();
    this.render();
    try {
      await API.setSkinGroups(this.groups, this.groupMap);
      return true;
    } catch (err) {
      this.groups = prevGroups;
      this.groupMap = prevMap;
      this.render();
      this.showToast(t('common.setFailed') + String(err), 'error');
      return false;
    }
  }

  toggleGroupCollapsed(gid) {
    const g = this.groups.find(x => x.id === gid);
    if (!g) return;
    this._commitGroups(() => { g.collapsed = !g.collapsed; });
  }

  // ── 分组编辑对话框（新建/编辑同一入口）：改名 + 勾选皮肤入组 ──
  // gid = null 为新建；结构特殊的弹窗不复用 confirmDialog 工厂（其正文为
  // 单 <p> 且确认必关，表单需要校验后留置），只复用 bindEsc /
  // closeOnMaskClick 小工具与 confirm-* 视觉类（dom.js 注释指明的模式）
  openGroupEditor(gid) {
    this.closeGroupEditor();
    this.closeGroupMenu();
    const g = gid ? this.groups.find(x => x.id === gid) : null;
    if (gid && !g) return;

    // 每行：勾选框 + 状态灯 + 名称 +（在别的组时的）现属组名——勾选他组
    // 皮肤 = 移过来，现属组名让这个「移」可预期
    const rows = this.skins.map(s => {
      const owner = this._groupOf(s.id);
      const ownerName = (owner && owner !== gid)
        ? (this.groups.find(x => x.id === owner)?.name || '') : '';
      const checked = gid ? owner === gid : false;
      const statusClass = !s.loaded ? 'unloaded' : s.hidden ? 'hidden' : 'loaded';
      return `
        <label class="group-edit-skin">
          <input type="checkbox" data-skin-id="${escAttr(s.id)}"${checked ? ' checked' : ''}>
          <span class="compact-status ${statusClass}"><span class="status-dot"></span></span>
          <span class="group-edit-skin-name">${esc(dispName(s))}</span>
          ${ownerName ? `<span class="group-edit-skin-group">${esc(ownerName)}</span>` : ''}
        </label>`;
    }).join('');

    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = `
      <div class="confirm-dialog wide group-edit-dialog">
        <h3>${g ? t('list.groupEditTitle') : t('list.groupNewTitle')}</h3>
        <div class="group-edit-field">
          <label class="group-edit-label">${t('list.groupName')}</label>
          <input class="group-edit-name" maxlength="64" spellcheck="false" autocomplete="off"
                 placeholder="${t('list.groupNamePlaceholder')}" value="${escAttr(g?.name || '')}">
          <div class="group-edit-error" hidden>${t('list.groupNameRequired')}</div>
        </div>
        <div class="group-edit-field">
          <label class="group-edit-label">${t('list.groupSkins')}</label>
          <div class="group-edit-hint">${t('list.groupSkinsHint')}</div>
          <div class="group-edit-skins">${rows || `<div class="group-edit-empty">${t('list.empty')}</div>`}</div>
        </div>
        <div class="confirm-buttons">
          <button class="confirm-btn cancel">${t('common.cancel')}</button>
          <button class="confirm-btn primary">${t('common.save')}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    this._groupEditorEl = overlay;

    const close = () => this.closeGroupEditor();
    this._groupEditorUnbind = bindEsc(close);
    closeOnMaskClick(overlay, close);
    overlay.querySelector('.confirm-btn.cancel').onclick = close;

    const nameInput = overlay.querySelector('.group-edit-name');
    const errorEl = overlay.querySelector('.group-edit-error');
    nameInput.addEventListener('input', () => { errorEl.hidden = true; });
    nameInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); save(); }
    });

    const save = async () => {
      const name = nameInput.value.trim();
      // 名称必填：留空不关闭，错误行就地提示（表单弹窗与确认框的差异点）
      if (!name) {
        errorEl.hidden = false;
        nameInput.focus();
        return;
      }
      const checked = new Set(
        [...overlay.querySelectorAll('.group-edit-skin input:checked')]
          .map(cb => cb.dataset.skinId)
      );
      close();
      // 成员语义：勾选 = 属于本组（他组皮肤移入）；未勾选的原成员移出
      //（回落未分组）；与本组无关的皮肤归属不动
      const ok = await this._commitGroups(() => {
        let target = gid;
        if (!target) {
          target = `g-${Date.now()}`;
          this.groups.push({ id: target, name, collapsed: false });
        } else {
          const grp = this.groups.find(x => x.id === target);
          if (grp) grp.name = name;
        }
        for (const s of this.skins) {
          if (checked.has(s.id)) this.groupMap[s.id] = target;
          else if (this.groupMap[s.id] === target) delete this.groupMap[s.id];
        }
      });
      if (ok && !gid) this.showToast(t('list.groupCreated'), 'success');
    };
    overlay.querySelector('.confirm-btn.primary').onclick = save;

    // 表单弹窗焦点落主字段（编辑态全选现名便于直接改）
    nameInput.focus();
    if (g) nameInput.select();
  }

  closeGroupEditor() {
    this._groupEditorUnbind?.();
    this._groupEditorUnbind = null;
    this._groupEditorEl?.remove();
    this._groupEditorEl = null;
  }

  // 删除组不删皮肤：成员全部回落「未分组」（确认框语义与删除皮肤区分）
  confirmDeleteGroup(gid) {
    const g = this.groups.find(x => x.id === gid);
    if (!g) return;
    confirmDialog({
      title: t('list.groupDeleteTitle'),
      bodyHtml: t('list.groupDeleteBody', { name: `<strong>"${esc(g.name)}"</strong>` }),
      hint: t('list.groupDeleteHint'),
      confirmText: t('common.delete'),
      danger: true,
      onConfirm: async () => {
        const ok = await this._commitGroups(() => {
          this.groups = this.groups.filter(x => x.id !== gid);
          for (const sid of Object.keys(this.groupMap)) {
            if (this.groupMap[sid] === gid) delete this.groupMap[sid];
          }
        });
        if (ok) this.showToast(t('list.groupDeleted'), 'info');
      },
    });
  }

  // 组 ⋯ 菜单（body 级浮层；管理器全局禁右键，组操作走左键小菜单）。
  // 定位：右缘对齐锚点钮，横向钳在窗口内。结构：上段组编辑（编辑/删除），
  // 分隔线，下段组内皮肤批量控制（加载/卸载/隐藏/显示）
  openGroupMenu(gid, anchor) {
    this.closeGroupMenu();
    const g = this.groups.find(x => x.id === gid);
    if (!g) return;
    anchor.classList.add('open');
    const menu = document.createElement('div');
    menu.className = 'group-menu';
    const item = (act, svg, label, cls = '') => `<button data-act="${act}"${cls ? ` class="${cls}"` : ''}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${svg}</svg>
        ${label}
      </button>`;
    menu.innerHTML =
      item('edit', '<path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>', t('list.groupEdit')) +
      item('delete', '<polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/>', t('list.groupDelete'), 'danger') +
      '<div class="menu-sep"></div>' +
      item('load', '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>', t('list.groupLoadAll')) +
      item('unload', '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/>', t('list.groupUnloadAll')) +
      item('hide', '<path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/>', t('list.groupHideAll')) +
      item('show', '<path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>', t('list.groupShowAll'));
    document.body.appendChild(menu);
    const r = anchor.getBoundingClientRect();
    const mw = menu.offsetWidth;
    menu.style.left = Math.max(8, Math.min(r.right - mw, window.innerWidth - mw - 8)) + 'px';
    menu.style.top = (r.bottom + 4) + 'px';
    this._groupMenuEl = menu;
    menu.querySelector('[data-act="edit"]').addEventListener('click', () => {
      this.closeGroupMenu();
      this.openGroupEditor(gid);
    });
    menu.querySelector('[data-act="delete"]').addEventListener('click', () => {
      this.closeGroupMenu();
      this.confirmDeleteGroup(gid);
    });
    for (const act of ['load', 'unload', 'hide', 'show']) {
      menu.querySelector(`[data-act="${act}"]`).addEventListener('click', () => {
        this.closeGroupMenu();
        this._batchGroupAction(gid, act);
      });
    }
  }

  // 组内皮肤批量控制：只对「目标状态之外」的成员执行（幂等——全已处于
  // 目标态时报无可操作）；逐个串行调用（窗口创建不开并发），计数反馈
  async _batchGroupAction(gid, action) {
    const members = this.skins.filter(s => this._groupOf(s.id) === gid);
    const targets = members.filter(s => {
      if (action === 'load') return !s.loaded;
      if (action === 'unload') return s.loaded;
      if (action === 'hide') return s.loaded && !s.hidden;
      return s.loaded && s.hidden; // show
    });
    if (targets.length === 0) {
      this.showToast(t('list.batchNoop'), 'info');
      return;
    }
    let ok = 0;
    let failed = 0;
    for (const s of targets) {
      try {
        if (action === 'load') await API.loadSkin(s.id);
        else if (action === 'unload') await API.unloadSkin(s.id);
        else await API.setSkinVisibility(s.id, action === 'show');
        ok++;
      } catch {
        failed++;
      }
    }
    const key = { load: 'list.batchLoaded', unload: 'list.batchUnloaded', hide: 'list.batchHidden', show: 'list.batchShown' }[action];
    if (ok > 0) this.showToast(t(key, { count: ok }), failed ? 'info' : 'success');
    if (failed > 0) this.showToast(t('list.batchFailed', { count: failed }), 'error');
    // 事件路径（skin-loaded/unloaded/skins-visibility-changed）已覆盖
    // 列表刷新，此处兜底一次对齐终态（含失败项的真实状态）
    await this.refresh();
  }

  closeGroupMenu() {
    this._groupMenuEl?.remove();
    this._groupMenuEl = null;
    this.container?.querySelectorAll('.group-more.open').forEach(b => b.classList.remove('open'));
  }

  render() {
    // 任何重绘都先摘掉 ⋯ 菜单（锚点元素随 innerHTML 重建即失效）
    this.closeGroupMenu();
    const q = this.query.trim().toLowerCase();
    // 匹配显示名（当前语言）、id、作者，大小写不敏感；列表规模小，
    // 每次输入即时过滤即可，无需防抖
    const visible = q
      ? this.skins.filter(s => `${dispName(s) || ''} ${s.id} ${s.author || ''}`.toLowerCase().includes(q))
      : this.skins;

    // 侧栏标题旁的数量徽标：「已加载/总数」（左 = 已加载皮肤数，右 = 总数——
    // 用户一眼可知加载了多少、一共有多少；实机反馈改此格式）。已加载数 >0
    // 时 accent 强调；无皮肤时置空字符串，:empty 整体收起
    const countEl = document.getElementById('skin-count');
    if (countEl) {
      const loaded = this.skins.filter(s => s.loaded).length;
      countEl.innerHTML = this.skins.length
        ? `<span class="count-loaded${loaded ? '' : ' zero'}">${loaded}</span>/${this.skins.length}`
        : '';
    }
    if (this.skins.length === 0) {
      this.container.innerHTML = `
        <div class="list-empty">
          <div class="list-empty-icon">
            <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>
          </div>
          <p>${t('list.empty')}</p>
          <p class="list-empty-hint">${t('list.emptyHint')}</p>
        </div>`;
      return;
    }

    // 有过滤词但零命中：搜索专属空态（与「还没有皮肤」区分）
    if (visible.length === 0) {
      this.container.innerHTML = `
        <div class="list-empty">
          <div class="list-empty-icon">
            <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/><line x1="8.5" y1="8.5" x2="13.5" y2="13.5"/><line x1="13.5" y1="8.5" x2="8.5" y2="13.5"/></svg>
          </div>
          <p>${t('list.noResults')}</p>
          <p class="list-empty-hint">${t('list.noResultsHint')}</p>
        </div>`;
      return;
    }

    this.container.innerHTML = this.renderList(visible);
    this.bindCards();
    this.bindGroups();
  }

  // 列表主体：从未分组过时平铺（与特性引入前完全一致），有分组时按组
  // 分节；末尾恒有「新建分组」入口（点击弹出分组编辑对话框）
  renderList(visible) {
    const filtering = this.query.trim().length > 0;
    let html = '';
    if (this.groups.length === 0) {
      html = visible.map(skin => this.renderCard(skin)).join('');
    } else {
      for (const g of this.groups) {
        const all = this.skins.filter(s => this._groupOf(s.id) === g.id);
        const members = visible.filter(s => this._groupOf(s.id) === g.id);
        // 过滤中无命中的组整组隐藏（空组平时保留，与「未分组」同作落点语义）
        if (filtering && members.length === 0) continue;
        html += this.renderGroupSection(g, members, all.length, false);
      }
      // 内置虚拟组「未分组」：不落盘、不可删、恒在末尾
      const unAll = this.skins.filter(s => this._groupOf(s.id) === null);
      const unMembers = visible.filter(s => this._groupOf(s.id) === null);
      if (!(filtering && unMembers.length === 0)) {
        html += this.renderGroupSection(null, unMembers, unAll.length, true);
      }
    }
    html += `<button class="group-new">+ ${t('list.newGroup')}</button>`;
    return html;
  }

  // 组区块：组头（chevron + 名称 + 计数 + ⋯ 菜单钮）+ 组体（成员卡片）。
  // 过滤中计数显示「命中/总数」（与侧栏数量徽标同口径）
  renderGroupSection(g, members, totalCount, ungrouped) {
    const gid = g ? g.id : '';
    const collapsed = g && g.collapsed ? ' collapsed' : '';
    const filtering = this.query.trim().length > 0;
    const countText = filtering && members.length !== totalCount
      ? `${members.length}/${totalCount}`
      : String(totalCount);
    const chevron = ungrouped
      ? '<span class="group-chevron-spacer"></span>'
      : `<span class="group-chevron"><svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><polyline points="2 3.5 5 6.5 8 3.5"/></svg></span>`;
    const moreBtn = ungrouped
      // 未分组无 ⋯ 菜单：等宽占位让计数与常规组垂直同轴对齐
      ? '<span class="group-more-spacer"></span>'
      : `<button class="group-more" data-group-id="${escAttr(gid)}" title="${t('list.groupActions')}"><svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="1.8"/><circle cx="12" cy="12" r="1.8"/><circle cx="19" cy="12" r="1.8"/></svg></button>`;
    return `
      <div class="skin-group${ungrouped ? ' ungrouped' : ''}${collapsed}" data-group-id="${escAttr(gid)}">
        <div class="group-header" data-group-id="${escAttr(gid)}">
          ${chevron}
          <span class="group-name">${esc(g ? g.name : t('list.ungrouped'))}</span>
          <span class="group-count">${countText}</span>
          ${moreBtn}
        </div>
        <div class="group-body">${members.map(skin => this.renderCard(skin)).join('')}</div>
      </div>`;
  }

  bindCards() {
    this.container.querySelectorAll('.skin-card').forEach(card => {
      card.addEventListener('click', (e) => {
        if (e.target.closest('.load-btn') || e.target.closest('.skin-delete-btn')) return;
        this.select(card.dataset.skinId);
      });
    });

    this.container.querySelectorAll('.load-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        // 点击即禁用防连点并发；成功后 refresh 重建按钮，仅失败需恢复
        btn.disabled = true;
        const done = btn.dataset.action === 'load'
          ? this.loadSkin(btn.dataset.skinId)
          : this.unloadSkin(btn.dataset.skinId);
        done.finally(() => { if (btn.isConnected) btn.disabled = false; });
      });
    });

    this.container.querySelectorAll('.skin-delete-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        // 运行中的皮肤禁用删除：.disabled 只是视觉态，点击仍会出现，需在此拦截
        if (btn.classList.contains('disabled')) {
          this.showToast(t('list.runningDeleteBlocked'), 'info');
          return;
        }
        this.confirmDelete(btn.dataset.skinId, btn.dataset.skinName);
      });
    });

    // 预览图加载失败：藏起 <img>、换出占位块。CSP 禁内联事件处理器，
    // 故在 JS 侧绑定——与 innerHTML 同一同步块内完成，不会错过 error 事件
    this.container.querySelectorAll('.skin-preview-img').forEach(img => {
      img.addEventListener('error', () => {
        img.style.display = 'none';
        img.nextElementSibling.style.display = 'flex';
      });
    });
  }

  bindGroups() {
    // 组头：点击折叠/展开（点在 ⋯ 上不触发；未分组虚拟组不可折叠）
    this.container.querySelectorAll('.group-header').forEach(header => {
      header.addEventListener('click', (e) => {
        if (e.target.closest('.group-more')) return;
        const gid = header.dataset.groupId;
        if (!gid) return;
        this.toggleGroupCollapsed(gid);
      });
    });

    // 组 ⋯ 菜单钮（再点一次已打开的钮 = 收起）
    this.container.querySelectorAll('.group-more').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        if (this._groupMenuEl && btn.classList.contains('open')) {
          this.closeGroupMenu();
          return;
        }
        this.openGroupMenu(btn.dataset.groupId, btn);
      });
    });

    // 新建分组入口：弹出分组编辑对话框（改名 + 勾选皮肤入组）
    this.container.querySelector('.group-new')?.addEventListener('click', () => this.openGroupEditor(null));
  }

  renderCard(skin) {
    const selected = skin.id === this.selectedId ? ' selected' : '';
    // 状态三档：未加载（灰）/ 已加载但窗口不可见 = 已隐藏（黄）/ 运行中（绿）。
    // hidden 由后端按真实窗口可见性（is_visible）下发，不是热键簿记
    const statusClass = !skin.loaded ? 'unloaded' : skin.hidden ? 'hidden' : 'loaded';
    const statusText = !skin.loaded ? t('common.unloaded') : skin.hidden ? t('common.hidden') : t('common.running');
    const deleteTitle = skin.loaded ? t('list.unloadBeforeDelete') : t('common.deleteSkin');
    const deleteDisabled = skin.loaded ? ' disabled' : '';
    const loadBtnHtml = skin.loaded
      ? `<button class="load-btn unload" data-skin-id="${escAttr(skin.id)}" data-action="unload">${t('common.unload')}</button>`
      : `<button class="load-btn" data-skin-id="${escAttr(skin.id)}" data-action="load">${t('common.load')}</button>`;
    const deleteSvg = `<svg width="12" height="12" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;

    // Preview thumbnail（缓存戳稳定：仅重新截取/版本更新时 bump，见 bumpPreview）
    let previewHtml = '';
    if (skin.preview) {
      const src = API.assetUrl(skin.preview) + '?v=' + (this.previewVersions.get(skin.id) || 0);
      // alt/src/data-* 一律 escAttr：皮肤包字段进双引号属性，esc() 不转义引号
      previewHtml = `<img class="skin-preview-img" src="${escAttr(src)}" alt="${escAttr(skin.name)}" loading="lazy">`;
      previewHtml += `<div class="skin-preview-placeholder" style="display:none"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg></div>`;
    } else {
      previewHtml = `<div class="skin-preview-placeholder"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg></div>`;
    }

    // 名称在卡片内容区（不进预览图）：压在图上的任何处理都被图片内容
    // 绑架（纯白底/透明捕获的预览上深条是一块补丁）。名称全宽双行夹
    // 截断，再长由 title 悬停兜底；版本芯片随行右侧（身份元数据归
    // 标题行）；作者不进卡片（编辑器页眉「作者：」是其正式席位）
    const name = dispName(skin);
    return `
      <div class="skin-card${selected}" data-skin-id="${escAttr(skin.id)}">
        <div class="skin-preview">
          ${previewHtml}
          <button class="skin-delete-btn${deleteDisabled}" data-skin-id="${escAttr(skin.id)}" data-skin-name="${escAttr(name)}" title="${deleteTitle}">
            ${deleteSvg}
          </button>
        </div>
        <div class="skin-card-content">
          <div class="skin-card-title">
            <span class="skin-card-name" title="${escAttr(name)}">${esc(name)}</span>
            ${skin.version ? `<span class="skin-card-ver">v${esc(skin.version)}</span>` : ''}
          </div>
          <div class="skin-card-footer">
            <span class="status-badge ${statusClass}"><span class="status-dot"></span>${statusText}</span>
            ${loadBtnHtml}
          </div>
        </div>
      </div>`;
  }

  async loadSkin(skinId) {
    try {
      await API.loadSkin(skinId);
      this.showToast(t('common.skinLoaded'), 'success');
      await this.refresh();
      // 编辑器联动刷新走后端 skin-loaded 事件单一路径（app.js），
      // 不在此直接触发——否则与事件路径并发双调 editor.load
    } catch (err) {
      this.showToast(t('common.loadFailed') + String(err), 'error');
    }
  }

  async unloadSkin(skinId) {
    try {
      await API.unloadSkin(skinId);
      this.showToast(t('common.skinUnloaded'), 'info');
      await this.refresh();
      // 编辑器联动同 loadSkin：由 skin-unloaded 事件路径覆盖
    } catch (err) {
      this.showToast(t('common.unloadFailed') + String(err), 'error');
    }
  }

  confirmDelete(skinId, skinName) {
    confirmDialog({
      title: t('list.confirmDeleteTitle'),
      bodyHtml: t('list.confirmDeleteBody', { name: `<strong>"${esc(skinName)}"</strong>` }),
      hint: t('list.confirmDeleteHint'),
      confirmText: t('common.delete'),
      danger: true,
      onConfirm: () => this.deleteSkin(skinId),
    });
  }

  async deleteSkin(skinId) {
    try {
      await API.removeSkin(skinId);
      this.showToast(t('list.deleted'), 'success');
      await this.refresh();
      if (this.selectedId === skinId) {
        this.selectedId = null;
        if (this.onSelect) this.onSelect(null);
      }
    } catch (err) {
      this.showToast(t('common.deleteFailed') + String(err), 'error');
    }
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}
