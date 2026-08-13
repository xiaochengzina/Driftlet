/**
 * dom.js — 共享 DOM 小工具
 *
 * esc / escAttr：innerHTML 拼接前的转义（管理器界面大量字符串拼 DOM）。
 * dispName / dispDesc：双语皮肤文案选取，skin-list / skin-editor /
 * install-wizard 同一规则，唯一定义在此。
 * confirmDialog：确认弹窗工厂（删除/重置/导入备份共用）；bindEsc /
 * closeOnMaskClick 是其拆出的小工具，结构特殊的弹窗（如更新提示）
 * 可只复用小工具而不套用工厂。
 */
import { t, getLang } from './i18n.js';

/** HTML 文本转义：& < > 转义，引号保留（故不能直接用于属性值，用 escAttr） */
export function esc(str) {
  const div = document.createElement('div');
  div.textContent = String(str ?? '');
  return div.innerHTML;
}

/** 双引号属性值转义：esc() 不转义引号，此处补 " → &quot; */
export function escAttr(str) {
  return esc(str).replace(/"/g, '&quot;');
}

// 双语皮肤（skin.json 声明 bilingual）：英文界面优先显示 *_en 文案，
// 字段留空回退默认文案
export function dispName(info) {
  return (getLang() === 'en' && info?.bilingual && info?.name_en) || info?.name;
}

export function dispDesc(info) {
  return (getLang() === 'en' && info?.bilingual && info?.description_en) || info?.description;
}

/** Esc 关闭：window 级 capture keydown；返回解绑函数，关闭后必须调用摘除 */
export function bindEsc(onEsc) {
  const onKey = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      onEsc();
    }
  };
  window.addEventListener('keydown', onKey, true);
  return () => window.removeEventListener('keydown', onKey, true);
}

/** 点遮罩空白区关闭（点在对话框内部不关） */
export function closeOnMaskClick(overlay, close) {
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) close();
  });
}

/**
 * 确认弹窗工厂。统一行为：Esc 关闭、点遮罩关闭、初始焦点落「取消」
 * （危险操作焦点不放确认键）；danger 时确认按钮加 danger class。
 * 确认点击后先关弹窗再执行 onConfirm（可为 async）；
 * onCancel 仅在未确认关闭（取消按钮 / Esc / 点遮罩）时调用。
 */
export function confirmDialog({ title, bodyHtml, hint, confirmText, danger = false, onCancel, onConfirm }) {
  const overlay = document.createElement('div');
  overlay.className = 'confirm-overlay';
  overlay.innerHTML = `
    <div class="confirm-dialog">
      <div class="confirm-icon${danger ? ' danger' : ''}"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg></div>
      <h3>${title}</h3>
      <p>${bodyHtml}</p>
      ${hint ? `<p class="confirm-hint">${hint}</p>` : ''}
      <div class="confirm-buttons">
        <button class="confirm-btn cancel">${t('common.cancel')}</button>
        <button class="confirm-btn${danger ? ' danger' : ' primary'}">${confirmText}</button>
      </div>
    </div>`;
  document.body.appendChild(overlay);

  // 未确认关闭（取消按钮 / Esc / 点遮罩）
  function close() {
    unbindEsc();
    overlay.remove();
    onCancel?.();
  }
  const unbindEsc = bindEsc(close);
  closeOnMaskClick(overlay, close);

  const cancelBtn = overlay.querySelector('.confirm-btn.cancel');
  cancelBtn.onclick = close;
  overlay.querySelector('.confirm-btn:not(.cancel)').onclick = async () => {
    unbindEsc();
    overlay.remove();
    await onConfirm?.();
  };
  cancelBtn.focus();
}
