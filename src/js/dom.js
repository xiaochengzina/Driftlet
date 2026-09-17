/**
 * dom.js — 共享 DOM 小工具
 *
 * esc / escAttr：innerHTML 拼接前的转义（管理器界面大量字符串拼 DOM）。
 * dispName / dispDesc：皮肤文案选取（单语言皮肤回退到其提供的语言），
 * skin-list / skin-editor / install-wizard / settings / focus 同一规则，唯一定义在此。
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

// 皮肤文案选取（对称字段：name_zh/name_en、description_zh/description_en——
// 旧无后缀字段名经后端 serde alias 解析进 *_zh，下发即此形态）：
// 界面语言优先取对应语言字段，缺失（undefined 或空串）回退另一语言——
// 单语言皮肤（只填一种语言）在中/英界面下都显示创作者提供的那种语言；
// 两个都填 = 双语皮肤随界面切换。任何字段组合都合法，无声明开关。
export function dispName(info) {
  const zh = info?.name_zh || '';
  const en = info?.name_en || '';
  return getLang() === 'en' ? (en || zh) : (zh || en);
}

export function dispDesc(info) {
  const zh = info?.description_zh || '';
  const en = info?.description_en || '';
  return getLang() === 'en' ? (en || zh) : (zh || en);
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
 * 热键录制绑定（设置页全局热键与编辑器皮肤专属热键共用——单一口源，
 * 勿再复制第三份）：点击进入录制态，Esc 取消，Backspace/Delete 清空，
 * 合法组合（≥1 修饰键 + 普通键）调 onSave(combo)（可 async；显示态
 * 由调用方在 onSave 里自绘）。录制期按钮只显短文案 recordingText。
 * 操作提示（Esc 取消等）写在行的描述 hint 里，不进按钮（实机反馈）。
 * 返回 { unbind }——宿主销毁/重绘/防叠开前必须调用，否则 window 级
 * capture 监听残留劫持键盘。
 * 注意：录制期间按下已注册热键仍会真实触发一次显隐切换（全局热键
 * 无法局部屏蔽，已知小怪癖）。
 */
export function bindHotkeyCapture(btn, { recordingText, onSave }) {
  let listener = null;
  btn.addEventListener('click', () => {
    if (listener) return; // 已在录制中
    const prevText = btn.textContent;
    btn.textContent = recordingText;
    btn.classList.add('active');

    const finish = () => {
      window.removeEventListener('keydown', onKey, true);
      listener = null;
      btn.classList.remove('active');
      btn.textContent = prevText;
    };
    const onKey = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') { finish(); return; }
      if (e.key === 'Backspace' || e.key === 'Delete') {
        finish();
        onSave('');
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
      onSave([...mods, key].join('+'));
    };
    listener = onKey;
    window.addEventListener('keydown', onKey, true);
  });
  return {
    unbind() {
      if (listener) {
        window.removeEventListener('keydown', listener, true);
        listener = null;
        btn.classList.remove('active');
      }
    },
  };
}

/**
 * 确认弹窗工厂。统一行为：Esc 关闭、点遮罩关闭、初始焦点落「取消」
 * （危险操作焦点不放确认键）；danger 时确认按钮加 danger class。
 * 确认点击后先关弹窗再执行 onConfirm（可为 async）；
 * onCancel 仅在未确认关闭（取消按钮 / Esc / 点遮罩）时调用。
 */
export function confirmDialog({ title, bodyHtml, hint, confirmText, danger = false, wide = false, onCancel, onConfirm }) {
  const overlay = document.createElement('div');
  overlay.className = 'confirm-overlay';
  overlay.innerHTML = `
    <div class="confirm-dialog${wide ? ' wide' : ''}">
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
