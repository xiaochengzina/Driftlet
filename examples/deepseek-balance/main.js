'use strict';

/**
 * deepseek-balance —— DeepSeek API 账户余额自动查询。
 *
 * 演示四件事：
 * 1. 皮肤页直接 fetch 外部 REST 接口（§3.4）：DeepSeek 余额接口返回 CORS 允许头，
 *    跨域 GET 无需任何权限声明；
 * 2. password 类型设置项（API Key）不注入页面，经 skin_get_setting 按窗口身份读取（§4.3）；
 * 3. 定时自动查询 + 页面隐藏暂停 / 恢复可见即补查 + 手动刷新按钮；
 * 4. 中英双语跟随管理器语言（desk-language-changed），动态内容全程 textContent / DOM API。
 *
 * 接口：GET https://api.deepseek.com/user/balance
 *   响应：{ is_available: bool,
 *           balance_infos: [{ currency: "CNY"|"USD",
 *                             total_balance / granted_balance / topped_up_balance: string }] }
 */

const API_URL = 'https://api.deepseek.com/user/balance';

const I18N = {
  'zh-CN': {
    title: 'DeepSeek 余额',
    noKey: '尚未配置 API Key',
    noKeyHint: '在管理器「皮肤设置」页填写 DeepSeek 开放平台的 API Key，保存后自动开始查询',
    loading: '查询中…',
    updated: (time) => `更新于 ${time}`,
    never: '尚未成功查询',
    noData: '账户暂无余额信息',
    refreshTip: '立即刷新',
    topUp: '充值',
    topUpTip: '打开 DeepSeek 充值页',
    granted: '赠金',
    toppedUp: '充值余额',
    statusOk: '正常',
    statusLow: '余额不足',
    statusError: '查询失败',
    errAuth: 'API Key 无效，请在皮肤设置中检查',
    errHttp: (code) => `接口返回 HTTP ${code}`,
    errNetwork: '网络异常，下个周期自动重试',
    errTimeout: '查询超时，下个周期自动重试',
    every: (min) => `每 ${min} 分钟自动查询`,
    notifyTitle: 'DeepSeek 余额不足',
    notifyBodyBelow: (list, th) => `总余额 ${list}，已低于预警线 ${th}`,
    notifyBodyUnavailable: '账户余额已不可用，请及时充值',
  },
  en: {
    title: 'DeepSeek Balance',
    noKey: 'No API key configured',
    noKeyHint: 'Fill in your DeepSeek platform API key on the manager\'s Skin Settings page; querying starts automatically once saved',
    loading: 'Querying…',
    updated: (time) => `Updated ${time}`,
    never: 'No successful query yet',
    noData: 'No balance info for this account',
    refreshTip: 'Refresh now',
    topUp: 'Top up',
    topUpTip: 'Open the DeepSeek top-up page',
    granted: 'Granted',
    toppedUp: 'Topped up',
    statusOk: 'OK',
    statusLow: 'Low balance',
    statusError: 'Query failed',
    errAuth: 'Invalid API key — check the skin settings',
    errHttp: (code) => `API returned HTTP ${code}`,
    errNetwork: 'Network error; retrying on the next cycle',
    errTimeout: 'Query timed out; retrying on the next cycle',
    every: (min) => `Auto-queries every ${min} min`,
    notifyTitle: 'DeepSeek balance low',
    notifyBodyBelow: (list, th) => `Balance ${list} is below the warning line ${th}`,
    notifyBodyUnavailable: 'The account balance is unavailable — please top up',
  },
};

const CURRENCY_SYMBOL = { CNY: '¥', USD: '$' };
const TOP_UP_URL = 'https://platform.deepseek.com/top_up';

let lang = 'zh-CN';
let settings = {};        // __DESK_PP__.settings（api_key 是 password 类型，这里恒为空串）
let apiKey = '';          // 真实 API Key（skin_get_setting 单独读取）
let balanceInfos = null;  // 最近一次成功的 balance_infos 数组
let isAvailable = true;   // 最近一次成功的 is_available
let lastError = '';       // 最近一次失败的本地化文案（成功时清空）
let lastUpdated = null;   // 最近一次成功的时间（Date）
let loading = false;
let inflightCtrl = null;  // 在飞请求的 AbortController（换 Key 时掐断，见 fetchBalance）
let pollTimer = null;
let wasLow = false;       // 上一次成功查询时是否处于低余额（通知边沿触发用）

const el = {};

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key];
  return typeof entry === 'function' ? entry(...args) : (entry ?? key);
}

function intervalMinutes() {
  const n = Number(settings.refresh_minutes);
  return Number.isFinite(n) && n > 0 ? Math.max(1, Math.min(1440, Math.round(n))) : 30;
}

/** 余额是服务端字符串（如 "100.00"），原样展示；异常值兜底为占位符 */
function fmt(v) {
  const s = typeof v === 'string' ? v.trim() : String(v ?? '');
  return s || '—';
}

/* ── 查询 ─────────────────────────────────────────────────── */

async function fetchBalance() {
  if (!apiKey || loading) return;
  loading = true;
  render();

  // 换 Key 竞态防护：fetch 开始时锁定当次 Key，落地前校验 Key 未变才写
  // 状态（旧 Key 的在飞响应不得覆盖清空态）；Key 变更时由事件处理器直接
  // abort 在飞请求（见 desk-setting-changed 里的 apiKey 分支）。
  const fetchKey = apiKey;
  const ctrl = new AbortController();
  inflightCtrl = ctrl;
  const timer = setTimeout(() => ctrl.abort(), 15000);
  try {
    const res = await fetch(API_URL, {
      method: 'GET',
      headers: { 'Authorization': `Bearer ${fetchKey}`, 'Accept': 'application/json' },
      signal: ctrl.signal,
      cache: 'no-store',
    });
    if (res.status === 401 || res.status === 403) throw new Error(t('errAuth'));
    if (!res.ok) throw new Error(t('errHttp', res.status));
    const data = await res.json();
    if (fetchKey !== apiKey) return; // Key 已在飞行中换掉——不落地
    balanceInfos = Array.isArray(data?.balance_infos) ? data.balance_infos : [];
    isAvailable = data?.is_available !== false;
    lastError = '';
    lastUpdated = new Date();
    // 低余额边沿触发：仅在「非低 → 低」跳变时发一次通知，余额回升后重新武装
    const low = computeLow();
    if (low && !wasLow && settings.notify_on_low !== false) sendLowNotification();
    wasLow = low;
  } catch (err) {
    if (err?.name === 'AbortError') lastError = t('errTimeout');
    else if (err instanceof TypeError) lastError = t('errNetwork'); // fetch 网络层失败
    else lastError = err?.message || t('errNetwork');
  } finally {
    clearTimeout(timer);
    if (inflightCtrl === ctrl) inflightCtrl = null;
    loading = false;
    render();
  }
}

/* ── 低余额预警与通知 ─────────────────────────────────────── */

/** 低于预警线的币种条目（预警线 0 = 关闭阈值预警） */
function lowEntries() {
  const th = Number(settings.warn_threshold);
  if (!balanceInfos || !(th > 0)) return [];
  return balanceInfos.filter((i) => {
    const v = parseFloat(i?.total_balance);
    return Number.isFinite(v) && v < th;
  });
}

/** 低余额 = 接口报告不可用，或任一币种跌入预警线以下 */
function computeLow() {
  if (!balanceInfos) return false;
  return !isAvailable || lowEntries().length > 0;
}

function sendLowNotification() {
  if (!window.__DESK_PP__?.invoke) return;
  const lows = lowEntries();
  const body = lows.length
    ? t('notifyBodyBelow',
        lows.map((i) => `${i?.currency || ''} ${fmt(i?.total_balance)}`).join(lang === 'en' ? ', ' : '，'),
        Number(settings.warn_threshold))
    : t('notifyBodyUnavailable');
  window.__DESK_PP__.invoke('show_notification', { title: t('notifyTitle'), body })
    .catch((err) => console.warn('deepseek-balance: show_notification 失败', err));
}

/* ── 定时调度：间隔轮询，页面隐藏暂停，恢复可见即补查 ─────── */

function schedule() {
  if (pollTimer !== null) { clearInterval(pollTimer); pollTimer = null; }
  // 隐藏暂停不得被「设置变更 → schedule() 重建定时器」绕过——隐藏时
  // 直接不起定时器（恢复可见时 visibilitychange 会补查并重建）
  if (document.hidden) return;
  pollTimer = setInterval(fetchBalance, intervalMinutes() * 60000);
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) {
    if (pollTimer !== null) { clearInterval(pollTimer); pollTimer = null; }
  } else {
    fetchBalance();
    schedule();
  }
});

/* ── 渲染（全程 DOM API / textContent） ───────────────────── */

function makeNote(big, small, cls) {
  const div = document.createElement('div');
  div.className = 'note' + (cls ? ' ' + cls : '');
  const b = document.createElement('div');
  b.className = 'big';
  b.textContent = big;
  div.appendChild(b);
  if (small) {
    const s = document.createElement('div');
    s.className = 'small';
    s.textContent = small;
    div.appendChild(s);
  }
  return div;
}

function makePart(label, value) {
  const div = document.createElement('div');
  div.className = 'part';
  const l = document.createElement('label');
  l.textContent = label;
  const b = document.createElement('b');
  b.textContent = value;
  div.appendChild(l);
  div.appendChild(b);
  return div;
}

function makeBalanceBlock(info, multi) {
  const sec = document.createElement('section');
  sec.className = 'bal';

  const row = document.createElement('div');
  row.className = 'total-row';
  if (multi) {
    const tag = document.createElement('span');
    tag.className = 'cur-tag';
    tag.textContent = String(info?.currency || '');
    row.appendChild(tag);
  }
  const total = document.createElement('div');
  total.className = 'total';
  // 预警标黄：接口报告不可用时全部币种标黄；否则仅低于预警线的币种标黄
  const th = Number(settings.warn_threshold);
  const v = parseFloat(info?.total_balance);
  if (!isAvailable || (th > 0 && Number.isFinite(v) && v < th)) total.classList.add('low');
  const cur = String(info?.currency || '');
  const sym = document.createElement('span');
  sym.className = 'sym';
  sym.textContent = CURRENCY_SYMBOL[cur] || (cur ? cur + ' ' : '');
  const num = document.createElement('span');
  num.textContent = fmt(info?.total_balance);
  total.appendChild(sym);
  total.appendChild(num);
  row.appendChild(total);
  sec.appendChild(row);

  if (settings.show_breakdown !== false) {
    const parts = document.createElement('div');
    parts.className = 'parts';
    // 赠金常规为 0（DeepSeek 基本不发放），仅在真实存在时才显示
    const g = parseFloat(info?.granted_balance);
    if (Number.isFinite(g) && g > 0) parts.appendChild(makePart(t('granted'), fmt(info?.granted_balance)));
    parts.appendChild(makePart(t('toppedUp'), fmt(info?.topped_up_balance)));
    sec.appendChild(parts);
  }
  return sec;
}

function renderBody() {
  el.body.textContent = '';

  if (!apiKey) {
    el.body.appendChild(makeNote(t('noKey'), t('noKeyHint'), ''));
    return;
  }
  if (balanceInfos) {
    if (!balanceInfos.length) {
      el.body.appendChild(makeNote(t('noData'), '', ''));
      return;
    }
    const multi = balanceInfos.length > 1;
    for (const info of balanceInfos) el.body.appendChild(makeBalanceBlock(info, multi));
    return;
  }
  if (loading) {
    el.body.appendChild(makeNote(t('loading'), '', ''));
    return;
  }
  if (lastError) {
    el.body.appendChild(makeNote(lastError, t('never'), 'error'));
    return;
  }
  el.body.appendChild(makeNote(t('loading'), '', ''));
}

function renderBadge() {
  const badge = el.badge;
  badge.classList.remove('ok', 'low', 'err');
  if (!apiKey || (!balanceInfos && !lastError)) { badge.hidden = true; return; }
  badge.hidden = false;
  if (lastError) {
    badge.classList.add('err');
    el.badgeText.textContent = t('statusError');
  } else if (computeLow()) {
    badge.classList.add('low');
    el.badgeText.textContent = t('statusLow');
  } else {
    badge.classList.add('ok');
    el.badgeText.textContent = t('statusOk');
  }
}

function renderFooter() {
  if (!apiKey) {
    el.updated.textContent = '';
  } else if (lastUpdated) {
    const time = lastUpdated.toTimeString().slice(0, 8);
    el.updated.textContent = lastError ? `${t('updated', time)} · ${lastError}` : t('updated', time);
  } else if (lastError) {
    el.updated.textContent = lastError;
  } else {
    el.updated.textContent = t('every', intervalMinutes());
  }
  el.refresh.classList.toggle('spin', loading);
  el.refresh.disabled = loading;
  el.refresh.title = t('refreshTip');
}

function render() {
  el.title.textContent = t('title');
  if (el.topup) {
    el.topup.textContent = t('topUp');
    el.topup.title = t('topUpTip');
  }
  const c = settings.accent_color;
  el.shell.style.setProperty('--accent', /^#[0-9a-fA-F]{6}/.test(c || '') ? c : '#4d6bfe');
  renderBody();
  renderBadge();
  renderFooter();
}

/* ── 事件 ─────────────────────────────────────────────────── */

function bindEvents() {
  el.refresh.addEventListener('click', fetchBalance);

  // 打开充值页（system 权限 open_external；纯浏览器调试时兜底 window.open）
  el.topup = document.getElementById('topup');
  el.topup.addEventListener('click', () => {
    if (window.__DESK_PP__?.invoke) {
      window.__DESK_PP__.invoke('open_external', { target: TOP_UP_URL })
        .catch((err) => console.warn('deepseek-balance: open_external 失败', err));
    } else {
      window.open(TOP_UP_URL, '_blank');
    }
  });

  // 管理器改设置后实时应用；password 变更事件照常携带新值（§4.3）
  document.addEventListener('desk-setting-changed', (e) => {
    const { key, value } = e.detail || {};
    if (!key) return;
    settings[key] = value;
    if (key === 'api_key') {
      apiKey = value || '';
      // 换/删 Key 时直接掐断在飞的旧 Key 请求（其落地已被 fetchKey 校验挡住，
      // abort 只是让它早点释放）
      inflightCtrl?.abort();
      balanceInfos = null;
      lastError = '';
      lastUpdated = null;
      fetchBalance();
      schedule();
    } else if (key === 'refresh_minutes') {
      schedule();
    }
    // 预警线/通知开关改动立即反映到徽标与标黄；静默同步通知边沿，改设置本身不触发通知
    wasLow = computeLow();
    render();
  });

  document.addEventListener('desk-language-changed', () => {
    lang = window.__DESK_PP__?.language === 'en' ? 'en' : 'zh-CN';
    document.documentElement.lang = lang;
    render();
  });
}

/* ── 启动 ─────────────────────────────────────────────────── */

async function boot() {
  for (const id of ['shell', 'title', 'badge', 'badge-text', 'body', 'updated', 'refresh']) {
    el[id.replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = document.getElementById(id);
  }

  // 桥不存在（纯浏览器调试）时按无 Key 状态离线渲染，不抛错（§7.2）
  lang = window.__DESK_PP__?.language === 'en' ? 'en' : 'zh-CN';
  document.documentElement.lang = lang;
  settings = window.__DESK_PP__?.settings || {};
  if (window.__DESK_PP__?.invoke) {
    try {
      apiKey = (await window.__DESK_PP__.invoke('skin_get_setting', { key: 'api_key' })) || '';
    } catch (err) {
      console.warn('deepseek-balance: skin_get_setting 失败', err);
      apiKey = '';
    }
  }

  bindEvents();
  render();
  fetchBalance();
  schedule();
}

boot();
