/**
 * toast.js — 全局单例信息提示
 *
 * 同一时刻只显示一条：新提示立即替换旧提示，快速连续操作不会堆叠。
 * 显示 2.2s 后向上淡出（0.2s，见 style.css 的 toastOut），
 * 动画结束后移除元素。
 */
let current = null;
let timer = null;

export default function showToast(msg, type = 'info') {
  // 新提示替换旧提示，避免堆叠
  if (current) {
    current.remove();
    current = null;
  }
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }

  const el = document.createElement('div');
  el.className = `toast ${type}`;
  el.textContent = msg;
  document.body.appendChild(el);
  current = el;

  timer = setTimeout(() => {
    timer = null;
    if (current !== el) return;
    current = null;
    el.classList.add('toast-out');
    el.addEventListener('animationend', () => el.remove(), { once: true });
  }, 2200);
}
