/**
 * skin-list.js — 皮肤列表组件
 */
import API from './api.js';
import showToast from './toast.js';
import { t } from './i18n.js';
import { esc, escAttr, dispName, confirmDialog } from './dom.js';

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

  render() {
    const q = this.query.trim().toLowerCase();
    // 匹配显示名（当前语言）、id、作者，大小写不敏感；列表规模小，
    // 每次输入即时过滤即可，无需防抖
    const visible = q
      ? this.skins.filter(s => `${dispName(s) || ''} ${s.id} ${s.author || ''}`.toLowerCase().includes(q))
      : this.skins;

    // 侧栏标题旁的数量徽标：过滤中且命中数 ≠ 总数时显示「命中/总数」，
    // 否则显示总数（无皮肤时置空字符串，:empty 整体收起）
    const countEl = document.getElementById('skin-count');
    if (countEl) {
      countEl.textContent = (q && visible.length !== this.skins.length)
        ? `${visible.length}/${this.skins.length}`
        : (this.skins.length ? String(this.skins.length) : '');
    }
    if (this.skins.length === 0) {
      this.container.innerHTML = `
        <div class="list-empty">
          <div class="list-empty-icon">
            <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>
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
            <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.5" y2="16.5"/><line x1="8.5" y1="8.5" x2="13.5" y2="13.5"/><line x1="13.5" y1="8.5" x2="8.5" y2="13.5"/></svg>
          </div>
          <p>${t('list.noResults')}</p>
          <p class="list-empty-hint">${t('list.noResultsHint')}</p>
        </div>`;
      return;
    }

    this.container.innerHTML = visible.map(skin => this.renderCard(skin)).join('');

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

  renderCard(skin) {
    const selected = skin.id === this.selectedId ? ' selected' : '';
    // 状态三档：未加载（灰）/ 已加载但窗口不可见 = 已隐藏（黄）/ 运行中（绿）。
    // hidden 由后端按真实窗口可见性（is_visible）下发，不是热键簿记
    const statusClass = !skin.loaded ? 'unloaded' : skin.hidden ? 'hidden' : 'loaded';
    const statusText = !skin.loaded ? t('common.unloaded') : skin.hidden ? t('common.hidden') : t('common.running');
    const deleteTitle = skin.loaded ? t('list.unloadBeforeDelete') : t('common.deleteSkin');
    const deleteDisabled = skin.loaded ? ' disabled' : '';

    // Preview thumbnail（缓存戳稳定：仅重新截取/版本更新时 bump，见 bumpPreview）
    let previewHtml = '';
    if (skin.preview) {
      const src = API.assetUrl(skin.preview) + '?v=' + (this.previewVersions.get(skin.id) || 0);
      // alt/src/data-* 一律 escAttr：皮肤包字段进双引号属性，esc() 不转义引号
      previewHtml = `<img class="skin-preview-img" src="${escAttr(src)}" alt="${escAttr(skin.name)}" loading="lazy">`;
      previewHtml += `<div class="skin-preview-placeholder" style="display:none"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg></div>`;
    } else {
      previewHtml = `<div class="skin-preview-placeholder"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg></div>`;
    }

    const meta = [skin.author, skin.version ? 'v' + skin.version : null]
      .filter(Boolean).join(' · ');

    return `
      <div class="skin-card${selected}" data-skin-id="${escAttr(skin.id)}">
        <div class="skin-preview">
          ${previewHtml}
          <button class="skin-delete-btn${deleteDisabled}" data-skin-id="${escAttr(skin.id)}" data-skin-name="${escAttr(dispName(skin))}" title="${deleteTitle}">
            <svg width="12" height="12" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          </button>
        </div>
        <div class="skin-card-content">
          <div class="skin-card-header">
            <span class="skin-card-name">${esc(dispName(skin))}</span>
            <span class="status-badge ${statusClass}"><span class="status-dot"></span>${statusText}</span>
          </div>
          <div class="skin-card-footer">
            <span class="skin-card-meta">${esc(meta)}</span>
            ${skin.loaded
              ? `<button class="load-btn unload" data-skin-id="${escAttr(skin.id)}" data-action="unload">${t('common.unload')}</button>`
              : `<button class="load-btn" data-skin-id="${escAttr(skin.id)}" data-action="load">${t('common.load')}</button>`
            }
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
