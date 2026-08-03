/**
 * settings.js — 设置面板（自启动 + 主题 + 语言）
 */
import API from './api.js';
import showToast from './toast.js';
import { t, getLang, applyLang } from './i18n.js';

// 当前打开的设置面板实例（供语言切换后原地重绘；关闭时清空）
let openSettings = null;

/** 语言切换后重绘已打开的设置面板（未打开则无操作） */
export function refreshOpenSettings() {
  if (!openSettings) return;
  // 重绘直接丢弃旧 DOM、不经过 close() 的清理：先摘除可能仍在录制的
  // 热键监听，否则 window 级 capture keydown 残留，持续劫持键盘
  openSettings._unbindHotkey();
  document.getElementById('settings-overlay')?.remove();
  openSettings.render();
}

export default class Settings {
  constructor(onClose) {
    this.onClose = onClose;
    this.autostart = false;
    this.theme = 'auto';
    this.hotkey = '';
    this.hotReload = true;
    this._hotkeyListener = null;
  }

  async open() {
    // Load current state
    this.autostart = await API.getAutostart().catch(() => false);
    const config = await API.getAppConfig().catch(() => ({ theme: 'auto' }));
    this.theme = config.theme || 'auto';
    this.hotkey = config.hotkey_toggle_skins || '';
    this.hotReload = config.hot_reload !== false;

    this.render();
  }

  render() {
    // 防叠开：已有面板（可能来自另一实例）先摘除其热键监听再移除
    if (document.getElementById('settings-overlay')) {
      openSettings?._unbindHotkey();
      document.getElementById('settings-overlay').remove();
    }
    openSettings = this;

    const overlay = document.createElement('div');
    overlay.className = 'settings-overlay';
    overlay.id = 'settings-overlay';

    overlay.innerHTML = `
      <div class="settings-panel">
        <h2>${t('settings.title')}</h2>

        <div class="settings-row">
          <div>
            <label>${t('settings.autostart')}</label>
            <div class="hint">${t('settings.autostartHint')}</div>
          </div>
          <label class="toggle">
            <input type="checkbox" id="cfg-autostart" ${this.autostart ? 'checked' : ''}>
            <span class="slider"></span>
          </label>
        </div>

        <div class="settings-row">
          <div>
            <label>${t('settings.theme')}</label>
            <div class="hint">${t('settings.themeHint')}</div>
          </div>
          <div class="theme-options">
            <button class="theme-btn ${this.theme === 'auto' ? 'active' : ''}" data-theme="auto">${t('settings.themeAuto')}</button>
            <button class="theme-btn ${this.theme === 'light' ? 'active' : ''}" data-theme="light">${t('settings.themeLight')}</button>
            <button class="theme-btn ${this.theme === 'dark' ? 'active' : ''}" data-theme="dark">${t('settings.themeDark')}</button>
          </div>
        </div>

        <div class="settings-row">
          <div>
            <label>${t('settings.language')}</label>
            <div class="hint">${t('settings.languageHint')}</div>
          </div>
          <div class="theme-options">
            <button class="theme-btn lang-btn ${getLang() === 'zh-CN' ? 'active' : ''}" data-lang="zh-CN">${t('settings.langZh')}</button>
            <button class="theme-btn lang-btn ${getLang() === 'en' ? 'active' : ''}" data-lang="en">${t('settings.langEn')}</button>
          </div>
        </div>

        <div class="settings-row">
          <div>
            <label>${t('settings.hotkey')}</label>
            <div class="hint">${t('settings.hotkeyHint')}</div>
          </div>
          <button class="theme-btn" id="cfg-hotkey">${this.esc(this.hotkey) || t('settings.hotkeyNone')}</button>
        </div>

        <div class="settings-row">
          <div>
            <label>${t('settings.hotReload')}</label>
            <div class="hint">${t('settings.hotReloadHint')}</div>
          </div>
          <label class="toggle">
            <input type="checkbox" id="cfg-hotreload" ${this.hotReload ? 'checked' : ''}>
            <span class="slider"></span>
          </label>
        </div>

        <div class="settings-row">
          <div>
            <label>${t('settings.backup')}</label>
            <div class="hint">${t('settings.backupHint')}</div>
          </div>
          <div class="theme-options">
            <button class="theme-btn" id="cfg-export">${t('settings.backupExport')}</button>
            <button class="theme-btn" id="cfg-import">${t('settings.backupImport')}</button>
          </div>
        </div>

        <button class="settings-close">${t('common.close')}</button>
      </div>
    `;

    document.body.appendChild(overlay);

    // Auto-start toggle
    overlay.querySelector('#cfg-autostart').onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setAutostart(on);
        showToast(on ? t('settings.autostartOn') : t('settings.autostartOff'), on ? 'success' : 'info');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // Theme buttons
    overlay.querySelectorAll('.theme-btn:not(.lang-btn)').forEach(btn => {
      btn.onclick = async () => {
        const theme = btn.dataset.theme;
        try {
          await API.setTheme(theme);
          applyTheme(theme);
          this.theme = theme;
          overlay.querySelectorAll('.theme-btn:not(.lang-btn)').forEach(b => b.classList.remove('active'));
          btn.classList.add('active');
          showToast(t('settings.themeSwitched', { theme: btn.textContent }), 'success');
        } catch (err) {
          showToast(t('common.setFailed') + String(err), 'error');
        }
      };
    });

    // Language buttons：applyLang 会触发全量重绘（含本面板，见 refreshOpenSettings）
    overlay.querySelectorAll('.lang-btn').forEach(btn => {
      btn.onclick = () => {
        const lang = btn.dataset.lang;
        if (lang === getLang()) return;
        applyLang(lang);
        showToast(t('settings.langSwitched'), 'success');
      };
    });

    // Hotkey capture：点击进入录制态，Esc 取消，Backspace/Delete 禁用，
    // 合法组合（≥1 修饰键 + 普通键）保存。注意录制期间按下当前热键仍会
    // 触发一次全局显隐切换（全局热键无法局部屏蔽，已知小怪癖）。
    const hotkeyBtn = overlay.querySelector('#cfg-hotkey');
    const renderHotkey = () => {
      hotkeyBtn.textContent = this.hotkey || t('settings.hotkeyNone');
    };
    const saveHotkey = async (combo) => {
      try {
        await API.setHotkey(combo);
        this.hotkey = combo;
        showToast(t('settings.hotkeySaved'), 'success');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
      renderHotkey();
    };
    hotkeyBtn.onclick = () => {
      if (this._hotkeyListener) return; // 已在录制中
      hotkeyBtn.textContent = `${t('settings.hotkeyRecording')} ${t('settings.hotkeySubHint')}`;
      hotkeyBtn.classList.add('active');

      const finish = () => {
        window.removeEventListener('keydown', onKey, true);
        this._hotkeyListener = null;
        hotkeyBtn.classList.remove('active');
        renderHotkey();
      };
      const onKey = (e) => {
        e.preventDefault();
        e.stopPropagation();
        if (e.key === 'Escape') { finish(); return; }
        if (e.key === 'Backspace' || e.key === 'Delete') {
          window.removeEventListener('keydown', onKey, true);
          this._hotkeyListener = null;
          hotkeyBtn.classList.remove('active');
          saveHotkey('');
          return;
        }
        // 单独的修饰键按下不构成组合，继续等
        if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;
        const mods = [];
        if (e.ctrlKey) mods.push('Ctrl');
        if (e.altKey) mods.push('Alt');
        if (e.shiftKey) mods.push('Shift');
        if (e.metaKey) mods.push('Super');
        if (mods.length === 0) return; // 必须带修饰键（裸键会全局劫持打字）
        let key = e.key === ' ' ? 'Space' : e.key;
        if (key.length === 1) key = key.toUpperCase();
        window.removeEventListener('keydown', onKey, true);
        this._hotkeyListener = null;
        hotkeyBtn.classList.remove('active');
        saveHotkey([...mods, key].join('+'));
      };
      this._hotkeyListener = onKey;
      window.addEventListener('keydown', onKey, true);
    };

    // 皮肤热重载开关（开发分区，仅 debug 构建的 watcher 读取该标志）
    overlay.querySelector('#cfg-hotreload').onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setHotReload(on);
        showToast(on ? t('settings.hotReloadOn') : t('settings.hotReloadOff'), 'info');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // 布局备份：导出为一个 zip；导入会覆盖全部配置与皮肤，先弹危险确认，
    // 成功后整页 reload——语言/主题/皮肤列表/配置缓存一次全部重建
    overlay.querySelector('#cfg-export').onclick = async () => {
      try {
        const path = await API.exportConfig();
        if (path) showToast(t('settings.backupExported', { path }), 'success');
        else showToast(t('common.canceled'), 'info');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };
    overlay.querySelector('#cfg-import').onclick = () => {
      const confirmOverlay = document.createElement('div');
      confirmOverlay.className = 'confirm-overlay';
      confirmOverlay.innerHTML = `
        <div class="confirm-dialog">
          <div class="confirm-icon danger"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg></div>
          <h3>${t('settings.backupImportTitle')}</h3>
          <p>${t('settings.backupImportBody')}</p>
          <p class="confirm-hint">${t('settings.backupImportHint')}</p>
          <div class="confirm-buttons">
            <button class="confirm-btn cancel">${t('common.cancel')}</button>
            <button class="confirm-btn danger">${t('settings.backupImport')}</button>
          </div>
        </div>`;
      document.body.appendChild(confirmOverlay);
      const closeConfirm = () => confirmOverlay.remove();
      confirmOverlay.querySelector('.confirm-btn.cancel').onclick = closeConfirm;
      confirmOverlay.querySelector('.confirm-btn.danger').onclick = async () => {
        closeConfirm();
        try {
          const done = await API.importConfig();
          if (done) {
            showToast(t('settings.backupImported'), 'success');
            setTimeout(() => location.reload(), 800);
          } else {
            showToast(t('common.canceled'), 'info');
          }
        } catch (err) {
          showToast(t('common.setFailed') + String(err), 'error');
        }
      };
      confirmOverlay.addEventListener('click', (e) => {
        if (e.target === confirmOverlay) closeConfirm();
      });
    };

    const close = () => {
      this._unbindHotkey();
      overlay.remove();
      if (openSettings === this) openSettings = null;
      if (this.onClose) this.onClose();
    };

    // Close
    overlay.querySelector('.settings-close').onclick = close;

    // Click outside to close
    overlay.onclick = (e) => {
      if (e.target === overlay) close();
    };
  }

  // 摘除热键录制的全局键监听。任何绕过 close() 的销毁/重绘路径
  // （refreshOpenSettings、防叠开移除）都必须先调它
  _unbindHotkey() {
    if (this._hotkeyListener) {
      window.removeEventListener('keydown', this._hotkeyListener, true);
      this._hotkeyListener = null;
    }
  }

  esc(str) {
    const div = document.createElement('div');
    div.textContent = String(str ?? '');
    return div.innerHTML;
  }
}

// ─── Theme engine ───

/**
 * Time-based auto theme:
 *   06:00 – 17:59 → light (day)
 *   18:00 – 05:59 → dark (night)
 */
function timeBasedTheme() {
  const hour = new Date().getHours();
  return (hour >= 6 && hour < 18) ? 'light' : 'dark';
}

// Check every 5 minutes for time transition
let _themeCheckInterval = null;

export function applyTheme(theme) {
  if (theme === 'auto') {
    document.documentElement.setAttribute('data-theme', timeBasedTheme());
  } else {
    document.documentElement.setAttribute('data-theme', theme);
  }
}

export async function initTheme() {
  // Periodic check for time-based auto theme
  if (!_themeCheckInterval) {
    _themeCheckInterval = setInterval(async () => {
      const config = await API.getAppConfig().catch(() => ({ theme: 'auto' }));
      if (!config.theme || config.theme === 'auto') {
        applyTheme('auto');
      }
    }, 5 * 60 * 1000); // every 5 minutes
  }

  // Load saved theme
  const config = await API.getAppConfig().catch(() => ({ theme: 'auto' }));
  applyTheme(config.theme || 'auto');
}
