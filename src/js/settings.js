/**
 * settings.js — 设置面板（页签布局，与皮肤编辑器 cfg-tabs 同一套视觉：
 *               通用[自启动/更新检测/快捷键] + 外观[主题/语言] + 高级[备份/日志/开发模式]）
 */
import API from './api.js';
import showToast from './toast.js';
import { t, getLang, applyLang } from './i18n.js';
import { esc, confirmDialog } from './dom.js';

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
  constructor() {
    this.autostart = false;
    this.theme = 'auto';
    this.hotkey = '';
    this.hotReload = false;
    this.updateCheck = true;
    this.activeTab = 'general';
    this._hotkeyListener = null;
  }

  async open() {
    // Load current state
    this.autostart = await API.getAutostart().catch(() => false);
    const config = await API.getAppConfig().catch(() => ({ theme: 'auto' }));
    this.theme = config.theme || 'auto';
    this.hotkey = config.hotkey_toggle_skins || '';
    this.hotReload = config.hot_reload === true;
    // 默认开：仅显式存了 false 才视为关闭（与后端 serde default 一致）
    this.updateCheck = config.update_check !== false;

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

    const tab = this.activeTab;
    overlay.innerHTML = `
      <div class="settings-panel">
        <h2>${t('settings.title')}</h2>

        <div class="cfg-tabs">
          <button class="cfg-tab ${tab === 'general' ? 'active' : ''}" data-tab="general">${t('settings.tabGeneral')}</button>
          <button class="cfg-tab ${tab === 'appearance' ? 'active' : ''}" data-tab="appearance">${t('settings.tabAppearance')}</button>
          <button class="cfg-tab ${tab === 'advanced' ? 'active' : ''}" data-tab="advanced">${t('settings.tabAdvanced')}</button>
        </div>

        <div class="settings-page" data-page="general" ${tab !== 'general' ? 'style="display:none"' : ''}>
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
              <label>${t('settings.updateCheck')}</label>
              <div class="hint">${t('settings.updateCheckHint')}</div>
            </div>
            <label class="toggle">
              <input type="checkbox" id="cfg-updatecheck" ${this.updateCheck ? 'checked' : ''}>
              <span class="slider"></span>
            </label>
          </div>
          <div class="settings-row">
            <div>
              <label>${t('settings.hotkey')}</label>
              <div class="hint">${t('settings.hotkeyHint')}</div>
            </div>
            <button class="theme-btn" id="cfg-hotkey">${esc(this.hotkey) || t('settings.hotkeyNone')}</button>
          </div>
        </div>

        <div class="settings-page" data-page="appearance" ${tab !== 'appearance' ? 'style="display:none"' : ''}>
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
        </div>

        <div class="settings-page" data-page="advanced" ${tab !== 'advanced' ? 'style="display:none"' : ''}>
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
          <div class="settings-row">
            <div>
              <label>${t('settings.log')}</label>
              <div class="hint">${t('settings.logHint')}</div>
            </div>
            <div class="theme-options">
              <button class="theme-btn" id="cfg-open-log">${t('settings.logOpen')}</button>
            </div>
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
        </div>

        <button class="settings-close">${t('common.close')}</button>
      </div>
    `;

    document.body.appendChild(overlay);

    // 页签切换（与皮肤编辑器 cfg-tabs 同一模式：按钮 active + 页 display）
    overlay.querySelectorAll('.cfg-tab').forEach(btn => {
      btn.onclick = () => {
        this.activeTab = btn.dataset.tab;
        overlay.querySelectorAll('.cfg-tab').forEach(b => b.classList.toggle('active', b === btn));
        overlay.querySelectorAll('.settings-page').forEach(page => {
          page.style.display = page.dataset.page === this.activeTab ? '' : 'none';
        });
      };
    });

    // Auto-start toggle
    overlay.querySelector('#cfg-autostart').onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setAutostart(on);
        showToast(on ? t('settings.autostartOn') : t('settings.autostartOff'), on ? 'success' : 'info');
      } catch (err) {
        // 保存失败回滚勾选态（对齐皮肤编辑器开关的回滚语义）
        e.target.checked = !on;
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // 更新检测开关（仅持久化；下次启动生效）
    overlay.querySelector('#cfg-updatecheck').onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setUpdateCheck(on);
        showToast(on ? t('settings.updateCheckOn') : t('settings.updateCheckOff'), on ? 'success' : 'info');
      } catch (err) {
        // 保存失败回滚勾选态（对齐皮肤编辑器开关的回滚语义）
        e.target.checked = !on;
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // Theme buttons
    overlay.querySelectorAll('.theme-btn[data-theme]').forEach(btn => {
      btn.onclick = async () => {
        const theme = btn.dataset.theme;
        try {
          await API.setTheme(theme);
          applyTheme(theme);
          this.theme = theme;
          overlay.querySelectorAll('.theme-btn[data-theme]').forEach(b => b.classList.remove('active'));
          btn.classList.add('active');
          showToast(t('settings.themeSwitched', { theme: btn.textContent }), 'success');
        } catch (err) {
          showToast(t('common.setFailed') + String(err), 'error');
        }
      };
    });

    // Language buttons：applyLang 会触发全量重绘（含本面板，见 refreshOpenSettings）
    overlay.querySelectorAll('.theme-btn[data-lang]').forEach(btn => {
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

    // 打开日志窗口（已开着则后端把它提到前台）；成功后设置页自动关闭
    overlay.querySelector('#cfg-open-log').onclick = async () => {
      try {
        await API.openLogWindow();
        close();
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };
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
          finish();
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
        finish();
        saveHotkey([...mods, key].join('+'));
      };
      this._hotkeyListener = onKey;
      window.addEventListener('keydown', onKey, true);
    };

    // 开发模式开关（热重载仅 debug 构建的 watcher 读取该标志；DevTools 解锁
    // 由 open_skin_devtools 实时读同一标志，全构建生效）
    overlay.querySelector('#cfg-hotreload').onchange = async (e) => {
      const on = e.target.checked;
      try {
        await API.setHotReload(on);
        showToast(on ? t('settings.hotReloadOn') : t('settings.hotReloadOff'), 'info');
      } catch (err) {
        // 保存失败回滚勾选态（对齐皮肤编辑器开关的回滚语义）
        e.target.checked = !on;
        showToast(t('common.setFailed') + String(err), 'error');
      }
    };

    // 布局备份：导出为一个 zip；导入会覆盖全部配置与皮肤，先弹危险确认，
    // 成功后整页 reload——语言/主题/皮肤列表/配置缓存一次全部重建
    const exportBtn = overlay.querySelector('#cfg-export');
    const importBtn = overlay.querySelector('#cfg-import');
    exportBtn.onclick = async () => {
      // 原生保存对话框弹出期间禁止重复点击，否则会叠开多个文件选择器
      exportBtn.disabled = true;
      try {
        const path = await API.exportConfig();
        if (path) showToast(t('settings.backupExported', { path }), 'success');
        else showToast(t('common.canceled'), 'info');
      } catch (err) {
        showToast(t('common.setFailed') + String(err), 'error');
      } finally {
        exportBtn.disabled = false;
      }
    };
    importBtn.onclick = () => {
      // 确认框 + 原生选择对话框存续期间禁止重复点击，防叠开
      importBtn.disabled = true;
      confirmDialog({
        title: t('settings.backupImportTitle'),
        bodyHtml: t('settings.backupImportBody'),
        hint: t('settings.backupImportHint'),
        confirmText: t('settings.backupImport'),
        danger: true,
        // 未确认而关闭（取消 / Esc / 点遮罩）：解除按钮禁用
        onCancel: () => { importBtn.disabled = false; },
        onConfirm: async () => {
          // 确认后文件选择器仍在交互，按钮保持禁用直至导入流程结束
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
          } finally {
            importBtn.disabled = false;
          }
        },
      });
    };

    const close = () => {
      this._unbindHotkey();
      overlay.remove();
      if (openSettings === this) openSettings = null;
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
