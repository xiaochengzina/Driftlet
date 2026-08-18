'use strict';

/**
 * power-tools —— 高危能力演示：任意路径文件读写（file_system）+
 * 皮肤窗口配置控制（control）。
 *
 * 每张卡片对应一组后端命令（权限标在卡片标题右侧）：
 *   文件    read_any_file / write_any_file（file_system：任意绝对路径，
 *           失败 reject 系统错误原文）
 *   配置    skin_get_window_config / skin_set_window_config（control：
 *           任意皮肤含自己；patch 按键部分更新）
 *
 * 规范：动态内容一律 textContent / DOM API，不拼 innerHTML；
 * 命令 reject 的可读错误文案显示在对应卡片的结果区（接口演示的一部分）；
 * 桥不存在（纯浏览器打开）时所有结果区显示占位、按钮不抛错。
 */

/* ── 皮肤自身的界面文案（字典决定【皮肤页面】的语言） ── */
const I18N = {
  'zh-CN': {
    title: '高危能力演示',
    subtitle: '任意路径文件读写 · 皮肤窗口配置控制',
    bridgeOk: '桥已连接',
    bridgeMissing: '桥不可用（纯浏览器预览）',

    cardFs: '任意路径文件读写',
    fsWarn: '没有目录边界，整盘可读可写——只在确有需要时使用；相对路径会被直接拒绝。',
    fsPathPlaceholder: '绝对路径，如 D:\\temp\\demo.txt',
    fsContentPlaceholder: '要写入的文本内容（读取结果也显示在这里）',
    btnFsRead: '读取',
    btnFsWrite: '写入',
    btnFsReadBin: '按二进制读取（base64）',
    btnFsList: '列目录',
    btnFsPreview: '预览为图片（__fs__ 直引）',
    resPreviewOk: '图片经 __fs__ 端点直引（不经 JS 内存）',
    resPreviewFailed: '预览加载失败（file_system 未声明或不是存在的文件）',

    cardCtl: '皮肤窗口配置',
    ctlNote: '目标皮肤 id（默认本皮肤）；patch 只写要改的键。opacity / 位置 / 尺寸要求目标已加载，其余键未加载时仅持久化。',
    ctlTargetPlaceholder: '目标皮肤 id（留空 = 本皮肤）',
    btnCtlGet: '读取配置',
    ctlPatchLabel: '要修改的键（留空不改）',
    ctlOpacity: 'opacity 0.1–1',
    ctlZoom: 'zoom 0.5–2',
    ctlX: 'x',
    ctlY: 'y',
    ctlPlacementKeep: '层级：不改',
    ctlPlacementTop: '层级：置顶',
    ctlPlacementDesktop: '层级：正常',
    btnCtlApply: '应用 patch',

    resWaiting: '（点击上方按钮，结果显示在这里）',
    resBridgeMissing: '桥不可用：请在 Driftlet 管理器中加载本皮肤后再试',
    resEmpty: '（空）',
    resErrorPrefix: '错误：',
    resReadOk: (n) => `读取成功（${n} 字符）`,
    resWriteOk: (p, n) => `已写入 ${p}（${n} 字节）`,
    resBinOk: (n) => `二进制读取成功（base64，${n} 字符）`,
    resApplied: (keys) => `已应用：${keys}`,
    resNeedPath: '请先填写绝对路径',
    resNeedPatch: '至少填写一个要修改的键',
  },
  en: {
    title: 'Power Tools',
    subtitle: 'Arbitrary-path file access · cross-skin window-config control',
    bridgeOk: 'Bridge connected',
    bridgeMissing: 'Bridge unavailable (browser preview)',

    cardFs: 'Arbitrary-Path File Access',
    fsWarn: 'No directory boundary — the whole disk is reachable. Use only when needed; relative paths are rejected.',
    fsPathPlaceholder: 'Absolute path, e.g. D:\\temp\\demo.txt',
    fsContentPlaceholder: 'Text to write (read results also appear here)',
    btnFsRead: 'Read',
    btnFsWrite: 'Write',
    btnFsReadBin: 'Read binary (base64)',
    btnFsList: 'List Dir',
    btnFsPreview: 'Preview as image (__fs__)',
    resPreviewOk: 'Image referenced via the __fs__ endpoint (no JS memory hop)',
    resPreviewFailed: 'Preview load failed (file_system not declared, or not an existing file)',

    cardCtl: 'Skin Window Config',
    ctlNote: 'Target skin id (defaults to this skin); patch only includes keys you fill. opacity / position / size require the target to be loaded; the rest persist when unloaded.',
    ctlTargetPlaceholder: 'Target skin id (empty = this skin)',
    btnCtlGet: 'Read Config',
    ctlPatchLabel: 'Keys to change (empty = untouched)',
    ctlOpacity: 'opacity 0.1–1',
    ctlZoom: 'zoom 0.5–2',
    ctlX: 'x',
    ctlY: 'y',
    ctlPlacementKeep: 'Level: keep',
    ctlPlacementTop: 'Level: on top',
    ctlPlacementDesktop: 'Level: normal',
    btnCtlApply: 'Apply Patch',

    resWaiting: '(Click a button above; the result appears here)',
    resBridgeMissing: 'Bridge unavailable: load this skin in the Driftlet manager first',
    resEmpty: '(empty)',
    resErrorPrefix: 'Error: ',
    resReadOk: (n) => `Read OK (${n} chars)`,
    resWriteOk: (p, n) => `Wrote ${p} (${n} bytes)`,
    resBinOk: (n) => `Binary read OK (base64, ${n} chars)`,
    resApplied: (keys) => `Applied: ${keys}`,
    resNeedPath: 'Enter an absolute path first',
    resNeedPatch: 'Fill at least one key to change',
  },
};

const RESULT_IDS = ['fs-result', 'ctl-result'];

let lang = 'zh-CN';

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key];
  return typeof entry === 'function' ? entry(...args) : (entry ?? key);
}

/* ── 桥防御与结果区 ───────────────────────────────────────── */

function canInvoke() {
  return !!window.__DESK_PP__?.invoke;
}

/** 命令 reject 的值是可读错误文案（也可能包成 Error），统一转字符串 */
function errText(err) {
  return err instanceof Error ? (err.message || String(err)) : String(err);
}

function showResult(id, text) {
  const el = document.getElementById(id);
  el.dataset.touched = '1'; // 已有真实输出，语言切换时不再覆盖成占位文案
  el.classList.remove('error', 'placeholder');
  el.textContent = text;
}

function showError(id, err) {
  const el = document.getElementById(id);
  el.dataset.touched = '1';
  el.classList.remove('placeholder');
  el.classList.add('error');
  el.textContent = `${t('resErrorPrefix')}${errText(err)}`;
}

/** 桥缺失守卫：纯浏览器打开时按钮不抛错，在对应结果区提示 */
function needBridge(resultId) {
  if (canInvoke()) return true;
  showError(resultId, t('resBridgeMissing'));
  return false;
}

/** 未产生过输出的结果区填占位（桥可用=等待点击；不可用=桥缺失提示） */
function renderPlaceholders() {
  for (const id of RESULT_IDS) {
    const el = document.getElementById(id);
    if (el.dataset.touched) continue;
    el.classList.remove('error');
    el.classList.add('placeholder');
    el.textContent = canInvoke() ? t('resWaiting') : t('resBridgeMissing');
  }
}

/* ── 界面语言：跟随管理器（desk-language-changed 事件由桥派发） ── */

function applyI18n() {
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll('[data-i18n-placeholder]').forEach((el) => {
    el.placeholder = t(el.dataset.i18nPlaceholder);
  });
}

function renderBridgeBadge() {
  const el = document.getElementById('bridge-badge');
  const ok = canInvoke();
  el.classList.toggle('missing', !ok);
  el.textContent = ok ? t('bridgeOk') : t('bridgeMissing');
}

/* ── 任意路径文件读写（权限 file_system） ─────────────────── */

function fsPath() {
  return document.getElementById('fs-path').value.trim();
}

async function onFsRead(binary) {
  if (!needBridge('fs-result')) return;
  const path = fsPath();
  if (!path) { showError('fs-result', t('resNeedPath')); return; }
  try {
    const text = await window.__DESK_PP__.invoke('skin_read_any_file', { path, binary: !!binary });
    document.getElementById('fs-content').value = text;
    showResult('fs-result', binary ? t('resBinOk', text.length) : t('resReadOk', text.length));
  } catch (err) {
    showError('fs-result', err);
  }
}

async function onFsWrite() {
  if (!needBridge('fs-result')) return;
  const path = fsPath();
  if (!path) { showError('fs-result', t('resNeedPath')); return; }
  const data = document.getElementById('fs-content').value;
  try {
    await window.__DESK_PP__.invoke('skin_write_any_file', { path, data });
    showResult('fs-result', t('resWriteOk', path, new TextEncoder().encode(data).length));
  } catch (err) {
    showError('fs-result', err);
  }
}

async function onFsList() {
  if (!needBridge('fs-result')) return;
  const path = fsPath();
  if (!path) { showError('fs-result', t('resNeedPath')); return; }
  try {
    const entries = await window.__DESK_PP__.invoke('skin_list_any_dir', { path });
    const text = (Array.isArray(entries) && entries.length)
      ? entries.map((e) => (e.is_dir ? `[dir] ${e.name}/` : `      ${e.name}  ${e.size} B`)).join('\n')
      : t('resEmpty');
    showResult('fs-result', text);
  } catch (err) {
    showError('fs-result', err);
  }
}

/** __fs__ 端点直引：图片 URL 不经 JS 内存（与 base64 通道对比用） */
function onFsPreview() {
  if (!needBridge('fs-result')) return;
  const path = fsPath();
  if (!path) { showError('fs-result', t('resNeedPath')); return; }
  const img = document.getElementById('fs-preview-img');
  img.hidden = false;
  img.onerror = () => { img.hidden = true; showError('fs-result', t('resPreviewFailed')); };
  img.onload = () => showResult('fs-result', t('resPreviewOk'));
  img.src = 'http://skin.localhost/__fs__?path=' + encodeURIComponent(path);
}

/* ── 皮肤窗口配置（权限 control） ─────────────────────────── */

function ctlTarget() {
  return document.getElementById('ctl-target').value.trim();
}

async function onCtlGet() {
  if (!needBridge('ctl-result')) return;
  const skinId = ctlTarget();  // 留空 = 本皮肤（后端「省略即自己」约定）
  try {
    const cfg = await window.__DESK_PP__.invoke('skin_get_window_config', { skinId });
    showResult('ctl-result', JSON.stringify(cfg, null, 2));
  } catch (err) {
    showError('ctl-result', err);
  }
}

/** 只收集非空字段组成 patch；placement 单独按字符串处理 */
function collectPatch() {
  const patch = {};
  const opacity = document.getElementById('ctl-opacity').value;
  const zoom = document.getElementById('ctl-zoom').value;
  const x = document.getElementById('ctl-x').value;
  const y = document.getElementById('ctl-y').value;
  const placement = document.getElementById('ctl-placement').value;
  if (opacity !== '') patch.opacity = Number(opacity);
  if (zoom !== '') patch.zoom = Number(zoom);
  if (x !== '') patch.x = Math.trunc(Number(x));
  if (y !== '') patch.y = Math.trunc(Number(y));
  if (placement) patch.placement = placement;
  return patch;
}

async function onCtlApply() {
  if (!needBridge('ctl-result')) return;
  const skinId = ctlTarget();
  const patch = collectPatch();
  const keys = Object.keys(patch);
  if (keys.length === 0) { showError('ctl-result', t('resNeedPatch')); return; }
  try {
    await window.__DESK_PP__.invoke('skin_set_window_config', { skinId, patch });
    showResult('ctl-result', t('resApplied', keys.join(', ')));
  } catch (err) {
    showError('ctl-result', err);
  }
}

/* ── 启动 ─────────────────────────────────────────────────── */

function boot() {
  lang = window.__DESK_PP__?.language || 'zh-CN';
  applyI18n();
  renderBridgeBadge();
  renderPlaceholders();

  document.getElementById('btn-fs-read').onclick = () => onFsRead(false);
  document.getElementById('btn-fs-write').onclick = onFsWrite;
  document.getElementById('btn-fs-read-bin').onclick = () => onFsRead(true);
  document.getElementById('btn-fs-list').onclick = onFsList;
  document.getElementById('btn-fs-preview').onclick = onFsPreview;
  document.getElementById('btn-ctl-get').onclick = onCtlGet;
  document.getElementById('btn-ctl-apply').onclick = onCtlApply;

  // 管理器切换语言 → 桥烘焙值更新 + 事件；重绘界面文案
  document.addEventListener('desk-language-changed', (e) => {
    lang = e.detail?.language || lang;
    applyI18n();
    renderBridgeBadge();
    renderPlaceholders();
  });
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', boot);
} else {
  boot();
}
