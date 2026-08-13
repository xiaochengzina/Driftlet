/**
 * log.js — 日志窗口（label "log"，由设置页「高级」页签 open_log_window 创建；
 * 打开成功后设置页自动关闭）
 *
 * 数据约定（与后端 app_log.rs 对应）：
 *   - 后端环形缓冲常驻内存（上限 1000），本窗口不存在时后端不 emit；
 *   - 打开时「先 listen 再拉快照」：listen 期间到达的条目先入缓冲，
 *     快照返回后按 seq 去重合并排序，此后进入纯增量追加模式；
 *   - 关窗即销毁，前端内存随之释放。
 * 来源过滤是动态列表（全部/后端/每个皮肤各一项，随条目出现重建选项）。
 * 主题与语言不能走 API.getAppConfig（require_manager 只认 main 窗），
 * 由后端烘焙进 URL query：log.html?theme=auto&lang=zh-CN；运行期切换经
 * "app-log-language" / "app-log-theme" 事件跟随。
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { initI18n, t } from './i18n.js';
import { applyTheme } from './settings.js';

// 与后端 app_log::MAX_ENTRIES 保持一致
const MAX_ENTRIES = 1000;
// 滚动条距底小于该值时，新条目到达才跟随滚动（用户上翻时不拽动）
const STICKY_BOTTOM_PX = 30;

/** @type {{ seq: number, ts_ms: number, level: string, source: string, message: string }[]} */
let entries = [];
// listen 已注册、快照未返回期间到达的增量（boot 后转纯追加）
let pendingLive = null;
// source: 'all' | 'backend' | 'skin:<id>'（精确匹配单个皮肤）
const filters = { info: true, warn: true, error: true, source: 'all' };
// 已出现的皮肤来源集合：appendLive 据此发现新来源并重建过滤选项
let knownSources = new Set();

let listEl = null;
let emptyEl = null;

function fmtTime(tsMs) {
  const d = new Date(tsMs);
  const p = (n) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function sourceLabel(source) {
  if (source === 'backend') return t('log.sourceBackend');
  if (source.startsWith('skin:')) return `${t('log.sourceSkin')} ${source.slice(5)}`;
  return source;
}

function passesFilter(entry) {
  if (!filters[entry.level]) return false;
  if (filters.source === 'all') return true;
  return entry.source === filters.source;
}

/** 当前条目里出现过的皮肤来源（保持首次出现顺序） */
function collectSkinSources() {
  const out = [];
  for (const e of entries) {
    if (e.source !== 'backend' && !out.includes(e.source)) out.push(e.source);
  }
  return out;
}

/** 按当前条目重建来源过滤选项；filters.source 选中的皮肤无条目时选项保留
 * （清空日志不改变过滤选择，与级别开关的既有行为一致） */
function rebuildSourceOptions() {
  const select = document.getElementById('log-source');
  if (!select) return;
  const skins = collectSkinSources();
  if (filters.source.startsWith('skin:') && !skins.includes(filters.source)) {
    skins.push(filters.source);
  }
  select.textContent = '';
  const mk = (value, label) => {
    const o = document.createElement('option');
    o.value = value;
    o.textContent = label;
    select.appendChild(o);
  };
  mk('all', t('log.sourceAll'));
  mk('backend', t('log.sourceBackend'));
  for (const s of skins) mk(s, sourceLabel(s));
  select.value = filters.source;
}

function makeEntryNode(entry) {
  const row = document.createElement('div');
  row.className = `log-entry log-${entry.level}`;

  const time = document.createElement('span');
  time.className = 'log-time';
  time.textContent = fmtTime(entry.ts_ms);

  const level = document.createElement('span');
  level.className = 'log-level';
  level.textContent = t(`log.level${entry.level.charAt(0).toUpperCase()}${entry.level.slice(1)}`);

  const source = document.createElement('span');
  source.className = 'log-source';
  source.textContent = sourceLabel(entry.source);

  const msg = document.createElement('span');
  msg.className = 'log-msg';
  msg.textContent = entry.message;

  row.append(time, level, source, msg);
  return row;
}

function trimDom() {
  while (listEl.children.length > MAX_ENTRIES) {
    listEl.firstChild.remove();
  }
}

function nearBottom() {
  return listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < STICKY_BOTTOM_PX;
}

function appendLive(entry) {
  entries.push(entry);
  if (entries.length > MAX_ENTRIES) entries.splice(0, entries.length - MAX_ENTRIES);
  if (entry.source !== 'backend' && !knownSources.has(entry.source)) {
    knownSources.add(entry.source);
    rebuildSourceOptions();
  }
  if (!passesFilter(entry)) return;
  const stick = nearBottom();
  listEl.appendChild(makeEntryNode(entry));
  trimDom();
  listEl.style.display = '';
  emptyEl.style.display = 'none';
  if (stick) listEl.scrollTop = listEl.scrollHeight;
}

/** 过滤条件变化后全量重渲染（只对本地数组操作，不动后端数据） */
function renderAll() {
  listEl.textContent = '';
  const visible = entries.filter(passesFilter);
  const frag = document.createDocumentFragment();
  for (const e of visible) frag.appendChild(makeEntryNode(e));
  listEl.appendChild(frag);
  trimDom();
  const hasVisible = visible.length > 0;
  listEl.style.display = hasVisible ? '' : 'none';
  emptyEl.style.display = hasVisible ? 'none' : '';
  listEl.scrollTop = listEl.scrollHeight;
}

function renderShell() {
  document.getElementById('app').innerHTML = `
    <div class="titlebar">
      <div class="brand">
        <div class="brand-logo"><img src="/logo.png" alt="Driftlet" draggable="false" /></div>
        <span class="brand-name">Driftlet</span>
        <span class="brand-sub">${t('log.title')}</span>
      </div>
      <div class="win-btns">
        <button id="btn-minimize" class="win-btn"><svg width="10" height="1"><rect width="10" height="1" fill="currentColor"/></svg></button>
        <button id="btn-close" class="win-btn win-btn-close"><svg width="10" height="10"><line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1.2"/><line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1.2"/></svg></button>
      </div>
    </div>
    <div class="log-toolbar">
      <div class="log-filters">
        <button class="log-chip ${filters.info ? 'active' : ''}" data-level="info">${t('log.levelInfo')}</button>
        <button class="log-chip log-chip-warn ${filters.warn ? 'active' : ''}" data-level="warn">${t('log.levelWarn')}</button>
        <button class="log-chip log-chip-error ${filters.error ? 'active' : ''}" data-level="error">${t('log.levelError')}</button>
        <select id="log-source" class="log-source-select"></select>
      </div>
      <button id="log-clear" class="theme-btn">${t('log.clear')}</button>
    </div>
    <div class="log-list" id="log-list"></div>
    <div class="log-empty" id="log-empty">${t('log.empty')}</div>`;

  listEl = document.getElementById('log-list');
  emptyEl = document.getElementById('log-empty');

  const win = getCurrentWindow();
  document.getElementById('btn-minimize').onclick = (e) => { e.currentTarget.blur(); win.minimize(); };
  document.getElementById('btn-close').onclick = (e) => { e.currentTarget.blur(); win.close(); };

  document.querySelectorAll('.log-chip').forEach((chip) => {
    chip.onclick = () => {
      const level = chip.dataset.level;
      filters[level] = !filters[level];
      chip.classList.toggle('active', filters[level]);
      renderAll();
    };
  });
  document.getElementById('log-source').onchange = (e) => {
    filters.source = e.target.value;
    renderAll();
  };
  document.getElementById('log-clear').onclick = async () => {
    try {
      await invoke('clear_app_log');
      entries = [];
      knownSources.clear();
      rebuildSourceOptions();
      renderAll();
    } catch (err) {
      console.error('clear_app_log failed:', err);
    }
  };
}

async function boot() {
  const params = new URLSearchParams(location.search);
  applyTheme(params.get('theme') || 'auto');
  await initI18n(params.get('lang'));

  renderShell();

  // 与管理器同款全局守卫：右键屏蔽 + 悬停门控（hover-ok 驱动 .win-btn:hover）。
  // 只在 boot 注册一次——语言切换会重跑 renderShell，放里面会逐次叠加
  document.addEventListener('contextmenu', (e) => e.preventDefault());
  document.addEventListener('pointermove', () => document.body.classList.add('hover-ok'), { capture: true });
  getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused) document.body.classList.remove('hover-ok');
  });

  // 先 listen 缓冲增量，再拉快照合并 —— 顺序反过来会丢窗口期内的条目
  pendingLive = [];
  await listen('app-log-added', (event) => {
    if (pendingLive) {
      pendingLive.push(event.payload);
    } else {
      appendLive(event.payload);
    }
  });
  try {
    const snapshot = await invoke('get_app_log');
    const bySeq = new Map();
    for (const e of snapshot) bySeq.set(e.seq, e);
    for (const e of pendingLive) bySeq.set(e.seq, e);
    entries = [...bySeq.values()].sort((a, b) => a.seq - b.seq);
    if (entries.length > MAX_ENTRIES) entries = entries.slice(-MAX_ENTRIES);
  } catch (err) {
    console.error('get_app_log failed:', err);
    entries = pendingLive.slice(-MAX_ENTRIES);
  }
  pendingLive = null;
  knownSources = new Set(collectSkinSources());
  rebuildSourceOptions();
  renderAll();

  // 管理器运行期切换语言时本窗同步（语言烘焙于建窗时，经此事件跟随）
  listen('app-log-language', async (event) => {
    if (typeof event.payload !== 'string') return;
    await initI18n(event.payload);
    renderShell();
    rebuildSourceOptions();
    renderAll();
  });

  // 主题同理：烘焙只管建窗一刻，运行期经此事件跟随
  listen('app-log-theme', (event) => {
    if (typeof event.payload === 'string') applyTheme(event.payload);
  });
}

boot();
