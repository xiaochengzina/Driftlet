/**
 * skin-list.js — 皮肤列表组件
 */
import API from './api.js';
import showToast from './toast.js';
import { t, getLang } from './i18n.js';

// 双语皮肤（skin.json 声明 bilingual）：英文界面优先显示 *_en 文案，
// 字段留空回退默认文案。与 skin-editor.js 的 schema 文案同一选取规则
function dispName(skin) {
  return (getLang() === 'en' && skin.bilingual && skin.name_en) || skin.name;
}

export default class SkinList {
  constructor(container, { onSelect } = {}) {
    this.container = container;
    this.onSelect = onSelect;
    this.skins = [];
    this.selectedId = null;
    // 预览图 URL 的缓存戳：值不变则 WebView2 直接命中缓存，避免每次
    // render 全量重解码；重新截取或皮肤版本更新时才 bump。
    this.previewVersions = new Map();
    this._lastVersions = new Map();
  }

  bumpPreview(skinId) {
    this.previewVersions.set(skinId, (this.previewVersions.get(skinId) || 0) + 1);
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
    } catch (err) {
      this.skins = [];
      this.showToast(t('list.loadFailed') + String(err), 'error');
    }
    this.render();
    return this.skins;
  }

  select(skinId) {
    this.selectedId = skinId;
    this.render();
    if (this.onSelect) this.onSelect(skinId);
  }

  render() {
    // 侧栏标题旁的数量徽标（有皮肤时才显示数字）
    const countEl = document.getElementById('skin-count');
    if (countEl) countEl.textContent = this.skins.length ? String(this.skins.length) : '';
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

    this.container.innerHTML = this.skins.map(skin => this.renderCard(skin)).join('');

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
    const statusClass = skin.loaded ? 'loaded' : 'unloaded';
    const statusText = skin.loaded ? t('common.running') : t('common.unloaded');
    const deleteTitle = skin.loaded ? t('list.unloadBeforeDelete') : t('common.deleteSkin');
    const deleteDisabled = skin.loaded ? ' disabled' : '';

    // Preview thumbnail（缓存戳稳定：仅重新截取/版本更新时 bump，见 bumpPreview）
    let previewHtml = '';
    if (skin.preview) {
      const src = API.assetUrl(skin.preview) + '?v=' + (this.previewVersions.get(skin.id) || 0);
      // alt/src/data-* 一律 escAttr：皮肤包字段进双引号属性，esc() 不转义引号
      previewHtml = `<img class="skin-preview-img" src="${this.escAttr(src)}" alt="${this.escAttr(skin.name)}" loading="lazy">`;
      previewHtml += `<div class="skin-preview-placeholder" style="display:none"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg></div>`;
    } else {
      previewHtml = `<div class="skin-preview-placeholder"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><path d="M21 15l-5-5L5 21"/></svg></div>`;
    }

    const meta = [skin.author, skin.version ? 'v' + skin.version : null]
      .filter(Boolean).join(' · ');

    return `
      <div class="skin-card${selected}" data-skin-id="${this.escAttr(skin.id)}">
        <div class="skin-preview">
          ${previewHtml}
          <button class="skin-delete-btn${deleteDisabled}" data-skin-id="${this.escAttr(skin.id)}" data-skin-name="${this.escAttr(dispName(skin))}" title="${deleteTitle}">
            <svg width="12" height="12" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          </button>
        </div>
        <div class="skin-card-content">
          <div class="skin-card-header">
            <span class="skin-card-name">${this.esc(dispName(skin))}</span>
            <span class="status-badge ${statusClass}"><span class="status-dot"></span>${statusText}</span>
          </div>
          <div class="skin-card-footer">
            <span class="skin-card-meta">${this.esc(meta)}</span>
            ${skin.loaded
              ? `<button class="load-btn unload" data-skin-id="${this.escAttr(skin.id)}" data-action="unload">${t('common.unload')}</button>`
              : `<button class="load-btn" data-skin-id="${this.escAttr(skin.id)}" data-action="load">${t('common.load')}</button>`
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
    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = `
      <div class="confirm-dialog">
        <div class="confirm-icon danger"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg></div>
        <h3>${t('list.confirmDeleteTitle')}</h3>
        <p>${t('list.confirmDeleteBody', { name: `<strong>"${this.esc(skinName)}"</strong>` })}</p>
        <p class="confirm-hint">${t('list.confirmDeleteHint')}</p>
        <div class="confirm-buttons">
          <button class="confirm-btn cancel">${t('common.cancel')}</button>
          <button class="confirm-btn danger">${t('common.delete')}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);

    // Esc 关闭 + 初始焦点落在「取消」（危险操作，焦点不放确认键）；
    // 关闭时摘除文档级监听，避免泄漏
    const onKey = (e) => { if (e.key === 'Escape') close(); };
    const close = () => {
      document.removeEventListener('keydown', onKey);
      overlay.remove();
    };
    document.addEventListener('keydown', onKey);

    overlay.querySelector('.confirm-btn.cancel').onclick = close;
    overlay.querySelector('.confirm-btn.danger').onclick = async () => {
      close();
      await this.deleteSkin(skinId);
    };
    overlay.addEventListener('click', (e) => {
      if (e.target === overlay) close();
    });
    overlay.querySelector('.confirm-btn.cancel').focus();
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

  esc(str) {
    const div = document.createElement('div');
    div.textContent = String(str ?? '');
    return div.innerHTML;
  }

  // Escape for double-quoted HTML attribute values (esc() leaves quotes intact)
  escAttr(str) {
    return this.esc(str).replace(/"/g, '&quot;');
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}
