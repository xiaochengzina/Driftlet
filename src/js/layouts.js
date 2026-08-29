/**
 * layouts.js — 布局方案面板（保存/应用/覆盖/重命名/删除命名布局）
 *
 * 布局 = 加载集 + 各皮肤几何（位置/尺寸）+ 显隐（语义边界：皮肤行为配置
 * 不归布局管，仍归各皮肤 skin_settings 自管）。浮层复用 confirm 系弹层
 * 骨架（bindEsc / closeOnMaskClick 小工具 + confirm-overlay /
 * confirm-dialog 视觉类——dom.js 注释指明的特殊弹窗模式，与分组编辑
 * 对话框同款）。
 *
 * 面板结构：页眉（display 字体 h2 眉题 + 一句话布局语义副题——页眉/页脚
 * 贴近设置页语言，中间保存区与清单块维持原语言）→ 保存区（名称输入 +
 * 存档主钮，无分区标签——按钮自带说明，评审定案多余）→ 方案清单（名称 + 皮肤数弱签 + 应用主钮 + 覆盖/
 * 重命名/删除幽灵图标钮）→ 尾注（计数 + 托盘菜单提示）+ 全宽关闭钮
 * （settings-close 同款）。图标统一官方 Feather 细线条语系（与组菜单 /
 * 管理器工具栏同族，路径逐字照抄勿手绘改动）。关闭出口 = 底部「关闭」/
 * 点遮罩/Esc。
 */
import API from './api.js';
import showToast from './toast.js';
import { t } from './i18n.js';
import { esc, escAttr, confirmDialog, bindEsc, closeOnMaskClick } from './dom.js';

// Feather 细线条图标（stroke 语系与组菜单 / 工具栏同族；颜色全部
// currentColor 随按钮状态走）
const ICON_GRID = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/></svg>';
const ICON_SAVE = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/><polyline points="17 21 17 13 7 13 7 21"/><polyline points="7 3 7 8 15 8"/></svg>';
const ICON_REFRESH = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23 4 23 10 17 10"/><polyline points="1 20 1 14 7 14"/><path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10"/><path d="M1 14l4.64 4.36A9 9 0 0 0 20.49 15"/></svg>';
const ICON_PENCIL = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/></svg>';
const ICON_TRASH = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><line x1="10" y1="11" x2="10" y2="17"/><line x1="14" y1="11" x2="14" y2="17"/></svg>';
const ICON_CHECK = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>';
const ICON_X = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>';

export default class LayoutPanel {
  constructor({ onApplied } = {}) {
    // 应用方案成功后的回调（app.js 用于刷新皮肤列表与徽标）
    this.onApplied = onApplied;
    this.overlay = null;
    this.layouts = [];
    this._unbindEsc = null;
    this._renamingId = null;
  }

  close() {
    this._unbindEsc?.();
    this._unbindEsc = null;
    this.overlay?.remove();
    this.overlay = null;
    this._renamingId = null;
    this._noAnim = false;
  }

  async open() {
    this.close();
    const config = await API.getAppConfig().catch(() => null);
    this.layouts = Array.isArray(config?.layouts) ? config.layouts : [];
    this.render();
  }

  // 面板内重绘（保名称输入框内容；方案数组本地为源——每次操作后后端
  // 已落盘，失败时从 config 重拉对齐）。重绘换 DOM 但属「已在场」更新：
  // 标记 no-anim 让 render 摘掉遮罩淡入——否则每次操作都重演入场动画，
  // 视觉上即「窗口重新打开闪一下」（实机反馈）
  rerender(keepName = true) {
    const nameValue = keepName ? this.overlay?.querySelector('#layout-name-input')?.value : '';
    this.overlay?.remove();
    this._unbindEsc?.();
    this._noAnim = true;
    this.render();
    if (nameValue) {
      const input = this.overlay?.querySelector('#layout-name-input');
      if (input) input.value = nameValue;
    }
  }

  async resyncFromConfig() {
    const config = await API.getAppConfig().catch(() => null);
    this.layouts = Array.isArray(config?.layouts) ? config.layouts : this.layouts;
  }

  render() {
    const overlay = document.createElement('div');
    // 首开淡入照常；面板内 rerender 置 _noAnim，遮罩动画不再重演
    overlay.className = 'confirm-overlay' + (this._noAnim ? ' no-anim' : '');
    const rows = this.layouts.length === 0
      ? `<div class="layout-empty"><span class="layout-empty-icon">${ICON_GRID}</span><span class="layout-empty-text">${t('layout.empty')}</span></div>`
      : this.layouts.map(l => this.renderRow(l)).join('');
    // 尾注：有方案时计数在前（mono 数字），托盘发现路径提示恒在
    const footHint = (this.layouts.length
      ? t('layout.footerCount', { count: this.layouts.length }) + ' · '
      : '') + t('layout.footerHint');
    overlay.innerHTML = `
      <div class="confirm-dialog wide layout-panel">
        <h2>${t('layout.title')}</h2>
        <div class="layout-sub">${t('layout.subtitle')}</div>
        <div class="layout-save">
          <input class="group-edit-name" id="layout-name-input" maxlength="64"
                 placeholder="${t('layout.namePlaceholder')}" spellcheck="false" autocomplete="off">
          <button class="confirm-btn primary" id="layout-save-btn">${ICON_SAVE}<span>${t('layout.saveCurrent')}</span></button>
        </div>
        <div class="layout-list">${rows}</div>
        <div class="layout-foot-hint">${footHint}</div>
        <button class="settings-close">${t('common.close')}</button>
      </div>`;
    document.body.appendChild(overlay);
    this.overlay = overlay;
    this._unbindEsc = bindEsc(() => this.close());
    closeOnMaskClick(overlay, () => this.close());
    overlay.querySelector('.settings-close').onclick = () => this.close();

    const nameInput = overlay.querySelector('#layout-name-input');
    const saveBtn = overlay.querySelector('#layout-save-btn');
    const save = () => this.saveCurrent(nameInput.value, saveBtn);
    saveBtn.onclick = save;
    nameInput.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); save(); }
    });
    nameInput.focus();

    const list = overlay.querySelector('.layout-list');
    // 行操作（事件委托）
    list.addEventListener('click', (e) => {
      const btn = e.target.closest('[data-act]');
      if (!btn) return;
      const id = btn.closest('.layout-row')?.dataset.layoutId;
      if (!id) return;
      if (btn.dataset.act === 'apply') this.apply(id, btn);
      else if (btn.dataset.act === 'overwrite') this.overwrite(id, btn);
      else if (btn.dataset.act === 'rename') this.startRename(id);
      else if (btn.dataset.act === 'delete') this.confirmDelete(id);
      else if (btn.dataset.act === 'rename-save') {
        const input = btn.closest('.layout-row')?.querySelector('.layout-rename-input');
        this.commitRename(id, input?.value ?? '');
      }
      else if (btn.dataset.act === 'rename-cancel') {
        this._renamingId = null;
        this.rerender();
      }
    });
    // 行内重命名：Enter 提交 / Esc 取消 / blur 提交（与分组编辑同键位语义）
    list.addEventListener('keydown', (e) => {
      if (!e.target.classList.contains('layout-rename-input')) return;
      const id = e.target.closest('.layout-row')?.dataset.layoutId;
      if (e.key === 'Enter') { e.preventDefault(); this.commitRename(id, e.target.value); }
      else if (e.key === 'Escape') { e.preventDefault(); this._renamingId = null; this.rerender(); }
    });
    list.addEventListener('focusout', (e) => {
      if (e.target.classList.contains('layout-rename-input')) {
        // 焦点移向 ✓/✕ 时交给按钮的 click 处理——否则 blur 先触发一次
        // 提交，点 ✕ 取消也会先把名字存上（顺序坑）
        if (e.relatedTarget?.closest?.('.layout-icon-btn')) return;
        const id = e.target.closest('.layout-row')?.dataset.layoutId;
        if (id && this._renamingId === id) this.commitRename(id, e.target.value);
      }
    });
  }

  renderRow(l) {
    if (this._renamingId === l.id) {
      // 重命名态：输入框 + 显式 ✓/✕（裸输入框无可见提交路径——Enter/blur
      // 提交、Esc 取消是盲操作，实机评审定案补两钮；图标用官方 check/x）
      return `<div class="layout-row renaming" data-layout-id="${escAttr(l.id)}">
        <input class="group-edit-name layout-rename-input" value="${escAttr(l.name)}" maxlength="64" spellcheck="false" autocomplete="off">
        <button class="layout-icon-btn" data-act="rename-save" title="${t('common.save')}">${ICON_CHECK}</button>
        <button class="layout-icon-btn" data-act="rename-cancel" title="${t('common.cancel')}">${ICON_X}</button>
      </div>`;
    }
    // 覆盖/重命名/删除收敛为幽灵图标钮——四枚文字钮会把行宽吃光，
    // 弱操作不该与「应用」主钮同形同级
    return `<div class="layout-row" data-layout-id="${escAttr(l.id)}">
      <span class="layout-row-name" title="${escAttr(l.name)}">${esc(l.name)}</span>
      <span class="layout-row-count">${t('layout.skinCount', { count: Object.keys(l.skins || {}).length })}</span>
      <span class="layout-row-actions">
        <button class="action-btn primary layout-act" data-act="apply">${t('layout.apply')}</button>
        <button class="layout-icon-btn" data-act="overwrite" title="${t('layout.overwriteTip')}">${ICON_REFRESH}</button>
        <button class="layout-icon-btn" data-act="rename" title="${t('layout.rename')}">${ICON_PENCIL}</button>
        <button class="layout-icon-btn danger" data-act="delete" title="${t('common.delete')}">${ICON_TRASH}</button>
      </span>
    </div>`;
  }

  // 操作期忙态：禁用按钮防连点；文字钮（应用/保存）换行内文案，
  // 图标钮只禁用。成功后 rerender 重建 DOM，失败走 finally 还原——
  // 以 isConnected 判别（按钮仍在面板里才还原文案/禁用态）
  _setBusy(btn, text) {
    if (!btn) return;
    if (text) {
      const label = btn.querySelector('span') || btn;
      btn.dataset.origText = label.textContent;
      label.textContent = text;
    }
    btn.disabled = true;
  }

  _restoreBtn(btn) {
    if (!btn?.isConnected) return;
    if (btn.dataset.origText != null) {
      const label = btn.querySelector('span') || btn;
      label.textContent = btn.dataset.origText;
      delete btn.dataset.origText;
    }
    btn.disabled = false;
  }

  async saveCurrent(name, btn) {
    const trimmed = (name || '').trim();
    if (!trimmed) {
      this.showToast(t('layout.nameRequired'), 'error');
      this.overlay?.querySelector('#layout-name-input')?.focus();
      return;
    }
    this._setBusy(btn, t('layout.saving'));
    try {
      const preset = await API.captureLayout(trimmed, null);
      this.layouts.push(preset);
      this.showToast(t('layout.saved', { name: trimmed }), 'success');
      this.rerender(false);
    } catch (err) {
      this.showToast(String(err), 'error');
    } finally {
      this._restoreBtn(btn);
    }
  }

  async apply(id, btn) {
    const layout = this.layouts.find(l => l.id === id);
    if (!layout) return;
    this._setBusy(btn, t('layout.applying'));
    try {
      const res = await API.applyLayout(id);
      this.showToast(t('layout.applied', { name: layout.name, count: res.applied }), 'success');
      if (res.skipped?.length) {
        this.showToast(t('layout.appliedSkipped', { count: res.skipped.length, ids: res.skipped.join(', ') }), 'info');
      }
      if (this.onApplied) await this.onApplied();
    } catch (err) {
      this.showToast(String(err), 'error');
    } finally {
      this._restoreBtn(btn);
    }
  }

  async overwrite(id, btn) {
    const layout = this.layouts.find(l => l.id === id);
    if (!layout) return;
    this._setBusy(btn, null);
    try {
      const preset = await API.captureLayout(layout.name, id);
      const idx = this.layouts.findIndex(l => l.id === id);
      if (idx >= 0) this.layouts[idx] = preset;
      this.showToast(t('layout.updated', { name: layout.name }), 'success');
      this.rerender();
    } catch (err) {
      this.showToast(String(err), 'error');
    } finally {
      this._restoreBtn(btn);
    }
  }

  startRename(id) {
    this._renamingId = id;
    this.rerender();
    const input = this.overlay?.querySelector('.layout-rename-input');
    if (input) { input.focus(); input.select(); }
  }

  // 提交重命名：空名/未变 = 取消；否则本地更新 + 整体写回（set_layouts 同
  // 分组哲学：前端持有完整状态，任何组操作后整体回写）
  async commitRename(id, name) {
    const layout = this.layouts.find(l => l.id === id);
    const trimmed = (name || '').trim();
    this._renamingId = null;
    if (!layout || !trimmed || trimmed === layout.name) { this.rerender(); return; }
    const prev = layout.name;
    layout.name = trimmed;
    try {
      await API.setLayouts(this.layouts);
      this.showToast(t('layout.renamed'), 'success');
    } catch (err) {
      layout.name = prev;
      this.showToast(t('common.setFailed') + String(err), 'error');
      await this.resyncFromConfig();
    }
    this.rerender();
  }

  confirmDelete(id) {
    const layout = this.layouts.find(l => l.id === id);
    if (!layout) return;
    confirmDialog({
      title: t('layout.deleteTitle'),
      bodyHtml: t('layout.deleteBody', { name: `<strong>"${esc(layout.name)}"</strong>` }),
      confirmText: t('common.delete'),
      danger: true,
      onConfirm: async () => {
        const idx = this.layouts.findIndex(l => l.id === id);
        const [removed] = this.layouts.splice(idx, 1);
        try {
          await API.setLayouts(this.layouts);
          this.showToast(t('layout.deleted'), 'info');
        } catch (err) {
          this.layouts.splice(idx, 0, removed);
          this.showToast(t('common.setFailed') + String(err), 'error');
          await this.resyncFromConfig();
        }
        this.rerender();
      },
    });
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}
