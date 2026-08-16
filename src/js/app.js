/**
 * app.js — 主入口
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { getVersion } from '@tauri-apps/api/app';
import { listen } from '@tauri-apps/api/event';
import API from './api.js';
import showToast from './toast.js';
import SkinList from './skin-list.js';
import SkinEditor from './skin-editor.js';
import InstallWizard from './install-wizard.js';
import Settings, { initTheme, refreshOpenSettings } from './settings.js';
import { initUpdateCheck } from './update-check.js';
import { t, initI18n } from './i18n.js';

window.__app = null;

// WebView2 在窗口隐藏时不发 mouseleave，重显后按「最后已知光标位置」
// 重算 :hover——那个位置还停在刚点过的按钮上，按钮残留移入样式；合成
// 事件清不掉（非可信输入），只有真实 pointermove 能让它拿到真实光标
// 位置。改为：隐藏前摘除 body.hover-ok 禁用悬停样式（CSS 门控），
// 重显后等第一次真实 pointermove 再恢复；按钮同时 blur 掉键盘焦点，
// 避免重显后残留 :focus-visible 描边。
function setHoverEnabled(on) {
  document.body.classList.toggle('hover-ok', on);
}

class App {
  constructor() {
    this.skinList = null;
    this.skinEditor = null;
    this._loadStateTimers = new Map();
    // 侧栏搜索行展开态（收起是默认态；有查询词时强制展开，见 bindSearch）
    this.searchOpen = false;
    window.__app = this;
    this.init();
  }

  async init() {
    // 先确定界面语言，再做首次渲染
    await initI18n();
    this.renderShell();
    this.bindGlobalGuards();
    this.bindWindowControls();
    this.skinList = new SkinList(
      document.getElementById('skin-list-container'),
      {
        onSelect: (skinId) => this.onSkinSelect(skinId),
      }
    );
    this.skinEditor = new SkinEditor(document.getElementById('main-panel'));
    this.wizard = new InstallWizard({
      onClose: (result) => this.onWizardClose(result),
    });
    this.bindToolbar();
    this.bindSearch();
    this.bindBackendEvents();
    this._paintVersion();
    await initTheme();
    await this.skinList.refresh();

    // 双击 .dskin 冷启动：后端暂存的待安装包，取出来进入安装引导
    try {
      const pending = await API.takePendingPackageInstall();
      if (pending) this.wizard.open(pending);
    } catch (err) {
      console.error('takePendingPackageInstall failed:', err);
    }

    // 启动时全局快捷键被其他程序占用：只记日志用户无感知，取出后 toast 提醒
    try {
      const badHotkey = await API.takeHotkeyError();
      if (badHotkey) showToast(t('app.hotkeyFailed', { hotkey: badHotkey }), 'error');
    } catch (err) {
      console.error('takeHotkeyError failed:', err);
    }

    // 启动更新检测（默认开；内部自行处理失败与弹窗，不阻塞首屏）
    initUpdateCheck();
  }

  // Backend-originated events: the skin right-click menu and the tray can
  // change load state or ask us to open a skin's config page while this
  // window is open, so keep the list and the editor in sync.
  bindBackendEvents() {
    listen('open-skin-config', (event) => {
      this.skinList.select(event.payload);
    });
    // 双击 .dskin 热启动（第二实例转发）：进入安装引导
    listen('open-skin-package', (event) => {
      this.wizard.open(event.payload);
    });
    listen('skin-loaded', async (event) => {
      await this.skinList.refresh();
      await this.onLoadStateChange(event.payload);
    });
    listen('skin-unloaded', async (event) => {
      await this.skinList.refresh();
      await this.onLoadStateChange(event.payload);
    });
  }

  renderShell() {
    document.getElementById('app').innerHTML = `
      <div class="titlebar">
        <div class="brand">
          <!-- 应用 logo：容器已带圆角与裁切，直接铺满即可 -->
          <div class="brand-logo" id="brand-logo">
            <img src="/logo.png" alt="Driftlet" draggable="false" />
          </div>
          <span class="brand-name">Driftlet</span>
          <span class="brand-sub">${t('app.subtitle')}</span>
          <span class="brand-version" id="brand-version"></span>
        </div>
        <div class="win-btns">
          <button id="btn-minimize" title="${t('app.minimize')}" class="win-btn"><svg width="10" height="1"><rect width="10" height="1" fill="currentColor"/></svg></button>
          <button id="btn-maximize" title="${t('app.maximize')}" class="win-btn"><svg width="10" height="10"><rect x="1" y="1" width="8" height="8" fill="none" stroke="currentColor" stroke-width="1.2"/></svg></button>
          <button id="btn-close" title="${t('common.close')}" class="win-btn win-btn-close"><svg width="10" height="10"><line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.2"/><line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.2"/></svg></button>
        </div>
      </div>
      <div class="app-body">
        <aside class="sidebar">
          <div class="sidebar-header">
            <div class="sidebar-heading">
              <span class="sidebar-eyebrow">HARBOR</span>
              <span class="sidebar-title">${t('app.skins')}</span>
            </div>
            <div class="sidebar-tools">
              <span class="sidebar-count" id="skin-count"></span>
              <button id="skin-search-toggle" class="search-toggle" title="${t('list.searchToggle')}">
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.5" y2="16.5"/></svg>
              </button>
            </div>
          </div>
          <!-- 可收起搜索行：平时只占头部一个放大镜钮（零占位），点击或
               Ctrl/Cmd+F 展开；有查询词时强制保持展开，清空后 Esc/再点收起 -->
          <div class="sidebar-search">
            <svg class="search-icon" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="21" y1="21" x2="16.5" y2="16.5"/></svg>
            <input id="skin-search" type="text" placeholder="${t('list.searchPlaceholder')}" autocomplete="off" spellcheck="false" />
            <button id="skin-search-clear" class="search-clear" title="${t('list.searchClear')}">
              <svg width="10" height="10" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
            </button>
          </div>
          <div class="skin-list" id="skin-list-container">
            <div class="list-empty"><p>${t('app.loadingSkins')}</p></div>
          </div>
          <div class="sidebar-footer">
            <button id="btn-add-skin" class="btn-add" title="${t('app.addSkinTitle')}">
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><line x1="6" y1="1" x2="6" y2="11"/><line x1="1" y1="6" x2="11" y2="6"/></svg>
              ${t('app.addSkin')}
            </button>
            <div class="footer-actions">
              <button id="btn-refresh" class="icon-btn" title="${t('app.refreshList')}">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-2.64-6.36"/><polyline points="21 3 21 9 15 9"/></svg>
              </button>
              <button id="btn-open-folder" class="icon-btn" title="${t('app.openFolder')}">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
              </button>
              <button id="btn-settings" class="icon-btn" title="${t('settings.title')}">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
              </button>
            </div>
          </div>
        </aside>
        <main class="main-panel" id="main-panel">
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
          </div>
        </main>
      </div>`;
  }

  // 一次性全局监听：右键屏蔽、悬停门控、最大化图标联动。
  // 与 bindWindowControls 分离——外壳重绘（语言切换）后只需重绑按钮，
  // 这些文档/窗口级监听不能重复注册。
  bindGlobalGuards() {
    const win = getCurrentWindow();

    // Disable right-click context menu
    document.addEventListener('contextmenu', e => e.preventDefault());

    // Ctrl/Cmd+F 展开并聚焦皮肤搜索框（输入控件内不劫持；弹层开着不聚焦幕后）
    document.addEventListener('keydown', (e) => {
      if (!(e.ctrlKey || e.metaKey) || e.shiftKey || e.altKey || e.key.toLowerCase() !== 'f') return;
      const tag = document.activeElement?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (document.getElementById('settings-overlay')) return;
      const box = document.getElementById('skin-search');
      if (!box) return;
      e.preventDefault();
      if (this.searchOpen) box.focus();
      else this._openSearch?.();
    });

    document.addEventListener('pointermove', () => setHoverEnabled(true), { capture: true });
    document.addEventListener('visibilitychange', () => { if (document.hidden) setHoverEnabled(false); });
    win.onFocusChanged(({ payload: focused }) => { if (!focused) setHoverEnabled(false); });

    // 最大化/还原时切换图标（按钮随外壳重建，按 id 现查）
    win.onResized(async () => {
      const btn = document.getElementById('btn-maximize');
      if (!btn) return;
      const isMaxed = await win.isMaximized();
      btn.innerHTML = isMaxed
        ? '<svg width="10" height="10"><rect x="3" y="3" width="6" height="6" fill="none" stroke="currentColor" stroke-width="1.2"/><rect x="1" y="1" width="6" height="6" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>'
        : '<svg width="10" height="10"><rect x="1" y="1" width="8" height="8" fill="none" stroke="currentColor" stroke-width="1.2"/></svg>';
    });
  }

  // 绑定标题栏按钮（外壳每次重绘后都需重绑）
  bindWindowControls() {
    const win = getCurrentWindow();

    const minimizeBtn = document.getElementById('btn-minimize');
    const maximizeBtn = document.getElementById('btn-maximize');
    const closeBtn = document.getElementById('btn-close');

    if (minimizeBtn) {
      minimizeBtn.onclick = () => { minimizeBtn.blur(); setHoverEnabled(false); win.minimize(); };
    }

    if (maximizeBtn) {
      maximizeBtn.onclick = () => win.toggleMaximize();
    }

    if (closeBtn) {
      closeBtn.onclick = () => { closeBtn.blur(); setHoverEnabled(false); win.hide(); };  // hide to tray, not quit
    }
  }

  // 语言切换后的全量重绘：重建外壳，再让各组件在新容器上按新语言重绘
  rerender() {
    this.renderShell();
    this.bindWindowControls();
    this.bindToolbar();
    this.bindSearch();
    this._paintVersion();
    // 外壳重建后容器元素已更换，重新挂接再重绘
    this.skinList.container = document.getElementById('skin-list-container');
    this.skinList.render();
    this.skinEditor.container = document.getElementById('main-panel');
    this.skinEditor.render();
    refreshOpenSettings();
  }

  // 标题栏版本号：取自 tauri.conf.json；失败留空，CSS 对空徽章自动隐藏
  async _paintVersion() {
    try {
      if (!this.appVersion) this.appVersion = await getVersion();
      const el = document.getElementById('brand-version');
      if (el) el.textContent = `v${this.appVersion}`;
    } catch { /* 留空 */ }
  }

  bindToolbar() {
    const addBtn = document.getElementById('btn-add-skin');
    const refreshBtn = document.getElementById('btn-refresh');
    const openBtn = document.getElementById('btn-open-folder');
    const settingsBtn = document.getElementById('btn-settings');

    // 「添加皮肤」直接安装 .dskin 皮肤包
    if (addBtn) {
      addBtn.onclick = () => this.installFromPackage();
    }

    if (refreshBtn) {
      refreshBtn.onclick = async () => {
        this.showToast(t('app.refreshing'), 'info');
        await this.skinList.refresh();
        this.showToast(t('app.refreshed'), 'info');
      };
    }

    if (openBtn) {
      openBtn.onclick = async () => {
        try {
          await API.openSkinsFolder();
          this.showToast(t('app.folderOpened'), 'success');
        } catch (err) {
          console.error(err);
          this.showToast(t('common.openFailed') + String(err), 'error');
        }
      };
    }

    if (settingsBtn) {
      settingsBtn.onclick = async () => {
        // 防叠开由 settings.js 模块级 openSettings 负责，此处无需簿记
        await new Settings().open();
      };
    }
  }

  // 搜索框绑定（外壳重绘后需重绑）。查询状态存在 SkinList.query、
  // 展开态存在 App.searchOpen（均不随外壳重建丢失）。收起只允许发生在
  // 无查询时——有词过滤中行必须可见；不做 blur 自动收起（blur 时列表
  // 还在原位，收起导致行高变化会让 click 落到错误卡片上）
  bindSearch() {
    const input = document.getElementById('skin-search');
    const clearBtn = document.getElementById('skin-search-clear');
    const toggle = document.getElementById('skin-search-toggle');
    const row = document.querySelector('.sidebar-search');
    if (!input || !clearBtn || !toggle || !row || !this.skinList) return;

    const hasQuery = () => input.value.length > 0;
    const applyVisibility = () => {
      const open = this.searchOpen || hasQuery();
      row.classList.toggle('open', open);
      toggle.classList.toggle('active', open);
    };
    const open = (focus = true) => {
      this.searchOpen = true;
      applyVisibility();
      if (focus) input.focus();
    };
    const close = () => {
      this.searchOpen = false;
      applyVisibility();
    };
    const sync = () => {
      clearBtn.classList.toggle('show', hasQuery());
      this.skinList.setQuery(input.value);
      applyVisibility();
    };
    this._openSearch = () => open(true);

    // 语言重绘后恢复：查询词在 SkinList、展开态在 App（不抢焦点）
    input.value = this.skinList.query;
    clearBtn.classList.toggle('show', hasQuery());
    applyVisibility();

    toggle.onclick = () => {
      if (this.searchOpen) {
        if (hasQuery()) input.focus(); else close();
      } else {
        open();
      }
    };
    input.oninput = sync;
    clearBtn.onclick = () => { input.value = ''; sync(); input.focus(); };
    // Esc：有词清词（保持展开），无词收起并交还焦点
    input.onkeydown = (e) => {
      if (e.key !== 'Escape') return;
      if (hasQuery()) { input.value = ''; sync(); } else { close(); }
      input.blur();
    };
  }

  // 安装皮肤包：选完文件统一走安装引导页（与双击 .dskin 同一入口），
  // 引导页自带包校验、状态确认、权限声明展示与安装/加载流程，
  // 关闭时经 onWizardClose 刷新皮肤库与编辑器。
  async installFromPackage() {
    let packagePath;
    try {
      packagePath = await API.pickSkinPackage();
    } catch (err) {
      this.showToast(t('app.pickerFailed') + String(err), 'error');
      return;
    }
    if (!packagePath) return;
    this.wizard.open(packagePath);
  }

  // 安装引导页关闭后：刷新皮肤库；若编辑器正开着该皮肤则同步刷新
  async onWizardClose(result) {
    await this.skinList.refresh();
    if (result?.skinId && this.skinEditor.skinId === result.skinId) {
      await this.skinEditor.load(result.skinId);
    }
  }

  async onSkinSelect(skinId) {
    if (!skinId) {
      this.skinEditor.clear();
      return;
    }
    await this.skinEditor.load(skinId);
  }

  // 编辑器随加载/卸载刷新的事件单一路径：后端 load/unload 命令（含前端
  // 自己发起的）都会 emit skin-loaded / skin-unloaded（commands.rs），
  // 列表按钮不再直连 editor.load，避免两路并发双调。
  // 删除选中的皮肤走 onSelect(null) → onSkinSelect → editor.clear()
  // reload 路径（层级「置顶→正常」/重新加载/热重载）会连发 unloaded +
  // loaded 两个事件，按皮肤各自 80ms 合并为一次刷新（否则整页连着重绘
  // 两遍；计时器必须按皮肤分键——「全部重载」连发多皮肤事件，共享一个
  // 计时器会让先到的皮肤刷新被后到者挤掉）
  onLoadStateChange(skinId) {
    clearTimeout(this._loadStateTimers.get(skinId));
    this._loadStateTimers.set(skinId, setTimeout(() => {
      this._loadStateTimers.delete(skinId);
      if (this.skinEditor.skinId === skinId) {
        this.skinEditor.load(skinId);
      }
    }, 80));
  }

  showToast(msg, type) {
    showToast(msg, type);
  }
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', () => new App());
} else {
  new App();
}
