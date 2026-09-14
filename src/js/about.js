/**
 * about.js — 关于面板（功能面板模板实例，docs/设计规范.md §3.3；
 *            导航栏功能预留位规则 §2.2 的第二个落地功能，布局方案同款落位）
 *
 * 内容：品牌区（logo + 名称 + 副题 + 版本芯片）、仓库地址（公开仓库，
 * 系统浏览器打开——open_repo_page 后端固定 URL，不接受前端入参）、
 * 开源协议（GPL-3.0）、手动检查更新（有更新弹同款更新对话框并关闭本
 * 面板；无更新/失败走 toast）。关闭出口 = 右上角 × / Esc / 点遮罩。
 */
import { getVersion } from '@tauri-apps/api/app';
import API from './api.js';
import showToast from './toast.js';
import { t } from './i18n.js';
import { esc, bindEsc, closeOnMaskClick } from './dom.js';
import { checkUpdatesManual } from './update-check.js';

export default class AboutPanel {
  constructor() {
    this.overlay = null;
    this._unbindEsc = null;
    this._version = null;
  }

  async open() {
    if (this.overlay) return; // 防叠开
    if (!this._version) this._version = await getVersion().catch(() => '');
    this.render();
  }

  close() {
    this._unbindEsc?.();
    this._unbindEsc = null;
    this.overlay?.remove();
    this.overlay = null;
  }

  // 用户协议全文：叠一层宽面板（与更新对话框叠放同模式）；文本按界面语言
  // 现取（后端按语言给对应版本，与安装器许可页同一对语言文件）——不缓存，
  // 语言切换后再开自动跟随
  async showAgreement() {
    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = `
      <div class="panel wide about-agreement-panel">
        <div class="panel-head">
          <h2>${t('about.agreement')}</h2>
          <button class="panel-close" title="${t('common.close')}">
            <svg width="11" height="11" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          </button>
        </div>
        <div class="panel-body">
          <div class="about-agreement-text">${esc(t('about.checking'))}</div>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const close = () => { unbindEsc(); overlay.remove(); };
    const unbindEsc = bindEsc(close);
    closeOnMaskClick(overlay, close);
    overlay.querySelector('.panel-close').onclick = close;

    try {
      const text = await API.getUserAgreement();
      // 拉取期间用户可能已关掉面板——面板不在则直接丢弃结果
      if (overlay.isConnected) {
        overlay.querySelector('.about-agreement-text').textContent = text;
      }
    } catch (err) {
      if (overlay.isConnected) {
        overlay.querySelector('.about-agreement-text').textContent = t('about.agreementFail') + String(err);
      }
    }
  }

  render() {
    const overlay = document.createElement('div');
    overlay.className = 'confirm-overlay';
    overlay.innerHTML = `
      <div class="panel about-panel">
        <div class="panel-head">
          <h2>${t('about.title')}</h2>
          <button class="panel-close" title="${t('common.close')}">
            <svg width="11" height="11" viewBox="0 0 12 12"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          </button>
        </div>
        <div class="panel-body">
          <div class="about-brand">
            <div class="about-logo"><img src="/logo.png" alt="Driftlet" draggable="false"></div>
            <div class="about-name">Driftlet</div>
            <div class="about-sub">${t('app.subtitle')}</div>
            ${this._version ? `<div class="about-ver">v${esc(this._version)}</div>` : ''}
          </div>
          <div class="about-rows">
            <div class="settings-row">
              <div>
                <label>${t('about.repo')}</label>
                <div class="hint mono">github.com/xiaochengzina/Driftlet</div>
              </div>
              <div class="btn-cluster">
                <button class="action-btn" id="about-open-repo">${t('about.open')}</button>
              </div>
            </div>
            <div class="settings-row">
              <div>
                <label>${t('about.agreement')}</label>
                <div class="hint">${t('about.agreementHint')}</div>
              </div>
              <div class="btn-cluster">
                <button class="action-btn" id="about-agreement">${t('about.view')}</button>
              </div>
            </div>
            <div class="settings-row">
              <div>
                <label>${t('about.checkUpdate')}</label>
                <div class="hint">${t('about.checkUpdateHint')}</div>
              </div>
              <div class="btn-cluster">
                <button class="action-btn" id="about-check">${t('about.checkUpdate')}</button>
              </div>
            </div>
          </div>
          <div class="about-copy">Copyright © 2026 Driftlet</div>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    this.overlay = overlay;
    this._unbindEsc = bindEsc(() => this.close());
    closeOnMaskClick(overlay, () => this.close());
    overlay.querySelector('.panel-close').onclick = () => this.close();

    // 打开公开仓库（系统浏览器；失败仅 toast，不关面板）
    overlay.querySelector('#about-open-repo').onclick = async () => {
      try {
        await API.openRepoPage();
      } catch (err) {
        showToast(t('common.openFailed') + String(err), 'error');
      }
    };

    // 查看用户协议（叠放全文面板）
    overlay.querySelector('#about-agreement').onclick = () => this.showAgreement();

    // 手动检查更新：有更新弹同款对话框并关闭本面板；无更新/失败 toast
    const checkBtn = overlay.querySelector('#about-check');
    checkBtn.onclick = async () => {
      checkBtn.disabled = true;
      checkBtn.textContent = t('about.checking');
      try {
        const has = await checkUpdatesManual();
        if (has) this.close();
        else showToast(t('about.upToDate'), 'info');
      } catch (err) {
        showToast(t('about.checkFailed') + String(err), 'error');
      } finally {
        if (checkBtn.isConnected) {
          checkBtn.disabled = false;
          checkBtn.textContent = t('about.checkUpdate');
        }
      }
    };
  }
}
