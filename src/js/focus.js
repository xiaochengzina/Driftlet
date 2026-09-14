/**
 * focus.js — 专注模式面板（功能面板模板实例，docs/设计规范.md §3.3；
 *            方案 docs/proposals/专注模式方案-2026-09.md）
 *
 * 模式的一切集中一处：状态卡（当前态 + 进入/退出主按钮）+ 动作选择器
 *（隐藏/卸载）+ 全屏自动进入开关 + 白名单勾选清单。面板开着时模式被
 * 别处触发（热键/托盘/全屏）经 focus-mode-changed 事件即时联动。
 * toast 统一由 app.js 的事件监听弹出（面板内只更新显示，双通道会双 toast）。
 */
import API from './api.js';
import showToast from './toast.js';
import { t } from './i18n.js';
import { esc, escAttr, dispName, bindEsc, closeOnMaskClick, bindHotkeyCapture } from './dom.js';

export default class FocusPanel {
  constructor() {
    this.overlay = null;
    this._unbindEsc = null;
    this.state = null;   // { active, action, auto_fullscreen, exempt: [ids] }
    this.skins = [];     // listSkins 全量（白名单清单的数据源）
    this.hotkey = '';    // 全局快捷键（专注模式开关的快捷键）
    this._hotkeyListener = null;
  }

  async open() {
    if (this.overlay) return; // 防叠开
    try {
      const [state, skins, config] = await Promise.all([
        API.getFocusModeState(), API.listSkins(), API.getAppConfig(),
      ]);
      this.state = state;
      this.skins = skins;
      this.hotkey = config?.hotkey_toggle_skins || '';
    } catch (err) {
      showToast(t('focus.loadFailed') + String(err), 'error');
      return;
    }
    this.render();
  }

  close() {
    this._unbindEsc?.();
    this._unbindEsc = null;
    this._hotkeyListener?.unbind?.();
    this._hotkeyListener = null;
    this.overlay?.remove();
    this.overlay = null;
  }

  /** 模式被别处触发（热键/托盘/全屏）：面板开着就原地刷新状态 */
  async refreshState() {
    if (!this.overlay) return;
    try {
      this.state = await API.getFocusModeState();
      this.render();
    } catch { /* 状态拉取失败保旧显示 */ }
  }

  render() {
    this._unbindEsc?.();
    this._hotkeyListener?.unbind?.();
    this._hotkeyListener = null;
    const wasOpen = !!this.overlay;
    this.overlay?.remove();

    const s = this.state;
    const actionLabel = s.action === 'unload' ? t('focus.actionUnload') : t('focus.actionHide');
    const rows = this.skins.length === 0
      ? `<div class="group-edit-empty">${t('list.empty')}</div>`
      : this.skins.map(skin => {
          const statusClass = !skin.loaded ? 'unloaded' : skin.hidden ? 'hidden' : 'loaded';
          const checked = s.exempt.includes(skin.id) ? ' checked' : '';
          return `
            <label class="group-edit-skin">
              <input type="checkbox" data-skin-id="${escAttr(skin.id)}"${checked}>
              <span class="compact-status ${statusClass}"><span class="status-dot"></span></span>
              <span class="group-edit-skin-name">${esc(dispName(skin))}</span>
            </label>`;
        }).join('');

    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay' + (wasOpen ? ' no-anim' : '');
    overlay.innerHTML = `
      <div class="panel focus-panel">
        <div class="panel-head">
          <div>
            <h2>${t('focus.title')}</h2>
            <div class="panel-sub">${t('focus.statusHint')}</div>
          </div>
          <button class="panel-close" title="${t('common.close')}">
            <svg width="11" height="11" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          </button>
        </div>
        <div class="panel-body">
          <div class="focus-status${s.active ? ' active' : ''}">
            <span class="focus-status-ico">
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="6"/><circle cx="12" cy="12" r="2"/></svg>
            </span>
            <div class="focus-status-text">
              <span class="focus-state-line">${s.active ? t('focus.statusActive') : t('focus.statusInactive')}</span>
              ${s.active ? `<span class="focus-state-sub">${esc(actionLabel)}${s.affected_count > 0 ? ` · ${t(s.action === 'unload' ? 'focus.cardUnloaded' : 'focus.cardHidden', { count: s.affected_count })}` : ''}</span>` : ''}
            </div>
            <button class="confirm-btn primary" id="focus-toggle">${s.active ? t('focus.exit') : t('focus.enter')}</button>
          </div>
          <div class="settings-row">
            <div>
              <label>${t('focus.action')}</label>
              <div class="hint">${t('focus.actionHint')}</div>
            </div>
            <div class="theme-options">
              <button class="theme-btn ${s.action !== 'unload' ? 'active' : ''}" data-action="hide">${t('focus.actionHide')}</button>
              <button class="theme-btn ${s.action === 'unload' ? 'active' : ''}" data-action="unload">${t('focus.actionUnload')}</button>
            </div>
          </div>
          <div class="settings-row">
            <div>
              <label>${t('focus.autoFullscreen')}</label>
              <div class="hint">${t('focus.autoFullscreenHint')}</div>
            </div>
            <label class="toggle">
              <input type="checkbox" id="focus-auto" ${s.auto_fullscreen ? 'checked' : ''}>
              <span class="slider"></span>
            </label>
          </div>
          <div class="settings-row">
            <div>
              <label>${t('settings.hotkey')}</label>
              <div class="hint">${t('settings.hotkeyHint')}</div>
            </div>
            <div class="btn-cluster">
              <button class="action-btn hotkey-btn" id="focus-hotkey">${esc(this.hotkey) || t('settings.hotkeyNone')}</button>
            </div>
          </div>
          <div class="focus-list-head">
            <label>${t('focus.whitelist')}</label>
            <div class="hint">${t('focus.whitelistHint')}</div>
          </div>
          <div class="group-edit-skins focus-whitelist">${rows}</div>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    this.overlay = overlay;
    this._unbindEsc = bindEsc(() => this.close());
    closeOnMaskClick(overlay, () => this.close());
    overlay.querySelector('.panel-close').onclick = () => this.close();

    // 进入/退出主按钮（忙态防连点；toast 走 app.js 的事件监听单通道）
    const toggleBtn = overlay.querySelector('#focus-toggle');
    toggleBtn.onclick = async () => {
      toggleBtn.disabled = true;
      toggleBtn.textContent = t('focus.working');
      try {
        const active = await API.toggleFocusMode();
        this.state.active = active;
        // 重拉全量状态（动作档可能被别处改过）再重绘
        this.state = await API.getFocusModeState();
        this.render();
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
        toggleBtn.disabled = false;
        toggleBtn.textContent = this.state.active ? t('focus.exit') : t('focus.enter');
      }
    };

    // 动作选择器（segmented 同族；模式激活期间改档不影响本次还原口径）
    overlay.querySelectorAll('.theme-btn[data-action]').forEach(btn => {
      btn.onclick = async () => {
        const prev = this.state.action;
        const next = btn.dataset.action;
        if (next === prev) return;
        try {
          await API.setFocusModeAction(next);
          this.state.action = next;
          overlay.querySelectorAll('.theme-btn[data-action]').forEach(b =>
            b.classList.toggle('active', b === btn));
        } catch (err) {
          showToast(t('common.setFailed') + String(err), 'error');
        }
      };
    });

    // 全屏自动进入开关（失败回滚勾选态，与设置页开关同语义）
    const autoBox = overlay.querySelector('#focus-auto');
    autoBox.onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setFocusModeAutoFullscreen(on);
        this.state.auto_fullscreen = on;
      } catch (err) {
        e.target.checked = !on;
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // 全局快捷键录制（dom.js 共享件；专注模式开关的快捷键——设置页原席位
    // 已迁此。录制期间按下当前热键仍会真实触发一次模式切换，已知小怪癖）
    const hotkeyBtn = overlay.querySelector('#focus-hotkey');
    const saveHotkey = async (combo) => {
      try {
        await API.setHotkey(combo);
        this.hotkey = combo;
        showToast(t('settings.hotkeySaved'), 'success');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
      hotkeyBtn.textContent = this.hotkey || t('settings.hotkeyNone');
    };
    this._hotkeyListener = bindHotkeyCapture(hotkeyBtn, {
      recordingText: t('settings.hotkeyRecording'),
      onSave: saveHotkey,
    });

    // 白名单勾选（失败回滚）
    overlay.querySelector('.focus-whitelist').addEventListener('change', async (e) => {
      const cb = e.target;
      if (!(cb instanceof HTMLInputElement) || cb.type !== 'checkbox') return;
      const skinId = cb.dataset.skinId;
      const on = cb.checked;
      try {
        await API.setFocusExempt(skinId, on);
        const i = this.state.exempt.indexOf(skinId);
        if (on && i < 0) this.state.exempt.push(skinId);
        if (!on && i >= 0) this.state.exempt.splice(i, 1);
      } catch (err) {
        cb.checked = !on;
        showToast(t('common.setFailed') + String(err), 'error');
      }
    });
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}
