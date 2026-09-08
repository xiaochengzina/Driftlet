/**
 * update-check.js — 启动更新检测与更新弹窗
 *
 * 开关存 config.update_check（默认开）。启动时后台查 GitHub 最新 release，
 * 发现新版本立即亮出管理器窗口（平时隐藏启动在托盘）并弹提示，安装包
 * 同时在后台多源竞速下载（直连 GitHub + 加速镜像，官方 SHA-256 在时镜像
 * 才参与、落盘哈希不符即废），进度经 update-download-progress 事件推给
 * 弹窗里的进度条：
 *   - 下载完成 → 主钮变「立即安装」；全部源失败 → 主钮变「前往下载」
 *     （打开 GitHub 最新 release 页，URL 后端固定）；该版本安装包已就位
 *     （上次下载完或点过「稍后」，后端核对哈希）→ 跳过下载直接「立即安装」；
 *   - 「取消」：仅关闭（下载仍在后台跑完，下次启动免重下）；勾选
 *     「不再提示更新」时取消 = 关闭更新检测，并补「已关闭」告知弹窗
 *     （可在设置页重新开启）。
 * 网络失败 / 无更新一律静默（仅 console 记录），不打断启动。
 */
import { getCurrentWindow } from '@tauri-apps/api/window';
import { listen } from '@tauri-apps/api/event';
import API from './api.js';
import { t } from './i18n.js';
import { esc, bindEsc, closeOnMaskClick } from './dom.js';

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
  // ready = 安装包已就位且通过核对；downloading = 后台下载中；
  // manual = 无安装包资产或全部下载源失败（主钮 = 前往下载页）
  const mode = {
    current: result.installer_ready ? 'ready' : result.installer_url ? 'downloading' : 'manual',
  };

  const overlay = document.createElement('div');
  overlay.className = 'confirm-overlay';
  overlay.innerHTML = `
    <div class="confirm-dialog">
      <div class="confirm-icon"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg></div>
      <h3>${t('update.title')}</h3>
      <p class="update-body">${t(mode.current === 'ready' ? 'update.bodyDownloaded' : 'update.body', { latest: esc(result.latest_version), current: esc(result.current_version) })}</p>
      <div class="update-progress-wrap" hidden>
        <div class="update-progress"><div class="update-progress-fill"></div></div>
        <div class="update-progress-text"></div>
      </div>
      <label class="update-remind"><input type="checkbox" id="update-dont-remind"><span>${t('update.dontRemind')}</span></label>
      <div class="confirm-buttons">
        <button class="confirm-btn cancel">${t('common.cancel')}</button>
        <button class="confirm-btn primary"></button>
      </div>
    </div>`;
  document.body.appendChild(overlay);

  const bodyEl = overlay.querySelector('.update-body');
  const wrap = overlay.querySelector('.update-progress-wrap');
  const bar = overlay.querySelector('.update-progress');
  const fill = overlay.querySelector('.update-progress-fill');
  const progressText = overlay.querySelector('.update-progress-text');
  const cancelBtn = overlay.querySelector('.confirm-btn.cancel');
  const goBtn = overlay.querySelector('.confirm-btn.primary');
  const dontRemind = overlay.querySelector('#update-dont-remind');

  function render() {
    const m = mode.current;
    goBtn.textContent =
      m === 'ready' ? t('update.installNow') : m === 'downloading' ? t('update.downloading') : t('update.download');
    goBtn.disabled = m === 'downloading';
    wrap.hidden = m !== 'downloading';
  }
  render();

  let unlistenProgress = null;

  function progressRatio(s) {
    return s.total > 0 ? s.downloaded / s.total : 0;
  }

  async function trackProgress() {
    try {
      unlistenProgress = await listen('update-download-progress', (event) => {
        if (!overlay.isConnected) return;
        const { stage, sources } = event.payload || {};
        if (stage === 'verifying') {
          progressText.textContent = t('update.verifying');
          return;
        }
        if (stage !== 'downloading' && stage !== 'connecting') return;
        // 进度按最领先的源推进（多源竞速，胜者通常是它）
        const best = (sources || []).filter((s) => s.downloaded > 0).sort((a, b) => progressRatio(b) - progressRatio(a))[0];
        if (!best) {
          bar.classList.add('indeterminate');
          return;
        }
        const label = best.index === 0 ? t('update.sourceDirect') : t('update.sourceMirror', { n: best.index });
        if (best.total > 0) {
          bar.classList.remove('indeterminate');
          const percent = Math.min(100, Math.floor((best.downloaded / best.total) * 100));
          fill.style.width = `${percent}%`;
          progressText.textContent = t('update.progressPercent', { source: label, percent });
        } else {
          // 单流且服务器未给总大小：滑块态 + 已下载体积
          bar.classList.add('indeterminate');
          const mb = (best.downloaded / 1048576).toFixed(1);
          progressText.textContent = t('update.progressBytes', { source: label, mb });
        }
      });
    } catch (err) {
      console.error('progress listen failed:', err);
    }
  }

  async function startDownload() {
    await trackProgress();
    try {
      await API.downloadUpdate(result.installer_url, result.latest_version, result.installer_sha256 ?? null);
      if (!overlay.isConnected) return;
      mode.current = 'ready';
      bodyEl.innerHTML = t('update.bodyDownloaded', {
        latest: esc(result.latest_version),
        current: esc(result.current_version),
      });
    } catch (err) {
      // 全部源失败：降级「前往下载」；后台日志留失败详情（用户-facing 只给按钮）
      console.error('update download failed:', err);
      if (!overlay.isConnected) return;
      mode.current = 'manual';
    } finally {
      if (unlistenProgress) {
        unlistenProgress();
        unlistenProgress = null;
      }
      if (overlay.isConnected) render();
    }
  }
  if (mode.current === 'downloading') startDownload();

  function close() {
    unbindEsc();
    if (unlistenProgress) {
      unlistenProgress();
      unlistenProgress = null;
    }
    overlay.remove();
  }
  // 取消：勾选「不再提示」时同时关闭更新检测，并补告知弹窗
  async function cancel() {
    const stop = dontRemind.checked;
    close();
    if (!stop) return;
    try {
      await API.setUpdateCheck(false);
      showDisabledNotice();
    } catch (err) {
      // 关闭失败时弹错误而非「已关闭」告知（沉默失败还报成功最误导）
      console.error('setUpdateCheck failed:', err);
      const { default: showToast } = await import('./toast.js');
      showToast(t('common.setFailed') + String(err), 'error');
    }
  }
  const unbindEsc = bindEsc(cancel);

  cancelBtn.onclick = cancel;
  goBtn.onclick = async () => {
    if (mode.current === 'downloading') return; // 下载中主钮禁用，双保险
    close();
    try {
      if (mode.current === 'ready') {
        // 立即安装：后端启动安装包并整站退出（此调用后应用即退出）
        await API.installUpdate();
      } else {
        await API.openReleasePage();
      }
    } catch (err) {
      console.error('update action failed:', err);
      // 安装失败（安装包缺失/启动失败）降级打开下载页
      if (mode.current === 'ready') {
        try { await API.openReleasePage(); } catch (err2) { console.error('openReleasePage failed:', err2); }
      }
    }
  };
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
  function close() {
    unbindEsc();
    overlay.remove();
  }
  const unbindEsc = bindEsc(close);
  okBtn.onclick = close;
  closeOnMaskClick(overlay, close);
  okBtn.focus();
}
