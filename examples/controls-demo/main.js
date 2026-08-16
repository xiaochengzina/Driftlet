'use strict';

/**
 * controls-demo —— 全部 20 种设置控件演示。
 *
 * 演示四件事：
 * 1. 读取自身 skin.json（相对路径 fetch），按当前语言渲染控件标签/分组/选项；
 * 2. 皮肤界面语言跟随管理器：初始取 __DESK_PP__.language，
 *    运行时监听 desk-language-changed 事件即时切换（页面零中英文混排）；
 * 3. 设置值实时应用：desk-setting-changed 事件驱动「应用区」与「值区」刷新；
 * 4. password 类型值不注入页面，经 skin_get_setting 按窗口身份读取。
 *
 * 规范：用户可编辑的值一律走 textContent / DOM API，不拼 innerHTML。
 */

/* ── 皮肤自身的界面文案（与 skin.json 的 schema 文案是两回事：
      schema 的 label_en 等决定【管理器配置面板】的语言；
      这里的字典决定【皮肤页面】的语言） ─────────────────────── */
const I18N = {
  'zh-CN': {
    subtitle: (n) => `演示全部 ${n} 种设置控件 · 界面语言跟随管理器`,
    ticker: (n, sec) => `数字步进驱动：每 ${sec} 秒跳一次 · 已跳 ${n} 次`,
    fallbackName: '控件演示',
    on: '开',
    off: '关',
    notSet: '未设置',
    empty: '（空）',
    secretSet: (n) => `已设置（${n} 个字符）`,
    defaultFont: '默认字体',
    weekdays: { mon: '周一', tue: '周二', wed: '周三', thu: '周四', fri: '周五', sat: '周六', sun: '周日' },
  },
  en: {
    subtitle: (n) => `Demo of all ${n} settings controls · UI language follows the manager`,
    ticker: (n, sec) => `Driven by the stepper: ticks every ${sec}s · ${n} ticks so far`,
    fallbackName: 'Controls Demo',
    on: 'On',
    off: 'Off',
    notSet: 'Not set',
    empty: '(empty)',
    secretSet: (n) => `Set (${n} chars)`,
    defaultFont: 'Default font',
    weekdays: { mon: 'Mon', tue: 'Tue', wed: 'Wed', thu: 'Thu', fri: 'Fri', sat: 'Sat', sun: 'Sun' },
  },
};

let lang = 'zh-CN';          // 当前皮肤界面语言
let schema = null;           // fetch 来的 skin.json
let settings = {};           // __DESK_PP__.settings（password 键恒为空串）
let secretCache = '';        // password 值（skin_get_setting 单独读取）
let tickCount = 0;           // 小秒表已跳次数（stepper 演示）
let tickSec = 5;             // 小秒表当前间隔（秒）
let tickTimer = null;        // 小秒表定时器句柄

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key];
  return typeof entry === 'function' ? entry(...args) : (entry ?? key);
}

/** 取当前语言下的文案：英文优先 en 字段，留空回退默认（与管理器面板同规则） */
function pickLang(zh, en) {
  return (lang === 'en' && en) || zh || en || '';
}

function setLang(next) {
  lang = I18N[next] ? next : 'zh-CN';
  document.documentElement.lang = lang;
  renderAll();
}

/* ── 数据获取 ─────────────────────────────────────────────── */

/** 皮肤可以相对路径 fetch 自己的 skin.json（skin:// 只拦截 settings.json） */
async function fetchSchema() {
  try {
    const res = await fetch('skin.json');
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return await res.json();
  } catch (err) {
    console.warn('controls-demo: 读取 skin.json 失败，按空 schema 渲染', err);
    return { settings: [] };
  }
}

/** password 类型：页面加载前烘焙的 settings 里恒为空串，需经命令单独读取 */
async function loadSecret() {
  if (!window.__DESK_PP__?.invoke) return;
  try {
    secretCache = (await window.__DESK_PP__.invoke('skin_get_setting', { key: 'api_secret' })) || '';
  } catch (err) {
    console.warn('controls-demo: skin_get_setting 失败', err);
    secretCache = '';
  }
}

/* ── 应用区：把部分设置值变成可见效果 ─────────────────────── */

function applySettings() {
  const s = settings;
  const defs = schema?.settings || [];
  const titleDef = defs.find((d) => d.key === 'title');

  // 标题 = title 设置值；空值回退皮肤名（按当前语言）
  document.getElementById('skin-title').textContent =
    s.title || pickLang(schema?.name, schema?.name_en) || t('fallbackName');
  document.getElementById('subtitle').textContent = t('subtitle', defs.length || 20);
  if (titleDef) document.getElementById('skin-title').title = pickLang(titleDef.description, titleDef.description_en);

  // 主题色（palette）：校验格式后应用，防非法值进 style
  const color = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(s.theme_color || '') ? s.theme_color : '#0e75c3';
  document.getElementById('appbar').style.setProperty('--accent', color);
  document.getElementById('progress-bar').style.setProperty('--accent', color);

  // 滑动条（slider）→ 进度条
  const level = Math.max(0, Math.min(100, Number(s.level) || 0));
  document.getElementById('progress-bar').style.width = `${level}%`;

  // 开关（boolean）→ 状态点
  const dot = document.getElementById('status-dot');
  dot.classList.toggle('off', !s.enabled);
  dot.title = s.enabled ? t('on') : t('off');

  // 字体（font）：空串 = 默认
  document.body.style.fontFamily = s.font_family || '';

  // 互斥开关组（radio）→ 背景色调（浅色三档：冷纸/暖白/雾蓝；
  // 旧版 night/auto 存值归一到默认档——只留浅色后不再有深色调）
  document.body.dataset.mode = ['day', 'warm', 'mist'].includes(s.mode) ? s.mode : 'day';

  // 下拉选择（select）→ 面板密度
  document.body.dataset.layout = s.layout === 'compact' ? 'compact' : 'comfy';

  // 小秒表文本随语言/设置刷新（间隔本身的重排在 restartTicker）
  renderTicker();
}

/* 数字步进（stepper）→ 小秒表：按 refresh_sec 的间隔跳动；值在管理器里
   一改，desk-setting-changed 处理会调本函数即时重排定时器（无需重载） */
function restartTicker() {
  tickSec = Math.max(1, Number(settings.refresh_sec) || 5);
  if (tickTimer) clearInterval(tickTimer);
  tickTimer = setInterval(() => {
    tickCount += 1;
    renderTicker();
  }, tickSec * 1000);
  renderTicker();
}

function renderTicker() {
  const el = document.getElementById('ticker');
  if (el) el.textContent = t('ticker', tickCount, tickSec);
}

/* ── 值区：按分组渲染全部控件当前值 ───────────────────────── */

function formatValue(def, value) {
  const type = def.type;
  if (type === 'password') return secretCache ? t('secretSet', secretCache.length) : t('notSet');
  if (type === 'boolean') return value ? t('on') : t('off');
  if (type === 'radio' || type === 'select') {
    const opt = (def.options || []).find((o) => o.value === value);
    return opt ? pickLang(opt.label, opt.label_en) : String(value ?? t('notSet'));
  }
  if (type === 'multiselect') {
    const labels = (Array.isArray(value) ? value : []).map((v) => {
      const opt = (def.options || []).find((o) => o.value === v);
      return opt ? pickLang(opt.label, opt.label_en) : String(v);
    });
    return labels.length ? labels.join(', ') : t('empty');
  }
  if (type === 'weekdays') {
    const days = (Array.isArray(value) ? value : []).map((d) => t('weekdays')[d] || d);
    return days.length ? days.join(', ') : t('empty');
  }
  if (type === 'timerange') {
    return value && value.start ? `${value.start} → ${value.end || '…'}` : t('notSet');
  }
  if (type === 'font') return value || t('defaultFont');
  if (value === '' || value === null || value === undefined) return t('notSet');
  return String(value);
}

/** 列表型控件（tasklist / todolist / datetasklist）渲染为 <ul> */
function buildListValue(def, value) {
  const ul = document.createElement('ul');
  ul.className = 'value-list';
  const items = Array.isArray(value) ? value : [];
  if (items.length === 0) {
    const li = document.createElement('li');
    li.className = 'muted';
    li.textContent = t('empty');
    ul.appendChild(li);
    return ul;
  }
  for (const item of items) {
    const li = document.createElement('li');
    if (def.type === 'todolist') {
      li.textContent = `${item.done ? '☑' : '☐'} ${item.text ?? ''}`;
      if (item.done) li.className = 'done';
    } else if (def.type === 'datetasklist') {
      li.textContent = item.time ? `${item.time}  ${item.text ?? ''}` : String(item.text ?? '');
    } else {
      li.textContent = String(item ?? '');
    }
    ul.appendChild(li);
  }
  return ul;
}

function renderPanel() {
  const panel = document.getElementById('panel');
  panel.textContent = ''; // 清空重建（DOM API，无 innerHTML 注入面）

  // 按声明顺序归组（与「皮肤设置」页同一归并规则）
  const groups = [];
  for (const def of schema?.settings || []) {
    const name = pickLang(def.group, def.group_en);
    let g = groups.find((g) => g.name === name);
    if (!g) { g = { name, defs: [] }; groups.push(g); }
    g.defs.push(def);
  }

  for (const g of groups) {
    const card = document.createElement('section');
    card.className = 'card';
    if (g.name) {
      const h = document.createElement('h2');
      h.textContent = g.name;
      card.appendChild(h);
    }
    for (const def of g.defs) {
      const row = document.createElement('div');
      row.className = 'row';

      const label = document.createElement('span');
      label.className = 'row-label';
      label.textContent = pickLang(def.label, def.label_en) || def.key;
      const desc = pickLang(def.description, def.description_en);
      if (desc) label.title = desc;
      row.appendChild(label);

      if (['tasklist', 'todolist', 'datetasklist'].includes(def.type)) {
        row.classList.add('row-block');
        row.appendChild(buildListValue(def, settings[def.key]));
      } else if (def.type === 'palette') {
        const wrap = document.createElement('span');
        wrap.className = 'row-value palette';
        const swatch = document.createElement('i');
        const hex = String(settings[def.key] ?? '');
        if (/^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(hex)) swatch.style.background = hex;
        wrap.appendChild(swatch);
        const code = document.createElement('span');
        code.textContent = hex || t('notSet');
        wrap.appendChild(code);
        row.appendChild(wrap);
      } else {
        const val = document.createElement('span');
        val.className = 'row-value';
        val.textContent = formatValue(def, settings[def.key]);
        row.appendChild(val);
      }
      card.appendChild(row);
    }
    panel.appendChild(card);
  }
}

function renderAll() {
  if (!schema) return;
  applySettings();
  renderPanel();
}

/* ── 事件 ─────────────────────────────────────────────────── */

// 管理器修改设置后实时推送（无需重载）
document.addEventListener('desk-setting-changed', (e) => {
  const { key, value } = e.detail || {};
  if (!key) return;
  settings[key] = value;                 // 桥也会同步，双保险
  if (key === 'api_secret') secretCache = value || '';
  renderAll();
  if (key === 'refresh_sec') restartTicker(); // 步进器改值即时重排小秒表
});

// 管理器切换语言后实时推送（本演示的核心：皮肤界面语言跟随管理器）
document.addEventListener('desk-language-changed', (e) => {
  setLang(e.detail?.language);
});

/* ── 启动 ─────────────────────────────────────────────────── */

(async function init() {
  settings = window.__DESK_PP__?.settings || {};
  lang = I18N[window.__DESK_PP__?.language] ? window.__DESK_PP__.language : 'zh-CN';
  document.documentElement.lang = lang;
  schema = await fetchSchema();
  await loadSecret();
  renderAll();
  restartTicker(); // 小秒表起走（stepper 演示）
})();
