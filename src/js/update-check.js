/**
 * update-check.js — 启动更新检测与更新弹窗
 *
 * 开关存 config.update_check（默认开）。启动时后台查 GitHub 最新 release，
 * 发现新版本则亮出管理器窗口（平时隐藏启动在托盘）并弹提示：
 *   - 「前往下载」：打开 GitHub 最新 release 页（URL 后端固定），关闭弹窗；
 *   - 「取消」：仅关闭；若勾选了「不再提示更新」，取消 = 关闭更新检测，
 *     并补一个「已关闭」告知弹窗（可在设置页重新开启）。
 * 网络失败 / 无更新一律静默（仅 console 记录），不打断启动。
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import API from './api.js';
import { t } from './i18n.js';

export async function initUpdateCheck() {
  try {
    const config = await API.getAppConfig();
    if (config.update_check === false) return;
    const result = await API.checkUpdate();
    if (!result?.has_update) return;
    // 弹提示前先把窗口亮出来（隐藏启动时弹窗画了也看不见）
    const win = getCurrentWindow();
    await win.unminimize();
    await win.show();
    await win.setFocus();
    showUpdateDialog(result);
  } catch (err) {
    console.error('update check failed:', err);
  }
}

function showUpdateDialog(result) {
  const overlay = document.createElement('div');
  overlay.className = 'confirm-overlay';
  overlay.innerHTML = `
    <div class="confirm-dialog">
      <div class="confirm-icon"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg></div>
      <h3>${t('update.title')}</h3>
      <p>${t('update.body', { latest: esc(result.latest_version), current: esc(result.current_version) })}</p>
      <label class="update-remind"><input type="checkbox" id="update-dont-remind"><span>${t('update.dontRemind')}</span></label>
      <div class="confirm-buttons">
        <button class="confirm-btn cancel">${t('common.cancel')}</button>
        <button class="confirm-btn primary">${t('update.download')}</button>
      </div>
    </div>`;
  document.body.appendChild(overlay);

  const cancelBtn = overlay.querySelector('.confirm-btn.cancel');
  const goBtn = overlay.querySelector('.confirm-btn.primary');
  const dontRemind = overlay.querySelector('#update-dont-remind');

  const close = () => {
    window.removeEventListener('keydown', onKey, true);
    overlay.remove();
  };
  // 取消：勾选「不再提示」时同时关闭更新检测，并补告知弹窗
  const cancel = async () => {
    const stop = dontRemind.checked;
    close();
    if (!stop) return;
    try {
      await API.setUpdateCheck(false);
    } catch (err) {
      console.error('setUpdateCheck failed:', err);
    }
    showDisabledNotice();
  };
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    }
  };

  cancelBtn.onclick = cancel;
  goBtn.onclick = async () => {
    close();
    try {
      await API.openReleasePage();
    } catch (err) {
      console.error('openReleasePage failed:', err);
    }
  };
  window.addEventListener('keydown', onKey, true);
  // 与删除/重置确认框一致：初始焦点落在「取消」
  cancelBtn.focus();
}

/** 「更新检测已关闭」告知弹窗（勾选不再提示后取消时弹出） */
function showDisabledNotice() {
  const overlay = document.createElement('div');
  overlay.className = 'confirm-overlay';
  overlay.innerHTML = `
    <div class="confirm-dialog">
      <div class="confirm-icon"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg></div>
      <h3>${t('update.disabledTitle')}</h3>
      <p>${t('update.disabledBody')}</p>
      <div class="confirm-buttons">
        <button class="confirm-btn primary">${t('update.ok')}</button>
      </div>
    </div>`;
  document.body.appendChild(overlay);

  const okBtn = overlay.querySelector('.confirm-btn.primary');
  const close = () => {
    window.removeEventListener('keydown', onKey, true);
    overlay.remove();
  };
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  };
  okBtn.onclick = close;
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) close();
  });
  window.addEventListener('keydown', onKey, true);
  okBtn.focus();
}

function esc(str) {
  const div = document.createElement('div');
  div.textContent = String(str ?? '');
  return div.innerHTML;
}
