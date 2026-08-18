/**
 * install-wizard.js — .dskin 双击安装的引导页
 *
 * 全窗覆盖层（盖住管理器界面与标题栏），状态机四态：
 *   检查中 → 确认 → 安装中 → 完成 / 失败
 * 检查或安装失败都进入失败态，给出后端返回的原因。
 */
import API from './api.js';
import { t } from './i18n.js';
import { esc, dispName, dispDesc } from './dom.js';
import { renderPermsHTML } from './perms.js';

export default class InstallWizard {
  /**
   * @param {{ onClose: (result: { skinId: string } | null) => void }} callbacks
   *   onClose 在引导页销毁时调用；安装成功过则带上 skinId，否则为 null。
   */
  constructor({ onClose } = {}) {
    this.onClose = onClose || (() => {});
    this.overlay = null;
    this.installedSkinId = null;
    this.busy = false; // 安装进行中，禁止关闭
    this._gen = 0; // 代际计数：防止上一次异步流程的结果落进新向导
  }

  /** 打开引导页。已打开时直接换包重新检查（不排队）。 */
  open(packagePath) {
    this._gen++;
    this.close(true);
    this.busy = false; // 旧流程已被代际计数作废，busy 不带到新向导
    this.installedSkinId = null;

    this.overlay = document.createElement('div');
    this.overlay.className = 'wizard-overlay';
    this.overlay.innerHTML = `
      <div class="wizard-titlebar">
        <span class="wizard-title">${t('wizard.title')}</span>
        <button class="win-btn wizard-close" title="${t('common.cancel')}">
          <svg width="10" height="10"><line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.2"/><line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.2"/></svg>
        </button>
      </div>
      <div class="wizard-body" id="wizard-body"></div>`;
    // 挂 document.body（同 settings overlay）：app.js rerender()（语言切换）
    // 会重写 #app.innerHTML，挂在里面会被静默销毁，detached DOM 继续空转
    document.body.appendChild(this.overlay);

    this.overlay.querySelector('.wizard-close').onclick = () => this.close();
    this._inspect(packagePath);
  }

  /** 关闭并销毁。安装进行中（busy）忽略关闭请求。 */
  close(silent = false) {
    if (!this.overlay) return;
    if (this.busy && !silent) return;
    this.overlay.remove();
    this.overlay = null;
    // 换包场景（silent）也一样：done 页装完即应刷新列表——installedSkinId
    // 非空时 onClose 不得被静默丢弃（否则刚装好的皮肤不可见直至手动刷新）
    if (this.installedSkinId || !silent) {
      this.onClose(this.installedSkinId ? { skinId: this.installedSkinId } : null);
      this.installedSkinId = null;
    }
  }

  get body() {
    return this.overlay?.querySelector('#wizard-body');
  }

  // ── 状态 1：检查中 ──
  async _inspect(packagePath) {
    const gen = this._gen;
    const fileName = packagePath.split(/[\\/]/).pop();
    this._renderStatus(t('wizard.checking'), fileName);

    try {
      const info = await API.inspectSkinPackage(packagePath);
      if (!this.overlay || gen !== this._gen) return; // 等待期间被换下/关闭
      this._renderConfirm(info, packagePath);
    } catch (err) {
      if (!this.overlay || gen !== this._gen) return;
      this._renderFailure(t('wizard.invalid'), err);
    }
  }

  // ── 状态 2：确认 ──
  _renderConfirm(info, packagePath) {
    const versionText = (v) => v ? `v${esc(v)}` : t('common.noVersion');
    let heading, statusLine, action, danger = false;
    switch (info.status) {
      case 'update':
        heading = t('app.updateSkin');
        statusLine = t('wizard.statusUpdate', { installed: versionText(info.installed_version), version: versionText(info.version) });
        action = t('common.update');
        break;
      case 'reinstall':
        heading = t('app.reinstall');
        statusLine = t('wizard.statusReinstall', { version: versionText(info.version) });
        action = t('common.reinstall');
        break;
      case 'downgrade':
        heading = t('app.downgrade');
        statusLine = t('wizard.statusDowngrade', { installed: versionText(info.installed_version), version: versionText(info.version) });
        action = t('common.downgrade');
        danger = true;
        break;
      default:
        heading = t('app.installSkin');
        statusLine = t('wizard.notInstalled');
        action = t('common.install');
    }
    const note = info.status === 'new' ? ''
      : `<p class="wizard-note">${t('app.settingsKept')}</p>`;

    this.body.innerHTML = `
      <div class="wizard-card">
        <div class="wizard-pkg-icon"><svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8l-9-5-9 5v8l9 5 9-5V8z"/><path d="M3 8l9 5 9-5"/><path d="M12 13v8"/></svg></div>
        <h2 class="wizard-name">${esc(dispName(info))}</h2>
        <div class="wizard-meta">
          <span>${esc(info.id)}</span>
          ${info.version ? `<span>v${esc(info.version)}</span>` : ''}
          ${info.author ? `<span>${esc(info.author)}</span>` : ''}
        </div>
        ${dispDesc(info) ? `<p class="wizard-desc">${esc(dispDesc(info))}</p>` : ''}
        ${renderPermsHTML(info.permissions, { withTitle: true })}
        ${info.requires_host_version ? `<p class="wizard-note warn">${t('wizard.hostTooOld', { version: esc(info.requires_host_version) })}</p>` : ''}
        <div class="wizard-statusline ${danger ? 'danger' : ''}">
          <strong>${heading}</strong> · ${statusLine}
        </div>
        ${note}
        <div class="wizard-actions">
          <button class="confirm-btn cancel" data-act="cancel">${t('common.cancel')}</button>
          <button class="confirm-btn ${danger ? 'danger' : 'primary'}" data-act="go">${action}</button>
        </div>
      </div>`;

    this.body.querySelector('[data-act="cancel"]').onclick = () => this.close();
    this.body.querySelector('[data-act="go"]').onclick = () => this._install(packagePath, info);
  }

  // 权限声明列表的渲染已收编为 src/js/perms.js 单一口源
  // （引导页与配置页权限分区共用 renderPermsHTML）。

  // ── 状态 3：安装中 ──
  async _install(packagePath, info) {
    const gen = this._gen;
    this.busy = true;
    this._renderStatus(t('wizard.installing'), dispName(info));

    try {
      await API.installSkinPackage(packagePath);
      if (!this.overlay || gen !== this._gen) {
        // busy 中被新双击换包强制 close：安装仍在后台完成了，走 onClose
        // 让外壳刷新皮肤列表，否则新皮肤要等手动刷新才出现。
        // 注意 busy 属于当前代际（open() 已为新向导重置），过期结果不得改动
        this.onClose({ skinId: info.id });
        return;
      }
      this.busy = false;
      this.installedSkinId = info.id;
      this._renderDone(info);
    } catch (err) {
      if (!this.overlay || gen !== this._gen) return;
      this.busy = false;
      this._renderFailure(t('wizard.installFailed'), err);
    }
  }

  // ── 状态 4：完成 ──
  _renderDone(info) {
    this.body.innerHTML = `
      <div class="wizard-card">
        <div class="wizard-result-icon success">
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 12.5 9.5 18 20 6.5"/></svg>
        </div>
        <h2 class="wizard-name">${t('wizard.done')}</h2>
        <p class="wizard-desc">${t('wizard.doneDesc', { name: esc(dispName(info)) })}</p>
        <div class="wizard-actions">
          <button class="confirm-btn cancel" data-act="done">${t('common.done')}</button>
          <button class="confirm-btn primary" data-act="load">${t('wizard.loadNow')}</button>
        </div>
      </div>`;

    this.body.querySelector('[data-act="done"]').onclick = () => this.close();
    const loadBtn = this.body.querySelector('[data-act="load"]');
    loadBtn.onclick = async () => {
      const gen = this._gen;
      loadBtn.disabled = true;
      loadBtn.textContent = t('wizard.loading');
      try {
        await API.loadSkin(info.id);
        loadBtn.textContent = t('wizard.loaded');
        // 引导任务已完成：短暂展示成功反馈后自动关闭
        // （gen 校验：期间若被新双击换包，不去关新向导）
        setTimeout(() => { if (gen === this._gen) this.close(); }, 500);
      } catch (err) {
        // 代际守卫（与 _install 同款）：在途 loadSkin 期间用户双击换包后，
        // 失败卡片不得渲染进新向导
        if (!this.overlay || gen !== this._gen) return;
        loadBtn.disabled = false;
        loadBtn.textContent = t('wizard.loadNow');
        this._renderFailure(t('wizard.loadFailed'), err);
      }
    };
  }

  // ── 失败态（检查 / 安装 / 加载共用）──
  _renderFailure(title, err) {
    if (!this.overlay) return;
    this.body.innerHTML = `
      <div class="wizard-card">
        <div class="wizard-result-icon error">
          <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="5" y1="5" x2="19" y2="19"/><line x1="19" y1="5" x2="5" y2="19"/></svg>
        </div>
        <h2 class="wizard-name">${esc(title)}</h2>
        <p class="wizard-error-text">${esc(String(err))}</p>
        <div class="wizard-actions">
          <button class="confirm-btn primary" data-act="close">${t('common.close')}</button>
        </div>
      </div>`;
    this.body.querySelector('[data-act="close"]').onclick = () => this.close();
  }

  _renderStatus(text, sub) {
    if (!this.overlay) return;
    this.body.innerHTML = `
      <div class="wizard-status">
        <div class="wizard-spinner"></div>
        <p>${esc(text)}</p>
        ${sub ? `<p class="wizard-status-sub">${esc(sub)}</p>` : ''}
      </div>`;
  }
}
