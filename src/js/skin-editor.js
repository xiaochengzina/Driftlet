/**
 * skin-editor.js — 皮肤配置面板
 *
 * 结构：顶部为皮肤信息；schema 非空时出现「窗口 / 皮肤设置」页签。
 * 「窗口」页 = 内置配置（外观/行为/位置大小/操作）；
 * 「皮肤设置」页 = 按 skin.json settings schema 自动生成的自定义控件，
 * 每种 type 对应一个渲染/绑定分支（新增控件需同步后端三处：
 * types.rs 枚举、loader.rs 兜底、commands.rs 校验）。
 * 皮肤可经 skin_set_setting 命令写回自己的设置值；本面板监听后端广播的
 * skin-setting-changed 事件，把对应控件原地刷新（见 _syncCustomControl）。
 */
import API from './api.js';
import showToast from './toast.js';
import { listen } from '@tauri-apps/api/event';
import { t, getLang } from './i18n.js';
import { esc, escAttr, dispName, confirmDialog } from './dom.js';
import { renderPermChipsHTML } from './perms.js';

// 调色板缺省预设色（skin.json 未声明 options 时）
// 12 个：与取色器、吸管同一行放下
const DEFAULT_PALETTE = [
  '#ff5a5a', '#ff8c42', '#ffb020', '#ffd666',
  '#34c759', '#13c2c2', '#4da3ff', '#2b6fe0',
  '#af52de', '#eb2f96', '#ffffff', '#000000',
];
// 任务列表内部上限 —— 刻意不向用户提示，到达后只是不再显示「添加」按钮
const MAX_TASKS = 500;

// 解析 #rrggbb / #rrggbbaa → { rgb: '#rrggbb', alpha: 0-100 }
function parseHexColor(v) {
  const s = String(v || '');
  if (/^#[0-9a-fA-F]{8}$/.test(s)) {
    return { rgb: s.slice(0, 7), alpha: Math.round(parseInt(s.slice(7), 16) / 255 * 100) };
  }
  if (/^#[0-9a-fA-F]{6}$/.test(s)) {
    return { rgb: s, alpha: 100 };
  }
  return { rgb: '#ffffff', alpha: 100 };
}

// 合成存储值：不透明时保持 #rrggbb（向后兼容），否则 #rrggbbaa
function composeHexColor(rgb, alphaPct) {
  if (alphaPct >= 100) return rgb;
  const a = Math.max(0, Math.min(255, Math.round(alphaPct * 255 / 100)));
  return rgb + a.toString(16).padStart(2, '0');
}

export default class SkinEditor {
  constructor(container) {
    this.container = container;
    this.skinId = null;
    this.detail = null;
    this.unlistenMoved = null;
    this.unlistenResized = null;
    this.unlistenSetting = null;
    this.activeTab = 'general';
    this._gen = 0; // 代际计数：并发 load 时只有最新一代允许落 UI（同 install-wizard.js）
  }

  async load(skinId) {
    const gen = ++this._gen;

    // Unlisten previous skin if any
    if (this.unlistenMoved) {
      this.unlistenMoved();
      this.unlistenMoved = null;
    }
    if (this.unlistenResized) {
      this.unlistenResized();
      this.unlistenResized = null;
    }
    if (this.unlistenSetting) {
      this.unlistenSetting();
      this.unlistenSetting = null;
    }

    // 切换皮肤时回到「窗口」页；同皮肤刷新 = 数据回灌，不播入场动画（防闪）
    const skinChanged = skinId !== this.skinId;
    if (skinChanged) this.activeTab = 'general';
    this.skinId = skinId;
    // 先取到局部变量，过代际校验后再写 this.detail：
    // 防止过期一代把旧皮肤的 detail 留在新 skinId 下
    let detail;
    try {
      detail = await API.getSkinDetail(skinId);
    } catch (err) {
      if (gen !== this._gen) return;
      this.detail = null; // 失败不落旧数据：render() 会走 clear()
      this.container.innerHTML = `<div class="panel-empty">${t('common.loadFailed')}${esc(String(err))}</div>`;
      return;
    }
    if (gen !== this._gen) return;
    this.detail = detail;
    // font 控件需要系统字体列表
    const schema = this.detail.settings_schema || [];
    if (schema.some(d => d.type === 'font')) {
      try {
        const fonts = await API.listSystemFonts();
        if (gen !== this._gen) return;
        this.systemFonts = fonts;
      } catch {
        // 失败也要过代际校验：旧代际的失败不得把新一代的 systemFonts 清成 []
        if (gen !== this._gen) return;
        this.systemFonts = [];
      }
    } else {
      this.systemFonts = null;
    }
    this.render(skinChanged);

    // Listen for position updates when the user drags the skin window.
    // The backend already persists the position on every Moved event
    // (debounced) — here we only refresh the X/Y inputs.
    // 每个 await listen 后都要过代际校验：过期一代拿到的解绑句柄立即调用，
    // 不得赋给 this.unlisten*——否则会被新一代覆盖，监听器永久泄漏。
    // listen 本身失败（IPC 异常）不得让 load 整体 reject：监听器缺失仅
    // 损失实时刷新，页面照常可用
    let unlistenMoved = null;
    try {
      unlistenMoved = await listen('skin-moved', (event) => {
        const { skinId: movedId, x, y } = event.payload;
        if (movedId === this.skinId) {
          this._updatePositionDisplay(x, y);
        }
      });
    } catch { /* 监听注册失败不阻断 */ }
    if (unlistenMoved) {
      if (gen !== this._gen) { unlistenMoved(); return; }
      this.unlistenMoved = unlistenMoved;
    }

    // Border-drag resize: backend persists (debounced) — refresh W/H inputs.
    let unlistenResized = null;
    try {
      unlistenResized = await listen('skin-resized', (event) => {
        const { skinId: resizedId, width, height } = event.payload;
        if (resizedId === this.skinId) {
          this._updateSizeDisplay(width, height);
        }
      });
    } catch { /* 监听注册失败不阻断 */ }
    if (unlistenResized) {
      if (gen !== this._gen) { unlistenResized(); return; }
      this.unlistenResized = unlistenResized;
    }

    // Skin-originated setting writes (skin_set_setting): backend persists —
    // here we only refresh the affected control in the open custom page.
    let unlistenSetting = null;
    try {
      unlistenSetting = await listen('skin-setting-changed', (event) => {
        const { skinId, key, value } = event.payload;
        if (skinId !== this.skinId || !this.detail) return;
        (this.detail.settings_values = this.detail.settings_values || {})[key] = value;
        this._syncCustomControl(key, value);
      });
    } catch { /* 监听注册失败不阻断 */ }
    if (unlistenSetting) {
      if (gen !== this._gen) { unlistenSetting(); return; }
      this.unlistenSetting = unlistenSetting;
    }
  }

  clear() {
    this._gen++; // 作废进行中的 load：其 await 返回后不得再落 UI
    this.skinId = null;
    this.detail = null;
    this.activeTab = 'general';
    if (this.unlistenMoved) {
      this.unlistenMoved();
      this.unlistenMoved = null;
    }
    if (this.unlistenResized) {
      this.unlistenResized();
      this.unlistenResized = null;
    }
    if (this.unlistenSetting) {
      this.unlistenSetting();
      this.unlistenSetting = null;
    }
    this.container.innerHTML = `
      <div class="panel-empty">
        <div class="panel-empty-inner">
          <div class="panel-empty-scene">
            <div class="panel-empty-icon">
              <svg width="34" height="34" viewBox="0 0 48 48" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="20" y="3" width="8" height="5.5" rx="1.4"/><path d="M21 8.5V13c0 2.4-3.2 2.8-3.2 6v20.5a4.5 4.5 0 0 0 4.5 4.5h3.4a4.5 4.5 0 0 0 4.5-4.5V19c0-3.2-3.2-3.6-3.2-6V8.5"/><rect x="20.6" y="24" width="6.8" height="9.5" rx="1.5"/><path d="M22.4 27.2h3.2M22.4 30.2h3.2"/></svg>
            </div>
            <svg class="drift-wake" width="196" height="24" viewBox="0 0 196 24" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"><path d="M4 12 C 19.7 5.3, 35.3 5.3, 51 12 S 82.3 18.7, 98 12 S 129.3 5.3, 145 12 S 176.3 18.7, 192 12" stroke-dasharray="4 5"/><circle cx="4" cy="12" r="2.2" fill="currentColor" stroke="none"/></svg>
          </div>
          <div class="panel-empty-title">${t('app.noSelection')}</div>
          <div class="panel-empty-hint">${t('app.noSelectionHint')}</div>
        </div>
      </div>`;
  }

  // Update X/Y fields in the config panel without re-rendering
  _updatePositionDisplay(x, y) {
    const elX = this.container.querySelector('#cfg-posx');
    const elY = this.container.querySelector('#cfg-posy');
    // 焦点保护：用户正在编辑该输入框时不冲掉进行中的内容
    if (elX && document.activeElement !== elX) elX.value = x;
    if (elY && document.activeElement !== elY) elY.value = y;
  }

  // Update W/H fields in the config panel without re-rendering
  _updateSizeDisplay(width, height) {
    const elW = this.container.querySelector('#cfg-width');
    const elH = this.container.querySelector('#cfg-height');
    if (elW && document.activeElement !== elW) elW.value = width;
    if (elH && document.activeElement !== elH) elH.value = height;
  }

  // Reflect a skin-originated setting write (skin_set_setting → backend
  // "skin-setting-changed") in the open custom-settings page, in place:
  // - list controls (tasklist / datetasklist / todolist): re-render rows —
  //   their bindings live on the container (event delegation), so replacing
  //   row nodes is safe;
  // - simple inputs (text / longtext / number / time / date / datetime /
  //   password / select / boolean / slider / font): assign value/checked —
  //   never replace the node, bindings are attached per element;
  // - composite controls (palette / chips / segments / timerange / weekdays):
  //   detail data only (already updated by the caller) — the next panel load
  //   picks the value up.  Skipped entirely while focus sits inside the
  //   control: the user is editing there, local content wins.
  _syncCustomControl(key, value) {
    const page = this.container.querySelector('.config-page[data-page="custom"]');
    if (!page) return;
    const ctrl = page.querySelector(`[data-key="${CSS.escape(key)}"]`);
    if (!ctrl || ctrl.contains(document.activeElement)) return;

    const rowsEl = ctrl.querySelector('.task-rows');
    if (rowsEl) {
      const items = Array.isArray(value) ? value : [];
      if (ctrl.classList.contains('cfg-tasklist')) {
        rowsEl.innerHTML = items.map(item => this.renderTaskRow(String(item))).join('');
      } else if (ctrl.classList.contains('cfg-todolist')) {
        rowsEl.innerHTML = items.map(it => this.renderTodoRow(String(it?.text ?? ''), !!(it && it.done))).join('');
      } else if (ctrl.classList.contains('cfg-datetasklist')) {
        rowsEl.innerHTML = items.map(it => this.renderDateTaskRow(String(it?.time ?? ''), String(it?.text ?? ''))).join('');
      } else {
        return;
      }
      const addBtn = ctrl.querySelector('.task-add');
      if (addBtn) addBtn.style.display = items.length >= MAX_TASKS ? 'none' : '';
      return;
    }

    // 赋值型：data-key 在 input/select/textarea 元素自身上
    const type = ctrl.dataset.type;
    if (type === 'boolean') {
      ctrl.checked = !!value;
    } else if (type === 'datetime') {
      ctrl.value = String(value || '').replace(' ', 'T');
    } else if (ctrl.classList.contains('cfg-custom-slider')) {
      ctrl.value = Number(value ?? 0);
      const display = ctrl.closest('.slider-row')?.querySelector('.slider-value');
      if (display) display.textContent = ctrl.value;
    } else if ('value' in ctrl) {
      ctrl.value = value ?? '';
    }
  }

  // animate 仅用于换肤/首次展示：同皮肤的数据回灌（开关、层级切换、
  // 后端事件同步）传 false 跳过入场动画，否则面板每次都闪一遍淡入
  render(animate = false) {
    if (!this.detail) return this.clear();

    const d = this.detail;
    const cfg = d.config;
    const hasCustom = (d.settings_schema || []).length > 0;
    const opacityPct = Math.round((cfg.opacity || 1) * 100);
    const generalHidden = hasCustom && this.activeTab !== 'general';
    const customHidden = this.activeTab !== 'custom';

    this.container.innerHTML = `
      <div class="config-panel" ${animate ? '' : 'style="animation:none"'}>
        <div class="cfg-header">
          <div class="cfg-header-main">
            <h2>${esc(dispName(d))}</h2>
            <div class="subtitle">${[d.author ? t('editor.byAuthor') + esc(d.author) : '', d.version ? `v${esc(d.version)}` : ''].filter(Boolean).join(' · ')}</div>
            <!-- 权限名称胶囊（perms.js 单一口源）：只列名称，颜色分级 -->
            ${renderPermChipsHTML(d.permissions)}
          </div>
          <span class="status-badge ${!d.loaded ? 'unloaded' : d.hidden ? 'hidden' : 'loaded'}"><span class="status-dot"></span>${!d.loaded ? t('common.unloaded') : d.hidden ? t('common.hidden') : t('common.running')}</span>
        </div>

        ${hasCustom ? `
        <div class="cfg-tabs">
          <button class="cfg-tab ${this.activeTab === 'general' ? 'active' : ''}" data-tab="general">${t('editor.tabWindow')}</button>
          <button class="cfg-tab ${this.activeTab === 'custom' ? 'active' : ''}" data-tab="custom">${t('editor.tabSkin')}</button>
        </div>` : ''}

        <div class="config-page" data-page="general" ${generalHidden ? 'style="display:none"' : ''}>
        <!-- 外观 -->
        <div class="config-section">
          <h3>${t('editor.appearance')}<span class="sec-en">APPEARANCE</span></h3>
          <div class="slider-row">
            <div class="slider-label">
              <span>${t('editor.opacity')}</span>
              <span class="slider-value" id="opacity-val">${opacityPct}%</span>
            </div>
            <input type="range" class="range-slider" id="cfg-opacity"
              min="10" max="100" value="${opacityPct}"
              ${!d.loaded ? 'disabled' : ''}>
          </div>
        </div>

        <!-- 行为 -->
        <div class="config-section">
          <h3>${t('editor.behavior')}<span class="sec-en">BEHAVIOR</span></h3>
          <div class="form-row">
            <div>
              <label>${t('editor.placement')}</label>
              <span class="hint">${t('editor.placementHint')}</span>
            </div>
            <div class="theme-options">
              <button class="theme-btn ${cfg.always_on_top ? 'active' : ''}" id="cfg-place-top"
                ${!d.loaded ? 'disabled' : ''}>${t('editor.placeTop')}</button>
              <button class="theme-btn ${cfg.on_desktop ? 'active' : ''}" id="cfg-place-desktop"
                ${!d.loaded ? 'disabled' : ''}>${t('editor.placeDesktop')}</button>
            </div>
          </div>

          <div class="form-row">
            <div>
              <label>${t('editor.lockPosition')}</label>
              <span class="hint">${t('editor.lockPositionHint')}</span>
            </div>
            <label class="toggle">
              <input type="checkbox" id="cfg-locked"
                ${cfg.position_locked ? 'checked' : ''}
                ${!d.loaded ? 'disabled' : ''}>
              <span class="slider"></span>
            </label>
          </div>

          <div class="form-row">
            <div>
              <label>${t('editor.clickThrough')}</label>
              <span class="hint">${t('editor.clickThroughHint')}</span>
            </div>
            <label class="toggle">
              <input type="checkbox" id="cfg-clickthrough"
                ${cfg.click_through ? 'checked' : ''}
                ${!d.loaded ? 'disabled' : ''}>
              <span class="slider"></span>
            </label>
          </div>
        </div>

        <!-- 位置和大小 -->
        <div class="config-section">
          <h3>${t('editor.geometry')}<span class="sec-en">GEOMETRY</span></h3>
          <div class="form-row">
            <div><label>${t('editor.position')}</label></div>
            <div class="num-inputs">
              <label>X <input type="number" id="cfg-posx"
                value="${cfg.x ?? 100}" ${!d.loaded ? 'disabled' : ''}></label>
              <label>Y <input type="number" id="cfg-posy"
                value="${cfg.y ?? 100}" ${!d.loaded ? 'disabled' : ''}></label>
            </div>
          </div>
          <div class="form-row">
            <div><label>${t('editor.size')}</label></div>
            <div class="num-inputs">
              <label>${t('editor.width')} <input type="number" id="cfg-width"
                value="${cfg.width}" min="50" max="4000"
                ${!d.loaded ? 'disabled' : ''}></label>
              <label>${t('editor.height')} <input type="number" id="cfg-height"
                value="${cfg.height}" min="50" max="4000"
                ${!d.loaded ? 'disabled' : ''}></label>
            </div>
          </div>

          <div class="form-row">
            <div>
              <label>${t('editor.resizable')}</label>
              <span class="hint">${t('editor.resizableHint')}</span>
            </div>
            <label class="toggle">
              <input type="checkbox" id="cfg-resizable"
                ${cfg.resizable ? 'checked' : ''}
                ${!d.loaded ? 'disabled' : ''}>
              <span class="slider"></span>
            </label>
          </div>

          <div class="slider-row">
            <div class="slider-label">
              <span>${t('editor.zoom')}</span>
              <span class="slider-value" id="zoom-val">${Math.round((cfg.zoom ?? 1) * 100)}%</span>
            </div>
            <span class="hint cfg-slider-hint">${t('editor.zoomHint')}</span>
            <input type="range" class="range-slider" id="cfg-zoom"
              min="50" max="200" step="5"
              value="${Math.round((cfg.zoom ?? 1) * 100)}"
              ${!d.loaded ? 'disabled' : ''}>
          </div>

          <div class="form-row">
            <div>
              <label>${t('editor.edgeSnap')}</label>
              <span class="hint">${t('editor.edgeSnapHint')}</span>
            </div>
            <label class="toggle">
              <input type="checkbox" id="cfg-edgesnap"
                ${cfg.edge_snap ? 'checked' : ''}
                ${!d.loaded ? 'disabled' : ''}>
              <span class="slider"></span>
            </label>
          </div>

          <div class="form-row">
            <div>
              <label>${t('editor.snapGap')}</label>
              <span class="hint">${t('editor.snapGapHint')}</span>
            </div>
            <div class="num-inputs">
              <label>px <input type="number" id="cfg-snapgap"
                value="${cfg.snap_gap ?? 0}" min="0" max="200"
                ${!d.loaded || !cfg.edge_snap ? 'disabled' : ''}></label>
            </div>
          </div>
        </div>

        <!-- 操作 -->
        <div class="config-section">
          <h3>${t('editor.actions')}<span class="sec-en">ACTIONS</span></h3>
          <div class="action-group">
            ${d.loaded
              ? `<button class="action-btn danger" id="btn-unload">${t('editor.unloadSkin')}</button>`
              : `<button class="action-btn primary" id="btn-load">${t('editor.loadSkin')}</button>`
            }
            ${d.loaded ? `<button class="action-btn" id="btn-reload">${t('editor.reload')}</button>` : ''}
            ${d.loaded ? `<button class="action-btn" id="btn-capture">${t('editor.capture')}</button>` : ''}
            ${d.loaded ? `<button class="action-btn" id="btn-onscreen">${t('editor.bringOnscreen')}</button>` : ''}
            <button class="action-btn" id="btn-openfolder">${t('editor.openFolder')}</button>
            <button class="action-btn danger" id="btn-reset">${t('editor.resetData')}</button>
            ${!d.loaded ? `<button class="action-btn danger" id="btn-delete">${t('common.deleteSkin')}</button>` : ''}
          </div>
        </div>
        </div>

        ${hasCustom ? `
        <div class="config-page" data-page="custom" ${customHidden ? 'style="display:none"' : ''}>
          <!-- 未加载时禁用整页控件：fieldset[disabled] 使内部所有表单控件不可交互（鼠标与键盘） -->
          <fieldset class="cfg-fieldset" ${d.loaded ? '' : 'disabled'}>
            ${this.renderCustomSettings()}
          </fieldset>
        </div>` : ''}
      </div>`;

    this.bindEvents();
  }

  // Render the custom-settings page.  Controls are grouped into cards by the
  // optional "group" field in skin.json: groups appear in first-seen order,
  // settings without a group form a leading untitled card (so a schema with
  // no groups at all renders exactly one plain card, as before).
  renderCustomSettings() {
    const schema = this.detail.settings_schema || [];
    const values = this.detail.settings_values || {};
    // 双语皮肤（skin.json 声明 "bilingual": true，作者侧声明、非用户选项）：
    // 管理器语言为英文时优先显示 *_en 文案，字段留空回退默认文案。
    // 分组键取当前语言的显示文案，保证同语言下同组归并
    const en = getLang() === 'en' && !!this.detail.bilingual;

    const groups = [];
    const byName = new Map();
    for (const def of schema) {
      const name = (en && def.group_en) ? def.group_en : (def.group || '');
      if (!byName.has(name)) {
        const g = { name, defs: [] };
        byName.set(name, g);
        groups.push(g);
      }
      byName.get(name).defs.push(def);
    }

    return groups.map(g => {
      const rows = g.defs.map(def => this.renderSettingRow(def, values, en)).join('');
      const title = g.name ? `<h3>${esc(g.name)}</h3>` : '';
      return `<div class="config-section">${title}${rows}</div>`;
    }).join('');
  }

  // Render one control row from its schema entry.
  // 皮肤未加载时整页控件由 render() 外层 fieldset[disabled] 统一禁用，
  // 与「窗口」页一致——未加载时仅「操作」分区保持可交互。
  renderSettingRow(def, values, en) {
      const key = escAttr(def.key);
      // 双语选取：en = 英文界面且皮肤声明 bilingual；*_en 留空回退默认文案
      const labelText = (en && def.label_en) ? def.label_en : (def.label || def.key);
      const descText = (en && def.description_en) ? def.description_en : def.description;
      const optText = (o) => (en && o.label_en) ? o.label_en : (o.label || o.value);
      const optTitle = (o) => (en && o.label_en) ? o.label_en : o.label;
      const label = esc(labelText);
      // 描述文字：skin.json 可选 "description"，与内置配置项的 hint 同款
      const hint = descText ? `<span class="hint">${esc(descText)}</span>` : '';
      const labelCell = `<div><label>${label}</label>${hint}</div>`;
      const value = values[def.key];
      let row = '';
      switch (def.type) {
        case 'boolean':
          row = `<div class="form-row">${labelCell}
            <label class="toggle">
              <input type="checkbox" class="cfg-custom" data-key="${key}" data-type="boolean"
                ${value ? 'checked' : ''}>
              <span class="slider"></span>
            </label></div>`;
          break;
        case 'number':
          row = `<div class="form-row">${labelCell}
            <input type="number" class="cfg-custom cfg-input" data-key="${key}" data-type="number"
              value="${Number(value ?? 0)}"
              ${def.min != null ? `min="${def.min}"` : ''} ${def.max != null ? `max="${def.max}"` : ''}
              ${def.step != null ? `step="${def.step}"` : ''}></div>`;
          break;
        case 'stepper': {
          // 数字步进器：−/＋ 按 step 增减；min/max 缺省 = 无界（与 number 的
          // 可选边界一致，不像 slider 有 0/100 兜底），到界即禁用对应按钮
          const num = Number(value ?? def.min ?? 0);
          const step = def.step ?? 1;
          const atMin = def.min != null && num <= def.min;
          const atMax = def.max != null && num >= def.max;
          row = `<div class="form-row">${labelCell}
            <div class="cfg-stepper" data-key="${key}"
              data-min="${def.min ?? ''}" data-max="${def.max ?? ''}" data-step="${step}">
              <button class="cfg-step-btn" data-dir="-1" ${atMin ? 'disabled' : ''}
                title="${t('editor.stepDecrease')}">−</button>
              <span class="cfg-step-val">${num}</span>
              <button class="cfg-step-btn" data-dir="1" ${atMax ? 'disabled' : ''}
                title="${t('editor.stepIncrease')}">+</button>
            </div></div>`;
          break;
        }
        case 'longtext':
          row = `<div class="form-row form-row-block">${labelCell}
            <textarea class="cfg-custom cfg-textarea" data-key="${key}" data-type="longtext"
              rows="4">${esc(String(value ?? ''))}</textarea></div>`;
          break;
        case 'time':
          row = `<div class="form-row">${labelCell}
            <input type="time" step="1" class="cfg-custom cfg-input" data-key="${key}" data-type="time"
              value="${escAttr(String(value || ''))}"></div>`;
          break;
        case 'date':
          row = `<div class="form-row">${labelCell}
            <input type="date" class="cfg-custom cfg-input" data-key="${key}" data-type="date"
              value="${escAttr(String(value || ''))}"></div>`;
          break;
        case 'datetime': {
          // 存储 "YYYY-MM-DD HH:MM:SS" ↔ 输入框 "YYYY-MM-DDTHH:MM:SS"
          const v = String(value || '').replace(' ', 'T');
          row = `<div class="form-row">${labelCell}
            <input type="datetime-local" step="1" class="cfg-custom cfg-input" data-key="${key}" data-type="datetime"
              value="${escAttr(v)}"></div>`;
          break;
        }
        case 'password':
          row = `<div class="form-row">${labelCell}
            <div class="cfg-password">
              <input type="password" class="cfg-custom cfg-input" data-key="${key}" data-type="password"
                value="${escAttr(String(value ?? ''))}" autocomplete="off">
              <button type="button" class="cfg-pw-toggle" tabindex="-1"
                data-title-show="${t('editor.showPassword')}" data-title-hide="${t('editor.hidePassword')}"
                title="${t('editor.showPassword')}">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
              </button>
            </div></div>`;
          break;
        // 文件/文件夹选择器：只读输入框显示路径 + 浏览（系统对话框由管理器
        // 后端弹——皮肤拿不到窗口句柄）+ 清除；值 = 绝对路径，空串 = 未选
        case 'file':
        case 'directory':
          row = `<div class="form-row">${labelCell}
            <div class="cfg-pick" data-key="${key}" data-mode="${def.type === 'directory' ? 'directory' : 'file'}"
              data-filters="${escAttr(JSON.stringify(def.filters || []))}">
              <input type="text" class="cfg-input cfg-pick-path" readonly
                value="${escAttr(String(value ?? ''))}" title="${escAttr(String(value ?? ''))}" placeholder="${t('editor.pickEmpty')}">
              <button type="button" class="cfg-pick-btn">${t('editor.browse')}</button>
              <button type="button" class="cfg-pick-clear" title="${t('editor.clearPath')}">×</button>
            </div></div>`;
          break;
        case 'select':
          row = `<div class="form-row">${labelCell}
            <select class="cfg-custom cfg-select" data-key="${key}" data-type="select">
              ${(def.options || []).map(o =>
                `<option value="${escAttr(o.value)}" ${o.value === value ? 'selected' : ''}>${esc(optText(o))}</option>`
              ).join('')}
            </select></div>`;
          break;
        case 'slider': {
          const min = def.min ?? 0;
          const max = def.max ?? 100;
          const step = def.step ?? 1;
          const num = Number(value ?? min);
          row = `<div class="slider-row">
            <div class="slider-label"><span>${label}</span><span class="slider-value">${num}</span></div>
            ${hint ? `<span class="hint cfg-slider-hint">${esc(descText)}</span>` : ''}
            <input type="range" class="range-slider cfg-custom-slider" data-key="${key}"
              min="${min}" max="${max}" step="${step}" value="${num}">
          </div>`;
          break;
        }
        case 'multiselect': {
          const selected = Array.isArray(value) ? value : [];
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-chips" data-key="${key}">
              ${(def.options || []).map(o =>
                `<button class="cfg-chip ${selected.includes(o.value) ? 'active' : ''}"
                  data-value="${escAttr(o.value)}">${esc(optText(o))}</button>`
              ).join('')}
            </div></div>`;
          break;
        }
        case 'radio':
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-segments" data-key="${key}">
              ${(def.options || []).map(o =>
                `<button class="cfg-segment ${o.value === value ? 'active' : ''}"
                  data-value="${escAttr(o.value)}">${esc(optText(o))}</button>`
              ).join('')}
            </div></div>`;
          break;
        case 'palette': {
          // 预设色值白名单校验：皮肤包 option 值会进 style 属性（管理器窗口
          // 是独立信任域）——escAttr 只防属性逃逸，防不了 `;` 追加声明
          // （任意 CSS 注入 / url() 外发请求）。非 #hex 值直接丢弃
          const SAFE_COLOR = /^#[0-9a-fA-F]{3}([0-9a-fA-F]{3})?([0-9a-fA-F]{2})?$/;
          const swatches = ((def.options || []).length
            ? def.options
            : DEFAULT_PALETTE.map(c => ({ value: c, label: null }))
          ).filter(o => SAFE_COLOR.test(String(o.value)));
          // 值格式 #rrggbb 或 #rrggbbaa（带透明度）
          const { rgb, alpha } = parseHexColor(String(value || '#ffffff'));
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-palette" data-key="${key}">
              <div class="cfg-palette-colors">
                ${swatches.map(o =>
                  `<button class="cfg-swatch ${parseHexColor(o.value).rgb.toLowerCase() === rgb.toLowerCase() ? 'active' : ''}"
                    data-value="${escAttr(o.value)}" style="background:${escAttr(o.value)}"
                    ${optTitle(o) ? `title="${escAttr(optTitle(o))}"` : ''}></button>`
                ).join('')}
                <input type="color" class="cfg-color cfg-palette-custom" value="${escAttr(rgb)}">
              </div>
              <div class="cfg-alpha-row">
                <input type="range" class="range-slider cfg-alpha" min="0" max="100" step="1"
                  value="${alpha}" style="background:linear-gradient(90deg, transparent, ${escAttr(rgb)})">
                <span class="cfg-alpha-val">${alpha}%</span>
              </div>
            </div></div>`;
          break;
        }
        case 'timerange': {
          const range = (value && typeof value === 'object') ? value : {};
          // 存储 "YYYY-MM-DD HH:MM:SS" ↔ 输入框 "YYYY-MM-DDTHH:MM:SS"
          const toInput = (s) => escAttr(String(s || '').replace(' ', 'T'));
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-timerange" data-key="${key}">
              <input type="datetime-local" step="1" class="cfg-input cfg-tr-start" value="${toInput(range.start)}">
              <span class="cfg-tr-sep">${t('editor.rangeSep')}</span>
              <input type="datetime-local" step="1" class="cfg-input cfg-tr-end" value="${toInput(range.end)}">
            </div></div>`;
          break;
        }
        case 'tasklist': {
          const items = Array.isArray(value) ? value : [];
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-tasklist" data-key="${key}">
              <div class="task-rows">
                ${items.map(item => this.renderTaskRow(String(item))).join('')}
              </div>
              <button class="action-btn task-add" ${items.length >= MAX_TASKS ? 'style="display:none"' : ''}>${t('editor.addItem')}</button>
            </div></div>`;
          break;
        }
        case 'todolist': {
          const items = Array.isArray(value) ? value : [];
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-todolist" data-key="${key}">
              <div class="task-rows">
                ${items.map(it => this.renderTodoRow(String(it?.text ?? ''), !!(it && it.done))).join('')}
              </div>
              <button class="action-btn task-add" ${items.length >= MAX_TASKS ? 'style="display:none"' : ''}>${t('editor.addItem')}</button>
            </div></div>`;
          break;
        }
        case 'weekdays': {
          // 固定周一至周日；复用 chips 交互与样式
          const DAYS = [['mon', t('editor.wdMon')], ['tue', t('editor.wdTue')], ['wed', t('editor.wdWed')], ['thu', t('editor.wdThu')],
                        ['fri', t('editor.wdFri')], ['sat', t('editor.wdSat')], ['sun', t('editor.wdSun')]];
          const selected = Array.isArray(value) ? value : [];
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-chips" data-key="${key}">
              ${DAYS.map(([v, l]) =>
                `<button class="cfg-chip ${selected.includes(v) ? 'active' : ''}" data-value="${v}">${l}</button>`
              ).join('')}
            </div></div>`;
          break;
        }
        case 'font': {
          const fonts = this.systemFonts || [];
          const current = String(value || '');
          // 当前字体不在系统列表里也保留为可选，避免显示跳变
          const options = current && !fonts.includes(current) ? [current, ...fonts] : fonts;
          row = `<div class="form-row">${labelCell}
            <select class="cfg-custom cfg-select" data-key="${key}" data-type="font">
              <option value="" ${current ? '' : 'selected'}>${t('editor.systemDefault')}</option>
              ${options.map(f =>
                `<option value="${escAttr(f)}" ${f === current ? 'selected' : ''}>${esc(f)}</option>`
              ).join('')}
            </select></div>`;
          break;
        }
        case 'datetasklist': {
          const items = Array.isArray(value) ? value : [];
          row = `<div class="form-row form-row-block">${labelCell}
            <div class="cfg-datetasklist" data-key="${key}">
              <div class="task-rows">
                ${items.map(it => this.renderDateTaskRow(String(it?.time ?? ''), String(it?.text ?? ''))).join('')}
              </div>
              <button class="action-btn task-add" ${items.length >= MAX_TASKS ? 'style="display:none"' : ''}>${t('editor.addItem')}</button>
            </div></div>`;
          break;
        }
        default: // text
          row = `<div class="form-row">${labelCell}
            <input type="text" class="cfg-custom cfg-input" data-key="${key}" data-type="text"
              value="${escAttr(String(value ?? ''))}"></div>`;
      }
      return row;
  }

  renderTaskRow(text) {
    return `<div class="task-row">
      <input type="text" class="cfg-input task-input" value="${escAttr(text)}">
      <button class="task-del" title="${t('common.delete')}">×</button>
    </div>`;
  }

  renderTodoRow(text, done) {
    return `<div class="task-row todo-row${done ? ' done' : ''}">
      <input type="checkbox" class="todo-check" ${done ? 'checked' : ''}>
      <input type="text" class="cfg-input task-input" value="${escAttr(text)}">
      <button class="task-del" title="${t('common.delete')}">×</button>
    </div>`;
  }

  renderDateTaskRow(time, text) {
    // 存储 "YYYY-MM-DD HH:MM:SS" ↔ 输入框 "YYYY-MM-DDTHH:MM:SS"
    return `<div class="task-row dt-row">
      <input type="datetime-local" step="1" class="cfg-input dt-time"
        value="${escAttr(time.replace(' ', 'T'))}">
      <input type="text" class="cfg-input task-input dt-text" placeholder="${t('editor.taskContent')}"
        value="${escAttr(text)}">
      <button class="task-del" title="${t('common.delete')}">×</button>
    </div>`;
  }

  bindEvents() {
    // 页签
    this.container.querySelectorAll('.cfg-tab').forEach(btn => {
      btn.addEventListener('click', () => {
        this.activeTab = btn.dataset.tab;
        this.container.querySelectorAll('.cfg-tab').forEach(b =>
          b.classList.toggle('active', b === btn));
        this.container.querySelectorAll('.config-page').forEach(page =>
          page.style.display = page.dataset.page === this.activeTab ? '' : 'none');
      });
    });

    // 不透明度滑块：拖动中只更新显示，松手才保存；成功静默（值已可见），仅失败提示
    const opacitySlider = this.container.querySelector('#cfg-opacity');
    const opacityVal = this.container.querySelector('#opacity-val');
    opacitySlider?.addEventListener('input', () => {
      opacityVal.textContent = opacitySlider.value + '%';
    });
    opacitySlider?.addEventListener('change', () => {
      const val = parseInt(opacitySlider.value) / 100;
      API.setOpacity(this.skinId, val)
        .catch(err => this.showToast(String(err), 'error'));
    });

    // 窗口放置双态（置顶/正常）：分段按钮二选一，后端两个方向都是原位
    // 翻转（不重建窗口、不打断皮肤运行时状态），也不发 loaded/unloaded
    // 事件。成功就地把选中态迁到被点按钮（整页 load 会重播面板入场动画，
    // 肉眼可见的闪；点当前档位也由此变纯 no-op）；失败才整页 load 回滚
    //（防停在非法态）。成功静默：选中态迁移即是反馈
    const bindPlacement = (id, placement) => {
      const btn = this.container.querySelector(id);
      if (!btn) return;
      btn.onclick = () => {
        API.setPlacement(this.skinId, placement)
          .then(() => {
            this.container.querySelectorAll('#cfg-place-top, #cfg-place-desktop')
              .forEach(b => b.classList.toggle('active', b === btn));
          })
          .catch(err => {
            this.showToast(String(err), 'error');
            this.load(this.skinId);
          });
      };
    };
    bindPlacement('#cfg-place-top', 'top');
    bindPlacement('#cfg-place-desktop', 'desktop');

    // 禁止拖动：开关态迁移即是反馈，成功静默
    this.bindToggle('cfg-locked', (on) => {
      API.setPositionLocked(this.skinId, on)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 鼠标穿透：开启时保留提示（皮肤将失去交互，需知会用户），关闭静默
    this.bindToggle('cfg-clickthrough', (on) => {
      API.setClickThrough(this.skinId, on)
        .then(() => { if (on) this.showToast(t('editor.clickThroughOn'), 'info'); })
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 拖拽调整大小
    this.bindToggle('cfg-resizable', (on) => {
      API.setResizable(this.skinId, on)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 缩放比例滑块：拖动中只更新显示，松手才保存（同不透明度）
    const zoomSlider = this.container.querySelector('#cfg-zoom');
    const zoomVal = this.container.querySelector('#zoom-val');
    zoomSlider?.addEventListener('input', () => {
      zoomVal.textContent = zoomSlider.value + '%';
    });
    zoomSlider?.addEventListener('change', () => {
      const val = parseInt(zoomSlider.value) / 100;
      API.setZoom(this.skinId, val)
        .catch(err => this.showToast(String(err), 'error'));
    });

    // 边缘吸附：开关切换时间距输入框随之启停
    this.bindToggle('cfg-edgesnap', (on) => {
      API.setEdgeSnap(this.skinId, on)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
      const gapInput = this.container.querySelector('#cfg-snapgap');
      if (gapInput) gapInput.disabled = !on;
    });

    // 吸附间距
    const snapGapInput = this.container.querySelector('#cfg-snapgap');
    snapGapInput?.addEventListener('change', () => {
      const gap = parseInt(snapGapInput.value);
      if (Number.isNaN(gap)) return;
      const clamped = Math.max(0, Math.min(200, gap));
      snapGapInput.value = clamped;
      API.setSnapGap(this.skinId, clamped)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 位置
    this.bindNumInput('cfg-posx', 'cfg-posy', (x, y) => {
      API.setPosition(this.skinId, x, y)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 大小
    this.bindNumInput('cfg-width', 'cfg-height', (w, h) => {
      API.setSize(this.skinId, w, h)
        .catch(err => {
          this.showToast(String(err), 'error');
          this.load(this.skinId);
        });
    });

    // 操作按钮 — also refresh the skin list so button states stay in sync
    // 点击即禁用按钮防连点并发；成功后整页 load 重建按钮，仅失败需恢复
    this.container.querySelector('#btn-load')?.addEventListener('click', async (e) => {
      const btn = e.currentTarget;
      btn.disabled = true;
      try {
        await API.loadSkin(this.skinId);
        this.showToast(t('common.skinLoaded'), 'success');
        await this.load(this.skinId);
        await window.__app?.skinList?.refresh();
      } catch (err) {
        btn.disabled = false;
        this.showToast(String(err), 'error');
      }
    });
    this.container.querySelector('#btn-unload')?.addEventListener('click', async (e) => {
      const btn = e.currentTarget;
      btn.disabled = true;
      try {
        await API.unloadSkin(this.skinId);
        this.showToast(t('common.skinUnloaded'), 'info');
        await this.load(this.skinId);
        await window.__app?.skinList?.refresh();
      } catch (err) {
        btn.disabled = false;
        this.showToast(String(err), 'error');
      }
    });
    this.container.querySelector('#btn-reload')?.addEventListener('click', async (e) => {
      // 防连点（与 load/unload 按钮同款约定）
      e.currentTarget.disabled = true;
      try {
        await API.reloadSkin(this.skinId);
        this.showToast(t('editor.reloaded'), 'success');
        await this.load(this.skinId);
        await window.__app?.skinList?.refresh();
      } catch (err) {
        this.showToast(String(err), 'error');
        e.currentTarget.disabled = false;
      }
    });
    this.container.querySelector('#btn-capture')?.addEventListener('click', () => {
      this.showToast(t('editor.capturing'), 'info');
      API.capturePreview(this.skinId)
        .then(async () => {
          this.showToast(t('editor.captureSaved'), 'success');
          // Refresh skin list to show the new preview thumbnail
          if (window.__app?.skinList) {
            window.__app.skinList.bumpPreview(this.skinId);
            await window.__app.skinList.refresh();
          }
        })
        .catch(err => this.showToast(t('editor.captureFailed') + String(err), 'error'));
    });
    this.container.querySelector('#btn-onscreen')?.addEventListener('click', () => {
      API.bringOnscreen(this.skinId)
        .then((moved) => this.showToast(t(moved ? 'editor.broughtBack' : 'editor.alreadyOnscreen'), moved ? 'success' : 'info'))
        .catch(err => this.showToast(String(err), 'error'));
    });
    this.container.querySelector('#btn-openfolder')?.addEventListener('click', () => {
      API.openSkinFolder(this.skinId)
        .then(() => this.showToast(t('app.folderOpened'), 'success'))
        .catch(err => this.showToast(String(err), 'error'));
    });
    this.container.querySelector('#btn-reset')?.addEventListener('click', () => {
      this.confirmReset();
    });
    // 删除皮肤（仅未加载时渲染此按钮）：复用皮肤列表的确认弹窗与删除流程，
    // 列表刷新、选中清空（→ 本配置页 clear）都在其回调里联动完成
    this.container.querySelector('#btn-delete')?.addEventListener('click', () => {
      window.__app?.skinList?.confirmDelete(this.skinId, dispName(this.detail) || this.skinId);
    });

    this.bindCustomSettings();
  }

  // 重置数据：清除该皮肤的全部持久化配置（「窗口」页与「皮肤设置」页），
  // 恢复 skin.json 默认值；皮肤已加载时后端会连带重载使其立即生效。
  confirmReset() {
    confirmDialog({
      title: t('editor.confirmResetTitle'),
      bodyHtml: t('editor.confirmResetBody', { name: `<strong>"${esc(dispName(this.detail) || this.skinId)}"</strong>` }),
      hint: t('editor.confirmResetHint'),
      confirmText: t('common.reset'),
      danger: true,
      onConfirm: async () => {
        try {
          await API.resetSkinConfig(this.skinId);
          this.showToast(t('editor.resetDone'), 'success');
          await this.load(this.skinId);
          await window.__app?.skinList?.refresh();
        } catch (err) {
          this.showToast(t('editor.resetFailed') + String(err), 'error');
        }
      },
    });
  }

  bindToggle(id, onChange) {
    this.container.querySelector('#' + id)?.addEventListener('change', function () {
      onChange(this.checked);
    });
  }

  // 保存单个自定义设置；成功静默（控件态已是反馈），失败时重新 load 回滚控件显示
  saveCustomSetting(key, value) {
    const skinId = this.skinId;
    return API.setSkinCustomSetting(skinId, key, value)
      .catch(err => {
        this.showToast(String(err), 'error');
        // skinId 可能已被 clear() 置空（页面切换后迟到失败）——空态页不得
        // 被刷成「加载失败」页
        if (this.skinId) this.load(this.skinId);
      });
  }

  bindCustomSettings() {
    // 简单输入类：boolean / number / text / longtext / time / date / datetime / password / select
    this.container.querySelectorAll('.cfg-custom').forEach(el => {
      el.addEventListener('change', () => {
        const key = el.dataset.key;
        let value;
        switch (el.dataset.type) {
          case 'boolean':
            value = el.checked;
            break;
          case 'number':
            value = parseFloat(el.value);
            if (Number.isNaN(value)) return;
            break;
          case 'datetime':
            // 输入框 "YYYY-MM-DDTHH:MM:SS" → 存储 "YYYY-MM-DD HH:MM:SS"
            value = el.value.replace('T', ' ');
            break;
          default: // text / longtext / time / date / password / color / select
            value = el.value;
        }
        this.saveCustomSetting(key, value);
      });
    });

    // 密码显示/隐藏切换（仅改 input.type，不触发保存）
    this.container.querySelectorAll('.cfg-password').forEach(pw => {
      const input = pw.querySelector('.cfg-input');
      const btn = pw.querySelector('.cfg-pw-toggle');
      btn?.addEventListener('click', () => {
        const showing = input.type === 'text';
        input.type = showing ? 'password' : 'text';
        btn.classList.toggle('active', !showing);
        btn.title = showing ? btn.dataset.titleShow : btn.dataset.titleHide;
      });
    });

    // 文件/文件夹选择器：浏览 → 管理器后端弹系统对话框 → 选中即保存；
    // 清除 → 置空串保存。取消选择不做任何事（控件保持原值）
    this.container.querySelectorAll('.cfg-pick').forEach(pick => {
      const key = pick.dataset.key;
      const input = pick.querySelector('.cfg-pick-path');
      let filters = [];
      try { filters = JSON.parse(pick.dataset.filters || '[]'); } catch { /* 防御 */ }
      pick.querySelector('.cfg-pick-btn')?.addEventListener('click', async () => {
        try {
          const path = await API.pickPath(pick.dataset.mode, filters);
          if (path) {
            input.value = path;
            this.saveCustomSetting(key, path);
          }
        } catch (err) {
          this.showToast(String(err), 'error');
        }
      });
      pick.querySelector('.cfg-pick-clear')?.addEventListener('click', () => {
        input.value = '';
        this.saveCustomSetting(key, '');
      });
    });

    // 滑动条：拖动中只更新显示，松手才保存
    this.container.querySelectorAll('.cfg-custom-slider').forEach(el => {
      const display = el.closest('.slider-row')?.querySelector('.slider-value');
      el.addEventListener('input', () => {
        if (display) display.textContent = el.value;
      });
      el.addEventListener('change', () => {
        const value = parseFloat(el.value);
        if (Number.isNaN(value)) return;
        this.saveCustomSetting(el.dataset.key, value);
      });
    });

    // 数字步进器：−/＋ 按 step 增减并夹取 min/max，点击即保存（控件态即时
    // 迁移；失败由 saveCustomSetting 整页 load 回滚）。小数位跟随 step，
    // 防 0.1+0.2 的浮点尾巴。
    this.container.querySelectorAll('.cfg-stepper').forEach(group => {
      const valEl = group.querySelector('.cfg-step-val');
      const decBtn = group.querySelector('[data-dir="-1"]');
      const incBtn = group.querySelector('[data-dir="1"]');
      const min = group.dataset.min === '' ? null : parseFloat(group.dataset.min);
      const max = group.dataset.max === '' ? null : parseFloat(group.dataset.max);
      const step = parseFloat(group.dataset.step) || 1;
      // 小数位推导要认指数记数法（step "1e-7" 按 split('.') 算得 0，
      // toFixed(0) 把每次步进都抹回整数——步进静默卡死）
      const decimals = (() => {
        const s = group.dataset.step;
        const m = /e-(\d+)$/i.exec(s);
        if (m) return Number(m[1]);
        return (s.split('.')[1] || '').length;
      })();
      const sync = (n) => {
        valEl.textContent = String(n);
        decBtn.disabled = min !== null && n <= min;
        incBtn.disabled = max !== null && n >= max;
      };
      group.querySelectorAll('.cfg-step-btn').forEach(btn => {
        btn.addEventListener('click', () => {
          let n = (parseFloat(valEl.textContent) || 0) + step * Number(btn.dataset.dir);
          if (min !== null) n = Math.max(min, n);
          if (max !== null) n = Math.min(max, n);
          n = parseFloat(n.toFixed(decimals));
          if (String(n) === valEl.textContent) return; // 已在边界
          sync(n);
          this.saveCustomSetting(group.dataset.key, n);
        });
      });
    });

    // 多选开关（chips）：点击切换选中态，整体保存数组
    this.container.querySelectorAll('.cfg-chips').forEach(group => {
      group.querySelectorAll('.cfg-chip').forEach(chip => {
        chip.addEventListener('click', () => {
          chip.classList.toggle('active');
          const values = [...group.querySelectorAll('.cfg-chip.active')]
            .map(c => c.dataset.value);
          this.saveCustomSetting(group.dataset.key, values);
        });
      });
    });

    // 互斥开关组（segments）：组内只保留一个 active
    this.container.querySelectorAll('.cfg-segments').forEach(group => {
      group.querySelectorAll('.cfg-segment').forEach(seg => {
        seg.addEventListener('click', () => {
          if (seg.classList.contains('active')) return;
          group.querySelectorAll('.cfg-segment').forEach(s =>
            s.classList.toggle('active', s === seg));
          this.saveCustomSetting(group.dataset.key, seg.dataset.value);
        });
      });
    });

    // 调色板：预设色块 / 自定义取色 / 透明度滑块
    // （屏幕取色用原生取色面板自带的吸管）
    // 透明度是独立轴——换色保留当前透明度；存储时 100% 写 #rrggbb，否则 #rrggbbaa
    this.container.querySelectorAll('.cfg-palette').forEach(pal => {
      const key = pal.dataset.key;
      const customInput = pal.querySelector('.cfg-palette-custom');
      const alphaEl = pal.querySelector('.cfg-alpha');
      const alphaVal = pal.querySelector('.cfg-alpha-val');
      const currentColor = () => composeHexColor(customInput.value, parseInt(alphaEl.value, 10));
      const syncSwatches = (rgb) => {
        pal.querySelectorAll('.cfg-swatch').forEach(sw =>
          sw.classList.toggle('active',
            parseHexColor(sw.dataset.value).rgb.toLowerCase() === rgb.toLowerCase()));
      };
      const syncGradient = () => {
        alphaEl.style.background = `linear-gradient(90deg, transparent, ${customInput.value})`;
      };
      pal.querySelectorAll('.cfg-swatch').forEach(sw => {
        sw.addEventListener('click', () => {
          const rgb = parseHexColor(sw.dataset.value).rgb;
          if (customInput) customInput.value = rgb;
          syncSwatches(rgb);
          syncGradient();
          this.saveCustomSetting(key, currentColor());
        });
      });
      customInput?.addEventListener('change', () => {
        syncSwatches(customInput.value);
        syncGradient();
        this.saveCustomSetting(key, currentColor());
      });
      alphaEl?.addEventListener('input', () => {
        alphaVal.textContent = alphaEl.value + '%';
      });
      alphaEl?.addEventListener('change', () => {
        this.saveCustomSetting(key, currentColor());
      });
    });

    // 时间范围：起止任一变化即保存；起晚于止时自动调换
    this.container.querySelectorAll('.cfg-timerange').forEach(tr => {
      const startEl = tr.querySelector('.cfg-tr-start');
      const endEl = tr.querySelector('.cfg-tr-end');
      const onChange = () => {
        // 输入框 "YYYY-MM-DDTHH:MM:SS" ↔ 存储 "YYYY-MM-DD HH:MM:SS"
        let start = (startEl.value || '').replace('T', ' ');
        let end = (endEl.value || '').replace('T', ' ');
        if (start && end && start > end) {
          [start, end] = [end, start];
          startEl.value = start.replace(' ', 'T');
          endEl.value = end.replace(' ', 'T');
          this.showToast(t('editor.timeSwapped'), 'info');
        }
        this.saveCustomSetting(tr.dataset.key, { start, end });
      };
      startEl?.addEventListener('change', onChange);
      endEl?.addEventListener('change', onChange);
    });

    // 任务列表：行 = 单个文本输入
    this.bindListControl('.cfg-tasklist', {
      onChange: (e, save) => {
        if (e.target.classList.contains('task-input')) save();
      },
      rowToItem: (r) => r.querySelector('.task-input').value,
      keepItem: (v) => v.trim() !== '',
      rowEmpty: (r) => r.querySelector('.task-input').value.trim() === '',
      renderEmptyRow: () => this.renderTaskRow(''),
      focusSelector: '.task-input',
    });

    // 待办任务列表：行 = 勾选框 + 文本；勾选切换 done 态
    this.bindListControl('.cfg-todolist', {
      onChange: (e, save) => {
        if (e.target.classList.contains('todo-check')) {
          e.target.closest('.task-row').classList.toggle('done', e.target.checked);
          save();
        } else if (e.target.classList.contains('task-input')) {
          save();
        }
      },
      rowToItem: (r) => ({
        text: r.querySelector('.task-input').value,
        done: r.querySelector('.todo-check').checked,
      }),
      keepItem: (it) => it.text.trim() !== '',
      rowEmpty: (r) => r.querySelector('.task-input').value.trim() === '',
      renderEmptyRow: () => this.renderTodoRow('', false),
      focusSelector: '.task-input',
    });

    // 日期任务列表：行 = 日期时间 + 任务文本
    this.bindListControl('.cfg-datetasklist', {
      onChange: (e, save) => {
        if (e.target.classList.contains('dt-time') ||
            e.target.classList.contains('dt-text')) save();
      },
      // 输入框 "YYYY-MM-DDTHH:MM:SS" → 存储 "YYYY-MM-DD HH:MM:SS"
      rowToItem: (r) => ({
        time: (r.querySelector('.dt-time').value || '').replace('T', ' '),
        text: r.querySelector('.dt-text').value,
      }),
      // 时间与内容都空的行不落盘
      keepItem: (it) => it.time.trim() !== '' || it.text.trim() !== '',
      rowEmpty: (r) => !r.querySelector('.dt-time').value &&
        r.querySelector('.dt-text').value.trim() === '',
      renderEmptyRow: () => this.renderDateTaskRow('', ''),
      focusSelector: '.dt-text',
    });
  }

  // tasklist / todolist / datetasklist 的公共绑定：行编辑 / 删除 / 添加
  // （事件委托，动态行也生效）。三者仅 change 目标、行序列化、空行判定、
  // 新行渲染与焦点目标不同，由 opts 注入；save 快照与 blur→change→save /
  // click 竞态防护与合并前的三个独立实现逐点一致。
  bindListControl(selector, opts) {
    const { onChange, rowToItem, keepItem, rowEmpty, renderEmptyRow, focusSelector } = opts;
    this.container.querySelectorAll(selector).forEach(list => {
      const key = list.dataset.key;
      const rowsEl = list.querySelector('.task-rows');
      const addBtn = list.querySelector('.task-add');
      const updateAddBtn = () => {
        if (addBtn) addBtn.style.display =
          rowsEl.querySelectorAll('.task-row').length >= MAX_TASKS ? 'none' : '';
      };
      const save = () => {
        // 快照保存时刻的行：add 按钮在 API 返回前新加的空行不在快照内，
        // 防止下方清理把刚添加的行误删（blur→change→save 与 click 的竞态）
        const rowsAtSave = [...rowsEl.querySelectorAll('.task-row')];
        const items = rowsAtSave.map(rowToItem).filter(keepItem);
        this.saveCustomSetting(key, items).then(() => {
          // 后端会剔除空行 —— 同步移除快照内的本地空行
          rowsAtSave.forEach(row => {
            if (row.isConnected && rowEmpty(row)) row.remove();
          });
          updateAddBtn();
        });
      };
      list.addEventListener('change', (e) => onChange(e, save));
      list.addEventListener('click', (e) => {
        if (e.target.classList.contains('task-del')) {
          e.target.closest('.task-row').remove();
          save();
        } else if (e.target.classList.contains('task-add')) {
          // 只加本地行，输入失焦（change）后才落盘
          rowsEl.insertAdjacentHTML('beforeend', renderEmptyRow());
          updateAddBtn();
          rowsEl.lastElementChild.querySelector(focusSelector).focus();
        }
      });
    });
  }

  bindNumInput(idX, idY, onChange) {
    const elX = this.container.querySelector('#' + idX);
    const elY = this.container.querySelector('#' + idY);
    const handler = () => {
      // 清空失焦不得兜底成 0——用户删掉内容只是还没输完，写成 0 会把
      // 皮肤移到 (0,0)/尺寸设 0（snapgap 处理器对 NaN 是 return，此处曾漏）
      const x = parseInt(elX?.value, 10);
      const y = parseInt(elY?.value, 10);
      if (Number.isNaN(x) || Number.isNaN(y)) return;
      onChange(x, y);
    };
    elX?.addEventListener('change', handler);
    elY?.addEventListener('change', handler);
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}
