'use strict';

/**
 * sys-monitor —— 免权限系统信息仪表板。
 *
 * 演示文档 §5.2 中除 3 个音量/媒体相关外的 12 个只读系统信息命令（均无需声明 permissions）：
 *   速率类（每秒轮询）：get_cpu_info / get_gpu_info / get_memory_info /
 *     get_disks_info / get_network_info / get_processes / get_battery_info /
 *     get_idle_time / get_foreground_window_info
 *   静态类（启动调一次）：get_os_info / get_monitors / get_disk_space
 *
 * 规范：
 * - 速率类读数 = 两次采样差值，首次调用为 0（基线），故需每秒轮询；
 * - 页面隐藏（document.visibilitychange）时暂停轮询，可见时恢复；
 * - 动态内容一律 textContent / DOM API 渲染，不拼 innerHTML（XSS 红线）；
 * - 所有 __DESK_PP__ 访问带可选链——纯浏览器打开 index.html 不抛错，
 *   各卡片显示「桥不可用」占位；
 * - 命令 reject 时把错误文案显示在对应卡片的错误区；
 * - 界面语言跟随管理器：初始读 __DESK_PP__.language，
 *   监听 desk-language-changed 重渲染。
 */

/* ── 皮肤自身的界面文案（zh / en 字典） ───────────────────── */
const I18N = {
  'zh-CN': {
    title: '系统监视',
    subtitle: '12 个免权限只读接口 · 速率类每秒轮询',
    badgeNoPerm: '免权限',
    bridgeOff: '桥不可用（纯浏览器预览）——系统信息仅在 Driftlet 内可见',
    card: {
      cpu: 'CPU', gpu: 'GPU', mem: '内存', disks: '磁盘', cspace: 'C: 卷空间',
      net: '网络', proc: '进程 Top 5', batt: '电池', act: '活动状态',
      mon: '显示器', os: '操作系统',
    },
    staticOnce: '静态信息：启动时调用一次',
    cpuMeta: (p, l, mhz) => `${p} 物理核 / ${l} 线程 · ${mhz} MHz`,
    vram: (used, total, pct) => `显存 ${used} / ${total}（${pct}）`,
    usedOf: (used, total) => `${used} / ${total}`,
    diskRate: (r, w) => `读 ${r} 写 ${w}`,
    mac: (m) => `MAC ${m}`,
    localIps: (ips) => `本机 IP：${ips}`,
    sortCpu: '按 CPU',
    sortMem: '按内存',
    procTotal: (n) => `共 ${n} 个进程`,
    procEmpty: '（暂无数据）',
    noBattery: '台式机无电池',
    battAc: '电源已接通',
    battOnBatt: '使用电池',
    battCharging: '充电中',
    battDischarging: '放电中',
    battRemain: (txt) => `剩余约 ${txt}`,
    idleLabel: '无操作',
    fgLabel: '前台窗口',
    idleSec: (s) => `${s} 秒无操作`,
    idleMin: (m) => `${m} 分钟无操作`,
    noFg: '（无前台窗口）',
    monSummary: (n) => `${n} 台显示器`,
    monPrimary: '主屏',
    monItem: (name, w, h, scale) => `${name} · ${w}×${h} @ ${scale}%`,
    osHost: '主机名',
    osUser: '用户',
    osUptime: '开机时长',
    uptimeDH: (d, h) => `${d} 天 ${h} 小时`,
    uptimeHM: (h, m) => `${h} 小时 ${m} 分`,
    uptimeM: (m) => `${m} 分钟`,
    errPrefix: '错误',
  },
  en: {
    title: 'System Monitor',
    subtitle: '12 permission-free read-only APIs · rates polled every second',
    badgeNoPerm: 'No permissions',
    bridgeOff: 'Bridge unavailable (plain browser preview) — system info is only visible inside Driftlet',
    card: {
      cpu: 'CPU', gpu: 'GPU', mem: 'Memory', disks: 'Disks', cspace: 'Volume C:',
      net: 'Network', proc: 'Top 5 Processes', batt: 'Battery', act: 'Activity',
      mon: 'Monitors', os: 'Operating System',
    },
    staticOnce: 'Static info: called once at startup',
    cpuMeta: (p, l, mhz) => `${p} physical / ${l} logical cores · ${mhz} MHz`,
    vram: (used, total, pct) => `VRAM ${used} / ${total} (${pct})`,
    usedOf: (used, total) => `${used} / ${total}`,
    diskRate: (r, w) => `R ${r}  W ${w}`,
    mac: (m) => `MAC ${m}`,
    localIps: (ips) => `Local IPs: ${ips}`,
    sortCpu: 'By CPU',
    sortMem: 'By memory',
    procTotal: (n) => `${n} processes`,
    procEmpty: '(no data)',
    noBattery: 'Desktop PC, no battery',
    battAc: 'AC connected',
    battOnBatt: 'On battery',
    battCharging: 'charging',
    battDischarging: 'discharging',
    battRemain: (txt) => `~${txt} remaining`,
    idleLabel: 'Idle',
    fgLabel: 'Foreground',
    idleSec: (s) => `idle for ${s}s`,
    idleMin: (m) => `idle for ${m} min`,
    noFg: '(no foreground window)',
    monSummary: (n) => `${n} monitor(s)`,
    monPrimary: 'Primary',
    monItem: (name, w, h, scale) => `${name} · ${w}×${h} @ ${scale}%`,
    osHost: 'Host',
    osUser: 'User',
    osUptime: 'Uptime',
    uptimeDH: (d, h) => `${d}d ${h}h`,
    uptimeHM: (h, m) => `${h}h ${m}m`,
    uptimeM: (m) => `${m}m`,
    errPrefix: 'Error',
  },
};

let lang = 'zh-CN';
let procSort = 'cpu';          // get_processes 排序方式（cpu / memory）
const state = {};              // 各命令最近一次成功返回的数据（语言切换时重渲染用）

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key];
  return typeof entry === 'function' ? entry(...args) : (entry ?? key);
}

/* ── 桥调用与错误展示 ─────────────────────────────────────── */

/** 统一调用入口：无桥时 reject 出可读文案，由各卡片错误区展示 */
async function call(cmd, args) {
  if (!window.__DESK_PP__?.invoke) throw new Error(t('bridgeOff'));
  return window.__DESK_PP__.invoke(cmd, args);
}

// 每张卡片一个错误区；同卡多命令时按「命令名：文案」逐条列出
const cardErrors = {};

function renderCardError(cardId) {
  const el = document.getElementById(`err-${cardId}`);
  if (!el) return;
  const errs = cardErrors[cardId] || {};
  const msgs = Object.entries(errs).map(([cmd, m]) => `${cmd}: ${m}`);
  el.hidden = msgs.length === 0;
  el.textContent = msgs.join('；');
}

function reportErr(cardId, cmd, err) {
  (cardErrors[cardId] ||= {})[cmd] = String(err ?? t('errPrefix'));
  renderCardError(cardId);
}

function clearErr(cardId, cmd) {
  if (cardErrors[cardId]) delete cardErrors[cardId][cmd];
  renderCardError(cardId);
}

/* ── 格式化工具：字节 B/KB/MB/GB，速率 /s ─────────────────── */

function fmtBytes(n) {
  if (n === null || n === undefined || isNaN(Number(n))) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let v = Math.max(0, Number(n));
  let i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toFixed(digits)} ${units[i]}`;
}

function fmtRate(n) {
  return `${fmtBytes(n)}/s`;
}

function fmtPct(n) {
  return (n === null || n === undefined || isNaN(Number(n))) ? '—' : `${Number(n).toFixed(1)}%`;
}

/** 秒数 → 「d 天 h 小时 / h 小时 m 分 / m 分钟」 */
function fmtUptime(secs) {
  if (secs === null || secs === undefined || isNaN(Number(secs))) return '—';
  const s = Math.max(0, Math.floor(Number(secs)));
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return t('uptimeDH', d, h);
  if (h > 0) return t('uptimeHM', h, m);
  return t('uptimeM', m);
}

function setText(id, text) {
  const el = document.getElementById(id);
  if (el) el.textContent = text;
}

function setBar(id, pct) {
  const el = document.getElementById(id);
  if (el) el.style.width = `${Math.max(0, Math.min(100, Number(pct) || 0))}%`;
}

/* ── 各卡片刷新：取数 → 缓存 → 渲染 ───────────────────────── */

// get_cpu_info（数组恒 1 项）
async function refreshCpu() {
  try {
    const arr = await call('get_cpu_info');
    state.cpu = Array.isArray(arr) && arr.length ? arr[0] : null;
    clearErr('cpu', 'get_cpu_info');
  } catch (e) {
    reportErr('cpu', 'get_cpu_info', e);
  }
  renderCpu();
}

function renderCpu() {
  const c = state.cpu;
  setText('cpu-usage', c ? fmtPct(c.usage) : '—');
  setBar('cpu-bar', c ? c.usage : 0);
  setText('cpu-name', c?.name || '—');
  setText('cpu-meta', c ? t('cpuMeta', c.physical_cores ?? '—', c.logical_cores ?? '—', c.frequency_mhz ?? '—') : '—');

  // 每线程使用率：小竖条网格，数量动态（DOM API 重建）
  const grid = document.getElementById('cpu-cores');
  grid.textContent = '';
  const cores = Array.isArray(c?.usage_per_core) ? c.usage_per_core : [];
  for (let i = 0; i < cores.length; i++) {
    const core = document.createElement('i');
    const pct = Math.max(0, Math.min(100, Number(cores[i]) || 0));
    const fill = document.createElement('b');
    fill.style.height = `${pct}%`;
    core.appendChild(fill);
    core.title = `#${i + 1} ${pct.toFixed(1)}%`;
    grid.appendChild(core);
  }
}

// get_gpu_info（多 GPU 返回多项）
async function refreshGpu() {
  try {
    state.gpus = await call('get_gpu_info');
    clearErr('gpu', 'get_gpu_info');
  } catch (e) {
    reportErr('gpu', 'get_gpu_info', e);
  }
  renderGpu();
}

function renderGpu() {
  const list = document.getElementById('gpu-list');
  list.textContent = '';
  const gpus = Array.isArray(state.gpus) ? state.gpus : [];
  if (!gpus.length) {
    const p = document.createElement('p');
    p.className = 'line muted';
    p.textContent = '—';
    list.appendChild(p);
    return;
  }
  for (const g of gpus) {
    const item = document.createElement('div');
    item.className = 'gpu-item';

    const row = document.createElement('div');
    row.className = 'meter-row';
    const name = document.createElement('span');
    name.className = 'line gpu-name';
    name.textContent = g.name || '—';
    const pct = document.createElement('span');
    pct.className = 'meter-pct';
    pct.textContent = fmtPct(g.usage);
    row.appendChild(name);
    row.appendChild(pct);

    const meterRow = document.createElement('div');
    meterRow.className = 'meter-row';
    const meter = document.createElement('span');
    meter.className = 'meter';
    const bar = document.createElement('i');
    bar.style.width = `${Math.max(0, Math.min(100, Number(g.usage) || 0))}%`;
    meter.appendChild(bar);
    meterRow.appendChild(meter);

    const vram = document.createElement('p');
    vram.className = 'line muted';
    vram.textContent = t('vram', fmtBytes(g.vram_used), fmtBytes(g.vram_total), fmtPct(g.vram_usage_pct));

    item.appendChild(row);
    item.appendChild(meterRow);
    item.appendChild(vram);
    list.appendChild(item);
  }
}

// get_memory_info
async function refreshMemory() {
  try {
    state.mem = await call('get_memory_info');
    clearErr('mem', 'get_memory_info');
  } catch (e) {
    reportErr('mem', 'get_memory_info', e);
  }
  renderMemory();
}

function renderMemory() {
  const { ram, swap, commit } = state.mem || {};
  setBar('ram-bar', ram?.usage_pct);
  setText('ram-pct', fmtPct(ram?.usage_pct));
  setText('ram-text', ram ? t('usedOf', fmtBytes(ram.used), fmtBytes(ram.total)) : '—');
  setBar('swap-bar', swap?.usage_pct);
  setText('swap-pct', fmtPct(swap?.usage_pct));
  setText('swap-text', swap ? t('usedOf', fmtBytes(swap.used), fmtBytes(swap.total)) : '—');
  setBar('commit-bar', commit?.usage_pct);
  setText('commit-pct', fmtPct(commit?.usage_pct));
  setText('commit-text', commit ? t('usedOf', fmtBytes(commit.used), fmtBytes(commit.total)) : '—');
}

// get_disks_info
async function refreshDisks() {
  try {
    state.disks = await call('get_disks_info');
    clearErr('disks', 'get_disks_info');
  } catch (e) {
    reportErr('disks', 'get_disks_info', e);
  }
  renderDisks();
}

function renderDisks() {
  const list = document.getElementById('disk-list');
  list.textContent = '';
  const disks = Array.isArray(state.disks) ? state.disks : [];
  if (!disks.length) {
    const p = document.createElement('p');
    p.className = 'line muted';
    p.textContent = '—';
    list.appendChild(p);
    return;
  }
  for (const d of disks) {
    const item = document.createElement('div');
    item.className = 'disk-item';

    const head = document.createElement('div');
    head.className = 'meter-row';
    const name = document.createElement('span');
    name.className = 'line';
    // 名称 + 挂载点 + 文件系统
    name.textContent = `${d.name || d.mount_point || '—'} (${d.mount_point || '?'}) · ${d.fs || '—'}`;
    const pct = document.createElement('span');
    pct.className = 'meter-pct';
    pct.textContent = fmtPct(d.usage_pct);
    head.appendChild(name);
    head.appendChild(pct);

    const meterRow = document.createElement('div');
    meterRow.className = 'meter-row';
    const meter = document.createElement('span');
    meter.className = 'meter';
    const bar = document.createElement('i');
    bar.style.width = `${Math.max(0, Math.min(100, Number(d.usage_pct) || 0))}%`;
    meter.appendChild(bar);
    meterRow.appendChild(meter);

    const detail = document.createElement('p');
    detail.className = 'line muted';
    detail.textContent = `${t('usedOf', fmtBytes(d.used), fmtBytes(d.total))} · ${t('diskRate', fmtRate(d.read_bps), fmtRate(d.write_bps))}`;

    item.appendChild(head);
    item.appendChild(meterRow);
    item.appendChild(detail);
    list.appendChild(item);
  }
}

// get_disk_space（静态：启动调一次）
async function refreshDiskSpace() {
  try {
    state.cspace = await call('get_disk_space', { path: 'C:' });
    clearErr('cspace', 'get_disk_space');
  } catch (e) {
    reportErr('cspace', 'get_disk_space', e);
  }
  renderDiskSpace();
}

function renderDiskSpace() {
  const c = state.cspace;
  setBar('cspace-bar', c?.usage_pct);
  setText('cspace-pct', fmtPct(c?.usage_pct));
  setText('cspace-text', c
    ? `${t('usedOf', fmtBytes(c.used), fmtBytes(c.total))} · ${fmtBytes(c.free)} ${lang === 'en' ? 'free' : '可用'}`
    : '—');
}

// get_network_info
async function refreshNetwork() {
  try {
    state.net = await call('get_network_info');
    clearErr('net', 'get_network_info');
  } catch (e) {
    reportErr('net', 'get_network_info', e);
  }
  renderNetwork();
}

function renderNetwork() {
  const list = document.getElementById('net-list');
  list.textContent = '';
  const net = state.net;
  const adapters = Array.isArray(net?.adapters) ? net.adapters : [];
  if (!adapters.length) {
    const p = document.createElement('p');
    p.className = 'line muted';
    p.textContent = '—';
    list.appendChild(p);
  }
  for (const a of adapters) {
    const item = document.createElement('div');
    item.className = 'net-item';

    const head = document.createElement('p');
    head.className = 'line';
    head.textContent = a.name || '—';

    const ips = document.createElement('p');
    ips.className = 'line muted';
    ips.textContent = `${(Array.isArray(a.ips) ? a.ips : []).join(', ') || '—'} · ${t('mac', a.mac || '—')}`;

    const rate = document.createElement('p');
    rate.className = 'line muted';
    rate.textContent = `↑ ${fmtRate(a.upload_bps)}  ↓ ${fmtRate(a.download_bps)}`;

    item.appendChild(head);
    item.appendChild(ips);
    item.appendChild(rate);
    list.appendChild(item);
  }
  const localIps = Array.isArray(net?.local_ips) ? net.local_ips : [];
  setText('net-ips', localIps.length ? t('localIps', localIps.join(', ')) : '—');
}

// get_processes（cpu / memory 排序切换）
async function refreshProcesses() {
  try {
    state.procs = await call('get_processes', { sort: procSort, limit: 5 });
    clearErr('proc', 'get_processes');
  } catch (e) {
    reportErr('proc', 'get_processes', e);
  }
  renderProcesses();
}

function renderProcesses() {
  const data = state.procs;
  setText('proc-total', data ? t('procTotal', data.total ?? '—') : '');
  const list = document.getElementById('proc-list');
  list.textContent = '';
  const procs = Array.isArray(data?.processes) ? data.processes : [];
  if (!procs.length) {
    const p = document.createElement('p');
    p.className = 'line muted';
    p.textContent = t('procEmpty');
    list.appendChild(p);
    return;
  }
  for (const p of procs) {
    const row = document.createElement('div');
    row.className = 'proc-row';

    const name = document.createElement('span');
    name.className = 'proc-name';
    name.textContent = `${p.name || '—'} (${p.pid})`;
    name.title = name.textContent;

    const val = document.createElement('span');
    val.className = 'proc-val';
    val.textContent = procSort === 'memory'
      ? fmtBytes(p.memory_bytes)
      : fmtPct(p.cpu);

    row.appendChild(name);
    row.appendChild(val);
    list.appendChild(row);
  }
}

// get_battery_info（has_battery=false → 台式机无电池）
async function refreshBattery() {
  try {
    state.batt = await call('get_battery_info');
    clearErr('batt', 'get_battery_info');
  } catch (e) {
    reportErr('batt', 'get_battery_info', e);
  }
  renderBattery();
}

function renderBattery() {
  const b = state.batt;
  if (!b || b.has_battery === false) {
    setBar('batt-bar', 0);
    setText('batt-pct', '—');
    setText('batt-text', b ? t('noBattery') : '—');
    return;
  }
  setBar('batt-bar', b.percent);
  setText('batt-pct', b.percent === null || b.percent === undefined ? '—' : `${b.percent}%`);
  const parts = [b.ac_online ? t('battAc') : t('battOnBatt')];
  parts.push(b.charging ? t('battCharging') : t('battDischarging'));
  if (b.secs_remaining !== null && b.secs_remaining !== undefined) {
    parts.push(t('battRemain', fmtUptime(b.secs_remaining)));
  }
  setText('batt-text', parts.join(' · '));
}

// get_idle_time + get_foreground_window_info
async function refreshActivity() {
  try {
    state.idle = await call('get_idle_time');
    clearErr('act', 'get_idle_time');
  } catch (e) {
    reportErr('act', 'get_idle_time', e);
  }
  try {
    state.fg = await call('get_foreground_window_info');
    clearErr('act', 'get_foreground_window_info');
  } catch (e) {
    reportErr('act', 'get_foreground_window_info', e);
  }
  renderActivity();
}

function renderActivity() {
  const idleMs = state.idle;
  if (idleMs === null || idleMs === undefined || isNaN(Number(idleMs))) {
    setText('idle-text', '—');
  } else {
    const s = Math.floor(Number(idleMs) / 1000);
    setText('idle-text', s < 90 ? t('idleSec', s) : t('idleMin', Math.floor(s / 60)));
  }
  const w = state.fg;
  setText('fg-text', w ? `${w.process_name || '—'} (${w.pid ?? '—'}) — ${w.title || ''}` : t('noFg'));
}

// get_monitors（静态：启动调一次）
async function refreshMonitors() {
  try {
    state.mons = await call('get_monitors');
    clearErr('mon', 'get_monitors');
  } catch (e) {
    reportErr('mon', 'get_monitors', e);
  }
  renderMonitors();
}

function renderMonitors() {
  const list = document.getElementById('mon-list');
  list.textContent = '';
  const mons = Array.isArray(state.mons) ? state.mons : [];
  setText('mon-summary', mons.length ? t('monSummary', mons.length) : '—');
  for (const m of mons) {
    const p = document.createElement('p');
    p.className = 'line muted';
    // rect 是物理像素；scale_factor 1.25 = 125%
    p.textContent = t('monItem',
      m.name || '—',
      m.rect?.width ?? '?', m.rect?.height ?? '?',
      Math.round((Number(m.scale_factor) || 1) * 100));
    if (m.is_primary) {
      const tag = document.createElement('span');
      tag.className = 'tag';
      tag.textContent = t('monPrimary');
      p.appendChild(document.createTextNode(' '));
      p.appendChild(tag);
    }
    list.appendChild(p);
  }
}

// get_os_info（静态：启动调一次）
async function refreshOs() {
  try {
    state.os = await call('get_os_info');
    clearErr('os', 'get_os_info');
  } catch (e) {
    reportErr('os', 'get_os_info', e);
  }
  renderOs();
}

function renderOs() {
  const o = state.os;
  setText('os-name', o ? `${o.os_name || '—'} (${o.os_version || '?'}, build ${o.build ?? '?'})` : '—');
  setText('os-host', o?.host_name || '—');
  setText('os-user', o?.user_name || '—');
  setText('os-uptime', o ? fmtUptime(o.uptime_secs) : '—');
}

/* ── 静态文案渲染（语言切换时重跑） ───────────────────────── */

function renderStaticTexts() {
  setText('skin-title', t('title'));
  setText('subtitle', t('subtitle'));
  setText('perm-badge', t('badgeNoPerm'));
  setText('bridge-warn', t('bridgeOff'));
  document.querySelectorAll('h2[data-card]').forEach((h2) => {
    h2.textContent = t('card')[h2.dataset.card] || h2.dataset.card;
  });
  setText('cspace-hint', t('staticOnce'));
  setText('mon-hint', t('staticOnce'));
  setText('os-hint', t('staticOnce'));
  setText('sort-cpu', t('sortCpu'));
  setText('sort-mem', t('sortMem'));
  setText('idle-label', t('idleLabel'));
  setText('fg-label', t('fgLabel'));
  setText('os-host-label', t('osHost'));
  setText('os-user-label', t('osUser'));
  setText('os-uptime-label', t('osUptime'));
}

/** 语言切换：静态文案 + 各卡片按缓存数据重渲染 */
function renderAll() {
  renderStaticTexts();
  renderCpu();
  renderGpu();
  renderMemory();
  renderDisks();
  renderDiskSpace();
  renderNetwork();
  renderProcesses();
  renderBattery();
  renderActivity();
  renderMonitors();
  renderOs();
}

function setLang(next) {
  lang = I18N[next] ? next : 'zh-CN';
  document.documentElement.lang = lang;
  renderAll();
}

/* ── 轮询：速率类 1 秒一次；页面隐藏时暂停，可见时恢复 ────── */

const fastTasks = [
  refreshCpu, refreshGpu, refreshMemory, refreshDisks, refreshNetwork,
  refreshProcesses, refreshBattery, refreshActivity,
];

let timer = null;

function tick() {
  // 各任务独立 try/catch，单个失败不影响其他卡片
  for (const f of fastTasks) f();
}

function startPolling() {
  if (timer) return;
  tick();
  timer = setInterval(tick, 1000);
}

function stopPolling() {
  clearInterval(timer);
  timer = null;
}

document.addEventListener('visibilitychange', () => {
  if (document.hidden) stopPolling();
  else startPolling();
});

/* ── 事件 ─────────────────────────────────────────────────── */

// 管理器切换语言后实时推送
document.addEventListener('desk-language-changed', (e) => {
  setLang(e.detail?.language);
});

/* ── 启动 ─────────────────────────────────────────────────── */

(function init() {
  lang = I18N[window.__DESK_PP__?.language] ? window.__DESK_PP__.language : 'zh-CN';
  document.documentElement.lang = lang;

  // 排序切换按钮
  const btnCpu = document.getElementById('sort-cpu');
  const btnMem = document.getElementById('sort-mem');
  btnCpu.addEventListener('click', () => {
    procSort = 'cpu';
    btnCpu.classList.add('on');
    btnMem.classList.remove('on');
    refreshProcesses();
  });
  btnMem.addEventListener('click', () => {
    procSort = 'memory';
    btnMem.classList.add('on');
    btnCpu.classList.remove('on');
    refreshProcesses();
  });

  renderAll();

  if (!window.__DESK_PP__?.invoke) {
    // 纯浏览器打开：显示占位提示，各卡片错误区标注「桥不可用」，不启动轮询
    document.getElementById('bridge-warn').hidden = false;
    for (const cardId of ['cpu', 'gpu', 'mem', 'disks', 'cspace', 'net', 'proc', 'batt', 'act', 'mon', 'os']) {
      reportErr(cardId, '—', t('bridgeOff'));
    }
    return;
  }

  // 静态类：启动调一次；速率类：每秒轮询
  refreshOs();
  refreshMonitors();
  refreshDiskSpace();
  startPolling();
})();
