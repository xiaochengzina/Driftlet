'use strict';

/**
 * toolbox —— 本机接口演示：剪贴板 / 文件 / 注册表 / 命令 / 外链 / 设置读写。
 *
 * 每张卡片对应一组后端命令（权限标在卡片标题右侧）：
 *   剪贴板  read_clipboard_text / write_clipboard_text（clipboard）
 *   文件    skin_write_file / skin_read_file / skin_list_dir / skin_delete_file（免权限，仅限皮肤目录）
 *   注册表  read_registry_value（registry，只读）
 *   命令    run_command（shell：普通权限、隐藏窗口、超时杀进程）
 *   外链    open_external（system；.exe 等可执行目标会被拒绝——有专门的演示按钮）
 *   设置    skin_get_setting / skin_set_setting（免权限）
 *
 * 契约要点（皮肤开发指南 §4.4）：
 *   skin_set_setting 写成功后不会向自己回派 desk-setting-changed —— 本地状态自己维护；
 *   管理器侧的修改会推 desk-setting-changed —— 必须监听并同步 UI。
 *
 * 规范：动态内容一律 textContent / DOM API，不拼 innerHTML；
 * 命令 reject 的可读错误文案显示在对应卡片的结果区（接口演示的一部分）；
 * 桥不存在（纯浏览器打开）时所有结果区显示占位、按钮不抛错。
 */

/* ── 皮肤自身的界面文案（字典决定【皮肤页面】的语言；
      skin.json 里的 label_en 等决定【管理器配置面板】的语言，两者是两回事） ── */
const I18N = {
  'zh-CN': {
    title: '本机工具箱',
    subtitle: '剪贴板 · 文件 · 注册表 · 命令 · 外链 · 设置接口演示',
    bridgeOk: '桥已连接',
    bridgeMissing: '桥不可用（纯浏览器预览）',
    permFree: '免权限',

    cardClipboard: '剪贴板',
    clipWarn: '读取的是你当前的剪贴板——可能含有刚复制的敏感内容，只在确有需要时调用。',
    btnReadClip: '读取剪贴板',
    clipPlaceholder: '要写入剪贴板的文本…',
    btnWriteClip: '写入剪贴板',

    cardFiles: '文件（仅限皮肤目录）',
    filesNote: '写入内容取自下方「设置读写」卡的备注文本框；子目录 data 会自动创建。',
    btnWriteNote: '写备注 → data/note.txt',
    btnReadNote: '读回 data/note.txt',
    btnListData: '列目录 data',
    btnListRoot: '列根目录（省略 path）',
    btnDeleteNote: '删除 data/note.txt',

    cardRegistry: '注册表（只读）',
    regPresetTemp: '预设：HKCU\\Environment\\TEMP',
    regPathPlaceholder: '路径，如 Environment',
    regNamePlaceholder: '值名，如 TEMP',
    btnReadReg: '读取',
    regMultiNote: 'multi_string（数组）：',
    regBinaryNote: 'binary（base64）：',

    cardShell: '执行命令',
    shellNote: '普通权限（不提升）、隐藏窗口执行；超时杀进程（默认 30 秒，上限 120 秒）；stdout/stderr 各截断 1MB。',
    cmdPlaceholder: '命令，如 cmd',
    cmdArgsPlaceholder: '参数（空格分隔），如 /c ver',
    btnRun: '运行',

    cardExternal: '打开链接 / 文件',
    extPlaceholder: 'https://… 、mailto:… 或本机绝对路径',
    btnOpen: '打开',
    btnOpenDenied: '演示被拒绝（.exe 路径）',

    cardSettings: '设置读写',
    tokenNote: 'api_token 是 password 类型：值不会注入页面，__DESK_PP__.settings.api_token 恒为空串（可在上方输入框自行验证）；唯一读取通道是 skin_get_setting。',
    btnReadToken: '经命令读取 api_token',
    noteLabel: '备注 note（longtext）',
    btnSaveNote: '保存（skin_set_setting）',
    btnReadNoteCmd: '经命令读取 note',
    tasksLabel: '待办 tasks（todolist）——勾选/增删即写回；管理器侧修改实时同步进来',
    taskAddPlaceholder: '新待办…',
    btnAddTask: '添加',

    resWaiting: '（点击上方按钮，结果显示在这里）',
    resBridgeMissing: '桥不可用：请在 Driftlet 管理器中加载本皮肤后再试',
    resEmpty: '（空）',
    resErrorPrefix: '错误：',
    resWrote: (n) => `已写入剪贴板（${n} 个字符）`,
    resWroteFile: (p, n) => `已写入 ${p}（${n} 个字符）`,
    resDone: '完成',
    resDirEmpty: '（空目录）',
    resExitCode: (c) => `退出码：${c}`,
    resOpened: (target) => `已请求系统打开：${target}`,
    resNeedCmd: '请填写命令',
    resTokenSet: (n) => `已设置（${n} 个字符）。演示不展示原文——真实皮肤拿它去调自己的服务即可。`,
    resTokenEmpty: '未设置（空串）——请到管理器「皮肤设置」页填写后重试',
    resTokenChanged: '管理器侧已修改 api_token，点击「经命令读取 api_token」查看新状态',
    resSaved: (key) => `已保存 ${key}（写成功不会向自己回派事件，本地即最新）`,
    resSavedTruncated: (key) => `已保存 ${key}，超出后端上限的部分已截断（≤500 条、≤200 字符/条）`,
    resTasksEmpty: '（暂无待办）',
  },
  en: {
    title: 'Toolbox',
    subtitle: 'Clipboard · files · registry · shell · links & settings API demo',
    bridgeOk: 'Bridge connected',
    bridgeMissing: 'Bridge unavailable (browser preview)',
    permFree: 'no permission',

    cardClipboard: 'Clipboard',
    clipWarn: 'Reads your current clipboard — it may contain sensitive content you just copied; call only when you really need it.',
    btnReadClip: 'Read clipboard',
    clipPlaceholder: 'Text to write to the clipboard…',
    btnWriteClip: 'Write clipboard',

    cardFiles: 'Files (skin folder only)',
    filesNote: 'The write source is the note textarea in the Settings card below; the data subfolder is created automatically.',
    btnWriteNote: 'Write note → data/note.txt',
    btnReadNote: 'Read data/note.txt',
    btnListData: "List 'data'",
    btnListRoot: 'List root (path omitted)',
    btnDeleteNote: 'Delete data/note.txt',

    cardRegistry: 'Registry (read-only)',
    regPresetTemp: 'Preset: HKCU\\Environment\\TEMP',
    regPathPlaceholder: 'Path, e.g. Environment',
    regNamePlaceholder: 'Value name, e.g. TEMP',
    btnReadReg: 'Read',
    regMultiNote: 'multi_string (array):',
    regBinaryNote: 'binary (base64):',

    cardShell: 'Run Command',
    shellNote: 'Runs with normal privileges (no elevation) and a hidden window; killed on timeout (default 30s, max 120s); stdout/stderr truncated at 1MB each.',
    cmdPlaceholder: 'Command, e.g. cmd',
    cmdArgsPlaceholder: 'Args (space-separated), e.g. /c ver',
    btnRun: 'Run',

    cardExternal: 'Open Link / File',
    extPlaceholder: 'https://…, mailto:… or an absolute local path',
    btnOpen: 'Open',
    btnOpenDenied: 'Demo rejection (.exe path)',

    cardSettings: 'Settings Read / Write',
    tokenNote: 'api_token is a password setting: it is never baked into the page — __DESK_PP__.settings.api_token is always an empty string; the only read channel is skin_get_setting.',
    btnReadToken: 'Read api_token via command',
    noteLabel: 'Note (longtext)',
    btnSaveNote: 'Save (skin_set_setting)',
    btnReadNoteCmd: 'Read note via command',
    tasksLabel: 'Tasks (todolist) — toggles/edits write back instantly; manager-side edits sync in live',
    taskAddPlaceholder: 'New task…',
    btnAddTask: 'Add',

    resWaiting: '(Click a button above; results appear here)',
    resBridgeMissing: 'Bridge unavailable: load this skin in the Driftlet manager first',
    resEmpty: '(empty)',
    resErrorPrefix: 'Error: ',
    resWrote: (n) => `Clipboard written (${n} chars)`,
    resWroteFile: (p, n) => `Wrote ${p} (${n} chars)`,
    resDone: 'Done',
    resDirEmpty: '(empty directory)',
    resExitCode: (c) => `Exit code: ${c}`,
    resOpened: (target) => `Asked the system to open: ${target}`,
    resNeedCmd: 'Enter a command',
    resTokenSet: (n) => `Set (${n} chars). The demo never shows the raw value — a real skin would use it for its own service calls.`,
    resTokenEmpty: 'Not set (empty) — fill it in on the manager settings page and retry',
    resTokenChanged: 'api_token was changed manager-side; click "Read api_token via command" to re-check',
    resSaved: (key) => `Saved ${key} (no event is dispatched back on success — local state is already current)`,
    resSavedTruncated: (key) => `Saved ${key}; content beyond the backend limits was truncated (≤500 items, ≤200 chars each)`,
    resTasksEmpty: '(no tasks)',
  },
};

const NOTE_PATH = 'data/note.txt'; // 文件卡演示用的固定相对路径
const RESULT_IDS = ['clip-result', 'file-result', 'reg-result', 'cmd-result', 'ext-result', 'settings-result'];

let lang = 'zh-CN';  // 当前皮肤界面语言
let settings = {};   // __DESK_PP__.settings（password 键恒为空串，见指南 §4.3）
let tasks = [];      // 待办本地副本（skin_set_setting 写回的源）

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

/* ── 静态文案与桥状态徽标 ─────────────────────────────────── */

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

/* ── 剪贴板（权限 clipboard） ─────────────────────────────── */

async function onReadClipboard() {
  if (!needBridge('clip-result')) return;
  try {
    const text = await window.__DESK_PP__.invoke('read_clipboard_text');
    showResult('clip-result', text || t('resEmpty'));
  } catch (err) {
    showError('clip-result', err);
  }
}

async function onWriteClipboard() {
  if (!needBridge('clip-result')) return;
  const text = document.getElementById('clip-input').value;
  try {
    await window.__DESK_PP__.invoke('write_clipboard_text', { text });
    showResult('clip-result', t('resWrote', text.length));
  } catch (err) {
    showError('clip-result', err);
  }
}

/* ── 文件（权限 files，仅限皮肤自身目录） ─────────────────── */

async function onWriteNote() {
  if (!needBridge('file-result')) return;
  const data = document.getElementById('note-input').value;
  try {
    await window.__DESK_PP__.invoke('skin_write_file', { path: NOTE_PATH, data });
    showResult('file-result', t('resWroteFile', NOTE_PATH, data.length));
  } catch (err) {
    showError('file-result', err);
  }
}

async function onReadNote() {
  if (!needBridge('file-result')) return;
  try {
    const text = await window.__DESK_PP__.invoke('skin_read_file', { path: NOTE_PATH });
    showResult('file-result', text || t('resEmpty'));
  } catch (err) {
    showError('file-result', err);
  }
}

/** 列目录：path 省略时列皮肤根目录 */
async function onListDir(path) {
  if (!needBridge('file-result')) return;
  try {
    const entries = await window.__DESK_PP__.invoke('skin_list_dir', path ? { path } : {});
    showResult('file-result', formatDirEntries(entries));
  } catch (err) {
    showError('file-result', err);
  }
}

function formatDirEntries(entries) {
  if (!Array.isArray(entries) || entries.length === 0) return t('resDirEmpty');
  // 条目结构：{ name, is_dir, size }
  return entries
    .map((e) => (e.is_dir ? `[dir] ${e.name}/` : `      ${e.name}  ${e.size} B`))
    .join('\n');
}

async function onDeleteNote() {
  if (!needBridge('file-result')) return;
  try {
    await window.__DESK_PP__.invoke('skin_delete_file', { path: NOTE_PATH });
    showResult('file-result', t('resDone'));
  } catch (err) {
    showError('file-result', err);
  }
}

/* ── 注册表（权限 registry，只读） ────────────────────────── */

async function onReadRegistry(root, path, name) {
  if (!needBridge('reg-result')) return;
  try {
    const v = await window.__DESK_PP__.invoke('read_registry_value', { root, path, name });
    showResult('reg-result', formatRegistryValue(v));
  } catch (err) {
    showError('reg-result', err);
  }
}

/** 返回结构 { kind, value }；multi_string 是数组、binary 是 base64 */
function formatRegistryValue(v) {
  const lines = [`kind: ${v.kind}`];
  if (Array.isArray(v.value)) {
    lines.push(t('regMultiNote'));
    for (const item of v.value) lines.push(`  ${item}`);
  } else if (v.kind === 'binary') {
    lines.push(t('regBinaryNote'));
    lines.push(String(v.value));
  } else {
    lines.push(String(v.value));
  }
  return lines.join('\n');
}

/* ── 执行命令（权限 shell） ───────────────────────────────── */

async function onRunCommand(command, args, timeoutMs) {
  if (!needBridge('cmd-result')) return;
  if (!command) {
    showError('cmd-result', t('resNeedCmd'));
    return;
  }
  try {
    const r = await window.__DESK_PP__.invoke('run_command', { command, args, timeoutMs });
    // 返回结构 { code, stdout, stderr }；中文 Windows 的 GBK 输出后端已转码
    showResult('cmd-result',
      `${t('resExitCode', r.code)}\n── stdout ──\n${r.stdout || t('resEmpty')}\n── stderr ──\n${r.stderr || t('resEmpty')}`);
  } catch (err) {
    showError('cmd-result', err);
  }
}

function onRunCustom() {
  const command = document.getElementById('cmd-name').value.trim();
  const argsRaw = document.getElementById('cmd-args').value.trim();
  const args = splitArgs(argsRaw);
  const timeoutStr = document.getElementById('cmd-timeout').value.trim();
  // timeoutMs：默认 30000，上限 120000；空串/0/极小值按默认处理——
  // Number('')===0 且 Number.isFinite(0)===true，直接走 clamp 会落成 1ms
  //（后端下限抬到 100ms），任何命令必超时被杀
  const timeoutRaw = timeoutStr === '' ? NaN : Number(timeoutStr);
  const timeoutMs = Number.isFinite(timeoutRaw) && timeoutRaw >= 1000
    ? Math.min(120000, timeoutRaw)
    : 30000;
  onRunCommand(command, args, timeoutMs);
}

/* ── 打开链接/文件（权限 system） ─────────────────────────── */

/** 参数拆分支持引号："a b" 或 'a b' 整段为一个参数（此前纯空白拆分会把
    含空格的引号参数拆碎）。 */
function splitArgs(s) {
  if (!s) return [];
  const out = [];
  const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let m;
  while ((m = re.exec(s))) out.push(m[1] ?? m[2] ?? m[3]);
  return out;
}

async function onOpenExternal(target) {
  if (!needBridge('ext-result')) return;
  try {
    await window.__DESK_PP__.invoke('open_external', { target });
    showResult('ext-result', t('resOpened', target));
  } catch (err) {
    // 目标不存在与打开失败返回同样的错误；.exe 等可执行目标明确拒绝
    showError('ext-result', err);
  }
}

/* ── 设置读写（skin_get_setting / skin_set_setting，免权限） ─ */

async function onReadToken() {
  if (!needBridge('settings-result')) return;
  try {
    // password 类型的唯一读取通道；settings.api_token 恒为空串
    const token = await window.__DESK_PP__.invoke('skin_get_setting', { key: 'api_token' });
    showResult('settings-result', token ? t('resTokenSet', String(token).length) : t('resTokenEmpty'));
  } catch (err) {
    showError('settings-result', err);
  }
}

async function onSaveNote() {
  if (!needBridge('settings-result')) return;
  const value = document.getElementById('note-input').value;
  try {
    await window.__DESK_PP__.invoke('skin_set_setting', { key: 'note', value });
    // 契约 §4.4：写成功不会向自己回派 desk-setting-changed，文本框里已是新值
    showResult('settings-result', t('resSaved', 'note'));
  } catch (err) {
    showError('settings-result', err);
  }
}

async function onReadNoteCmd() {
  if (!needBridge('settings-result')) return;
  try {
    const value = await window.__DESK_PP__.invoke('skin_get_setting', { key: 'note' });
    showResult('settings-result', value || t('resEmpty'));
  } catch (err) {
    showError('settings-result', err);
  }
}

/* ── 待办列表（todolist：勾选/增删即写回） ────────────────── */

function normalizeTasks(value) {
  return (Array.isArray(value) ? value : []).map((it) => ({
    text: String(it?.text ?? ''),
    done: !!it?.done,
  }));
}

function renderTasks() {
  const ul = document.getElementById('task-list');
  ul.textContent = ''; // 清空重建（DOM API，无 innerHTML 注入面）
  if (tasks.length === 0) {
    const li = document.createElement('li');
    li.className = 'muted';
    li.textContent = t('resTasksEmpty');
    ul.appendChild(li);
    return;
  }
  tasks.forEach((item, i) => {
    const li = document.createElement('li');
    if (item.done) li.className = 'done';

    const cb = document.createElement('input');
    cb.type = 'checkbox';
    cb.checked = item.done;
    cb.addEventListener('change', () => {
      tasks[i].done = cb.checked;
      saveTasks();
      renderTasks(); // 刷新划线样式
    });

    const span = document.createElement('span');
    span.className = 'task-text';
    span.textContent = item.text;

    const del = document.createElement('button');
    del.className = 'task-del';
    del.textContent = '×';
    del.addEventListener('click', () => {
      tasks.splice(i, 1);
      saveTasks();
      renderTasks();
    });

    li.append(cb, span, del);
    ul.appendChild(li);
  });
}

/** 勾选/新增/删除后经 skin_set_setting 写回；写成功不回派事件（§4.4），UI 已是最新 */
async function saveTasks() {
  if (!canInvoke()) return;
  try {
    // 与后端约束一致的本地截断（200 字符/条、500 条）——超限时本地同步
    // 截断并明示，避免「提示已保存但持久值被后端静默截断」的不一致
    const before = tasks.length;
    tasks = tasks.slice(0, 500).map((it) =>
      typeof it?.text === 'string' && it.text.length > 200
        ? { ...it, text: it.text.slice(0, 200) }
        : it);
    await window.__DESK_PP__.invoke('skin_set_setting', { key: 'tasks', value: tasks });
    showResult('settings-result',
      before !== tasks.length ? t('resSavedTruncated', 'tasks') : t('resSaved', 'tasks'));
  } catch (err) {
    // 校验失败（如单条超 200 字符被静默截断之外的硬错误）显示在结果区
    showError('settings-result', err);
  }
}

function onAddTask() {
  const input = document.getElementById('task-input');
  const text = input.value.trim();
  if (!text) return;
  tasks.push({ text, done: false });
  input.value = '';
  saveTasks();
  renderTasks();
}

/* ── 渲染汇总 ─────────────────────────────────────────────── */

function renderAll() {
  applyI18n();
  renderBridgeBadge();
  renderPlaceholders();
  renderTasks();
}

function setLang(next) {
  lang = I18N[next] ? next : 'zh-CN';
  document.documentElement.lang = lang;
  renderAll();
}

/* ── 事件 ─────────────────────────────────────────────────── */

// 管理器修改设置后实时推送（无需重载）；皮肤自己写的不回派（§4.4）
document.addEventListener('desk-setting-changed', (e) => {
  const { key, value } = e.detail || {};
  if (!key) return;
  settings[key] = value; // 桥也会同步，双保险
  if (key === 'note') {
    // 焦点保护：用户正在文本框里编辑时不冲掉进行中的内容
    const noteEl = document.getElementById('note-input');
    if (document.activeElement === noteEl) return;
    noteEl.value = value || '';
  } else if (key === 'tasks') {
    tasks = normalizeTasks(value);
    renderTasks();
  } else if (key === 'api_token') {
    // 事件携带真实值，但 password 原文不应展示在页面上——提示重新读取状态即可
    showResult('settings-result', t('resTokenChanged'));
  }
});

// 管理器切换语言后实时推送：皮肤界面语言跟随管理器
document.addEventListener('desk-language-changed', (e) => {
  setLang(e.detail?.language);
});

/* ── 启动 ─────────────────────────────────────────────────── */

function bindButtons() {
  document.getElementById('btn-read-clip').addEventListener('click', onReadClipboard);
  document.getElementById('btn-write-clip').addEventListener('click', onWriteClipboard);

  document.getElementById('btn-write-note').addEventListener('click', onWriteNote);
  document.getElementById('btn-read-note').addEventListener('click', onReadNote);
  document.getElementById('btn-list-data').addEventListener('click', () => onListDir('data'));
  document.getElementById('btn-list-root').addEventListener('click', () => onListDir());
  document.getElementById('btn-delete-note').addEventListener('click', onDeleteNote);

  document.getElementById('btn-reg-preset').addEventListener('click',
    () => onReadRegistry('HKCU', 'Environment', 'TEMP'));
  document.getElementById('btn-reg-read').addEventListener('click', () => {
    onReadRegistry(
      document.getElementById('reg-root').value,
      document.getElementById('reg-path').value.trim(),
      document.getElementById('reg-name').value.trim());
  });

  document.getElementById('btn-cmd-ver').addEventListener('click',
    () => onRunCommand('cmd', ['/c', 'ver'], 30000));
  document.getElementById('btn-cmd-ipconfig').addEventListener('click',
    () => onRunCommand('cmd', ['/c', 'ipconfig'], 30000));
  document.getElementById('btn-cmd-run').addEventListener('click', onRunCustom);

  document.getElementById('btn-ext-open').addEventListener('click',
    () => onOpenExternal(document.getElementById('ext-target').value.trim()));
  // 故意用一个可执行文件路径演示 open_external 的拒绝错误文案
  document.getElementById('btn-ext-denied').addEventListener('click',
    () => onOpenExternal('C:\\Windows\\System32\\notepad.exe'));

  document.getElementById('btn-read-token').addEventListener('click', onReadToken);
  document.getElementById('btn-save-note').addEventListener('click', onSaveNote);
  document.getElementById('btn-read-note-cmd').addEventListener('click', onReadNoteCmd);
  document.getElementById('btn-task-add').addEventListener('click', onAddTask);
  document.getElementById('task-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') onAddTask();
  });
}

(function init() {
  settings = window.__DESK_PP__?.settings || {};
  lang = I18N[window.__DESK_PP__?.language] ? window.__DESK_PP__.language : 'zh-CN';
  document.documentElement.lang = lang;
  tasks = normalizeTasks(settings.tasks);
  document.getElementById('note-input').value = settings.note || '';
  bindButtons();
  renderAll();
})();
