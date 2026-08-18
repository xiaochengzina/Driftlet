'use strict';

/**
 * web-view —— 通用网页皮肤（iframe 内嵌）。
 *
 * 站点区是目标页面（iframe，地址来自 site_url 设置项）——cookie 与
 * WebView2 用户数据目录同罐，登录一次跨重启持久化；本地外壳保留完整桥
 * 能力（.drag-region 拖动、宿主右键菜单、皮肤设置）。
 * 定时刷新：跨域 iframe 不能碰 contentWindow，重赋值同 src 即重载；
 * 页面隐藏暂停、恢复可见立即补刷。
 * 未配置地址时显示引导态，iframe 不加载。
 */

const I18N = {
  'zh-CN': {
    title: '网页内嵌',
    btnRefresh: '立即刷新',
    emptyHint: '在管理器「皮肤设置」页填写站点地址（http/https）',
  },
  en: {
    title: 'Web View',
    btnRefresh: 'Refresh now',
    emptyHint: 'Set the site URL (http/https) on the manager\'s Skin Settings page',
  },
};

let lang = 'zh-CN';
let timer = null;
let currentUrl = '';

function t(key) {
  return (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key] ?? key;
}

function siteUrl() {
  const raw = String(window.__DESK_PP__?.settings?.site_url || '').trim();
  // 协议白名单：只允许 http/https（javascript:/data: 直达 iframe.src 会
  // 继承皮肤源执行——设置文案也只承诺 http/https；其余按未配置处理）
  return /^https?:\/\//i.test(raw) ? raw : '';
}

function setDot(state) {
  const el = document.getElementById('status-dot');
  el.className = 'dot' + (state ? ' ' + state : '');
  // 平台局限：跨域 iframe 的导航失败（如目标站 X-Frame-Options 拒绝嵌入）
  // 通常仍派发 load 事件（error 事件对导航失败几乎不触发）——状态点对
  // 这类失败会误显 ok，是 Chromium 的行为边界，不要依赖它诊断嵌站失败。
}

function reloadFrame() {
  const url = siteUrl();
  if (!url) return;
  const f = document.getElementById('frame');
  const btn = document.getElementById('btn-refresh');
  btn.classList.add('spin');
  setDot('loading');
  // 同 src 重赋值即重载（跨域不能碰 contentWindow.location）
  f.src = url;
}

/** 地址变化（含从空到有）：切换引导态/iframe、重载。返回是否触发了重载
    （visibilitychange 补刷据此避免地址变化时双重加载）。 */
function syncUrl() {
  const url = siteUrl();
  const f = document.getElementById('frame');
  const empty = document.getElementById('empty');
  const has = !!url;
  f.hidden = !has;
  empty.hidden = has;
  document.getElementById('btn-refresh').style.visibility = has ? '' : 'hidden';
  if (!has) {
    currentUrl = '';
    setDot('');
    return false;
  }
  if (url !== currentUrl) {
    currentUrl = url;
    reloadFrame();
    return true;
  }
  return false;
}

function restartTimer() {
  if (timer) clearInterval(timer);
  const minutes = Math.max(1, Number(window.__DESK_PP__?.settings?.refresh_min) || 5);
  timer = setInterval(() => {
    if (!document.hidden && siteUrl()) reloadFrame();
  }, minutes * 60 * 1000);
}

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll('[data-i18n-title]').forEach((el) => {
    el.title = t(el.dataset.i18nTitle);
  });
}

function boot() {
  lang = window.__DESK_PP__?.language || 'zh-CN';
  applyI18n();

  const f = document.getElementById('frame');
  f.addEventListener('load', () => {
    document.getElementById('btn-refresh').classList.remove('spin');
    setDot('ok');
  });
  f.addEventListener('error', () => setDot('err'));

  document.getElementById('btn-refresh').onclick = reloadFrame;
  document.addEventListener('desk-setting-changed', (e) => {
    if (e.detail?.key === 'refresh_min') restartTimer();
    if (e.detail?.key === 'site_url') syncUrl();
  });
  document.addEventListener('desk-language-changed', (e) => {
    lang = e.detail?.language || lang;
    applyI18n();
  });
  document.addEventListener('visibilitychange', () => {
    // syncUrl 返回 true = 地址变了已自行重载——再 reloadFrame 会双重加载
    if (!document.hidden) { if (!syncUrl() && siteUrl()) reloadFrame(); }
  });
  // 兜底对齐：窗口重新获得焦点时也对齐一次（管理器写入发生在皮肤失焦时
  // 也覆盖到——事件通道本身是有的，这里是第二道保险）
  window.addEventListener('focus', syncUrl);

  restartTimer();
  syncUrl(); // 首载（有地址即加载）
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', boot);
} else {
  boot();
}
