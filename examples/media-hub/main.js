'use strict';

/**
 * media-hub —— 音量 / 媒体 / 频谱 / 通知接口演示。
 *
 * 四张卡片演示 8 个后端命令：
 *   音量卡片：get_volume（免权限，启动读 + 每次操作后刷新）、
 *             set_volume（滑块 change 时才调用）/ set_mute（开关）——需 system 权限；
 *   媒体卡片：get_media_info（1 秒轮询；无播放会话返回 null，不是错误）、
 *             media_control（返回布尔 = 播放器是否接受；无会话时 reject）——需 system 权限；
 *   频谱卡片：get_audio_spectrum（系统环回，免权限，40ms 轮询，页面隐藏时暂停）、
 *             get_mic_spectrum（麦克风，需 mic 权限；停止轮询约 30 秒后设备自动释放）；
 *   通知卡片：show_notification（Windows toast，需 system 权限）。
 *
 * 约定：动态文本一律 textContent / DOM API，不拼 innerHTML；
 * 命令 reject 的可读文案显示在对应卡片的错误行里（权限模型的现场演示）；
 * 桥不存在（纯浏览器调试）时各区域显示占位文案，不抛错。
 */

/* ── 界面文案字典（决定【皮肤页面】的语言，跟随管理器切换，见指南 §4.5） ── */
const I18N = {
  'zh-CN': {
    title: '媒体控制台',
    subtitle: '音量 / 媒体 / 频谱 / 通知接口演示',
    bridgeOff: '桥不可用：纯浏览器调试中，后端命令无法调用',
    volHeading: '系统音量',
    volNote: 'get_volume 免权限；set_volume / set_mute 需 system 权限；滑块松手（change）时才调用 set_volume',
    mute: '静音',
    unmute: '取消静音',
    volStatus: (pct, muted) => `当前音量 ${pct}%${muted ? ' · 已静音' : ''}`,
    mediaHeading: '正在播放',
    mediaNote: 'get_media_info 每秒轮询；无播放会话时返回 null（不是错误）；media_control 需 system 权限',
    noSession: '无播放会话',
    noTitle: '（无标题）',
    stPlaying: '播放中',
    stPaused: '已暂停',
    stStopped: '已停止',
    prevTitle: '上一首（previous）',
    playPauseTitle: '播放 / 暂停（play_pause）',
    nextTitle: '下一首（next）',
    accepted: '播放器已接受该操作',
    notAccepted: '播放器未接受该操作',
    specHeading: '音频频谱',
    srcLoop: '系统声音',
    srcMic: '麦克风',
    peak: (v) => `峰值 ${v}`,
    specNoteLoop: 'get_audio_spectrum 免权限（WASAPI 环回，采集系统正在播放的声音）；40ms 轮询，页面隐藏时自动暂停',
    specNoteMic: 'get_mic_spectrum 需 mic 权限（真实拾音，涉及隐私）；停止轮询约 30 秒后设备自动释放',
    notifyHeading: '系统通知',
    notifyBtn: '发送演示通知',
    notifyOk: '通知已发送：Windows toast，操作中心可见，来源显示为 Driftlet',
    demoTitle: '媒体控制台',
    demoBody: '这是一条来自 media-hub 皮肤的演示通知。',
    notifyNote: 'show_notification 需 system 权限；请克制频率——用户被刷屏后会直接关掉整个应用的通知权限',
  },
  en: {
    title: 'Media Hub',
    subtitle: 'Volume, media, spectrum & notification API demo',
    bridgeOff: 'Bridge unavailable: debugging in a plain browser — backend commands cannot be called',
    volHeading: 'System Volume',
    volNote: 'get_volume needs no permission; set_volume / set_mute require the system permission; the slider only calls set_volume on change (release)',
    mute: 'Mute',
    unmute: 'Unmute',
    volStatus: (pct, muted) => `Volume ${pct}%${muted ? ' · muted' : ''}`,
    mediaHeading: 'Now Playing',
    mediaNote: 'get_media_info polls every second; returns null (not an error) when there is no playback session; media_control requires the system permission',
    noSession: 'No playback session',
    noTitle: '(untitled)',
    stPlaying: 'Playing',
    stPaused: 'Paused',
    stStopped: 'Stopped',
    prevTitle: 'Previous track (previous)',
    playPauseTitle: 'Play / pause (play_pause)',
    nextTitle: 'Next track (next)',
    accepted: 'The player accepted the action',
    notAccepted: 'The player did not accept the action',
    specHeading: 'Audio Spectrum',
    srcLoop: 'System audio',
    srcMic: 'Microphone',
    peak: (v) => `Peak ${v}`,
    specNoteLoop: 'get_audio_spectrum needs no permission (WASAPI loopback — captures what the system is playing); 40ms polling, auto-paused while the page is hidden',
    specNoteMic: 'get_mic_spectrum requires the mic permission (real capture, privacy-sensitive); the device auto-releases ~30s after polling stops',
    notifyHeading: 'Notification',
    notifyBtn: 'Send demo notification',
    notifyOk: 'Notification sent: a Windows toast, visible in Action Center, sourced from Driftlet',
    demoTitle: 'Media Hub',
    demoBody: 'This is a demo notification from the media-hub skin.',
    notifyNote: 'show_notification requires the system permission; keep the frequency low — spammed users will simply turn off notifications for the whole app',
  },
};

/* ── 桥与全局状态 ─────────────────────────────────────────── */

const bridge = window.__DESK_PP__ ?? null;          // 纯浏览器调试时为 null
const canInvoke = !!(bridge && typeof bridge.invoke === 'function');

let lang = 'zh-CN';

let volState = null;        // { pct: 0-100, muted: bool }，null = 尚未读到
let volErr = '';

let mediaState = null;      // get_media_info 结果；null = 无播放会话
let mediaErr = '';          // 轮询本身的错误
let feedbackToken = '';     // '' | 'accepted' | 'notAccepted'（存 token 以便切语言时重译）
let feedbackErr = '';       // media_control 的 reject 文案

let specSource = 'loop';    // 'loop' = 系统环回 | 'mic' = 麦克风
let specErr = '';
let lastBands = new Array(24).fill(0);
let lastPeak = 0;

let notifyOkShown = false;  // 存布尔而非文案，切语言时重译
let notifyErr = '';

const $ = (id) => document.getElementById(id);

function t(key, ...args) {
  const entry = (I18N[lang] && I18N[lang][key]) ?? I18N['zh-CN'][key];
  return typeof entry === 'function' ? entry(...args) : (entry ?? key);
}

/** 统一调用入口：无桥时 reject 占位文案，让各卡片走同一条错误显示路径 */
async function call(cmd, args) {
  if (!canInvoke) throw new Error(t('bridgeOff'));
  return bridge.invoke(cmd, args);
}

/** 错误行通用赋值：空串时隐藏 */
function setLine(el, msg) {
  el.textContent = msg || '';
  el.hidden = !msg;
}

/* ── 静态文案渲染（语言切换时重跑） ───────────────────────── */

function renderStatic() {
  $('skin-title').textContent = t('title');
  $('subtitle').textContent = t('subtitle');
  $('vol-heading').textContent = t('volHeading');
  $('vol-note').textContent = t('volNote');
  $('media-heading').textContent = t('mediaHeading');
  $('media-note').textContent = t('mediaNote');
  $('spec-heading').textContent = t('specHeading');
  $('notify-heading').textContent = t('notifyHeading');
  $('notify-note').textContent = t('notifyNote');
  $('btn-notify').textContent = t('notifyBtn');
  $('btn-prev').title = t('prevTitle');
  $('btn-playpause').title = t('playPauseTitle');
  $('btn-next').title = t('nextTitle');
  $('src-loop').textContent = t('srcLoop');
  $('src-mic').textContent = t('srcMic');
}

/* ── 音量卡片：get_volume / set_volume / set_mute ─────────── */

function renderVolume() {
  // 静音按钮文案始终渲染（首次读到音量前 / 纯浏览器模式下也有默认文案）
  const muteBtn = $('btn-mute');
  muteBtn.textContent = volState?.muted ? t('unmute') : t('mute');
  muteBtn.classList.toggle('on', !!volState?.muted);
  if (!canInvoke) {
    $('vol-status').textContent = t('bridgeOff');
  } else if (volState) {
    $('vol-slider').value = volState.pct;
    $('vol-pct').textContent = `${volState.pct}%`;
    $('vol-status').textContent = t('volStatus', volState.pct, volState.muted);
  }
  setLine($('vol-error'), volErr);
}

/** 回读真实音量状态（启动时 + 每次写操作后） */
async function refreshVolume() {
  try {
    const v = await call('get_volume');
    volState = { pct: Math.round(Number(v?.volume_pct) || 0), muted: !!v?.muted };
    volErr = '';
  } catch (err) {
    volErr = String(err);
  }
  renderVolume();
}

/* ── 媒体卡片：get_media_info / media_control ─────────────── */

/** 秒 → m:ss */
function fmtTime(secs) {
  const s = Math.max(0, Math.round(Number(secs) || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function statusText(status) {
  if (status === 'playing') return t('stPlaying');
  if (status === 'paused') return t('stPaused');
  if (status === 'stopped') return t('stStopped');
  return status || '';
}

function renderMedia() {
  const m = mediaState;
  const has = !!m;

  // 封面：cover_base64 非空时按 data URI 显示，否则显示占位符
  const coverImg = $('media-cover');
  const coverPh = $('media-cover-ph');
  if (has && m.cover_base64) {
    coverImg.src = `data:image/jpeg;base64,${m.cover_base64}`;
    coverImg.hidden = false;
    coverPh.hidden = true;
  } else {
    coverImg.removeAttribute('src'); // 释放上一张封面
    coverImg.hidden = true;
    coverPh.hidden = false;
  }

  $('media-title').textContent = has ? (m.title || t('noTitle')) : t('noSession');
  $('media-artist').textContent = has ? (m.artist || '') : '';
  $('media-album').textContent = has ? (m.album || '') : '';
  $('media-status').textContent = has ? statusText(m.status) : '';

  // 进度：duration 可能为 0（流媒体不上报），此时不显示进度
  const dur = has ? Number(m.duration_secs) || 0 : 0;
  const pos = has ? Math.max(0, Number(m.position_secs) || 0) : 0;
  $('media-progress').style.width = dur > 0 ? `${Math.min(100, (pos / dur) * 100)}%` : '0%';
  $('media-pos').textContent = dur > 0 ? fmtTime(pos) : '';
  $('media-dur').textContent = dur > 0 ? fmtTime(dur) : '';

  // 控制反馈：布尔结果译为「已接受 / 未接受」；reject 文案原样显示（如「无播放会话」）
  const feedback = $('media-feedback');
  if (!canInvoke) {
    feedback.textContent = t('bridgeOff');
    feedback.classList.remove('is-error');
  } else {
    feedback.textContent = feedbackErr || (feedbackToken ? t(feedbackToken) : '');
    feedback.classList.toggle('is-error', !!feedbackErr);
  }
  setLine($('media-error'), mediaErr);
}

let mediaBusy = false; // 防重入：上一次轮询未返回时不叠加

async function pollMedia() {
  if (mediaBusy) return;
  mediaBusy = true;
  try {
    mediaState = await call('get_media_info'); // 无会话返回 null（不是错误）
    mediaErr = '';
  } catch (err) {
    mediaErr = String(err);
  } finally {
    mediaBusy = false;
  }
  renderMedia();
}

async function sendMediaControl(action) {
  try {
    const ok = await call('media_control', { action });
    feedbackToken = ok ? 'accepted' : 'notAccepted';
    feedbackErr = '';
  } catch (err) {
    feedbackToken = '';
    feedbackErr = String(err); // 例如：无播放会话 / 未声明 system 权限
  }
  renderMedia();
  pollMedia(); // 操作后立即刷新一次媒体状态
}

/* ── 频谱卡片：get_audio_spectrum / get_mic_spectrum ──────── */

const BAR_COLOR = '#0e75c3';   // 与 style.css 的 --accent 保持一致
const PEAK_COLOR = '#e8590c';

function renderSpectrumMeta() {
  $('src-loop').classList.toggle('on', specSource === 'loop');
  $('src-mic').classList.toggle('on', specSource === 'mic');
  $('spec-note').textContent = specSource === 'mic' ? t('specNoteMic') : t('specNoteLoop');
  $('spec-peak').textContent = canInvoke ? t('peak', lastPeak.toFixed(2)) : t('bridgeOff');
  setLine($('spec-error'), specErr);
}

/** 柱状频谱 + 峰值指示线；画布按实际显示尺寸（×DPR）重置，窄窗口与高分屏下都清晰 */
function drawSpectrum() {
  const canvas = $('spec-canvas');
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (!w || !h) return;
  const pxW = Math.round(w * dpr);
  const pxH = Math.round(h * dpr);
  if (canvas.width !== pxW || canvas.height !== pxH) {
    canvas.width = pxW;
    canvas.height = pxH;
  }
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const n = lastBands.length || 1;
  const gap = 2;
  const barW = Math.max(1, (w - gap * (n - 1)) / n);
  ctx.fillStyle = BAR_COLOR;
  for (let i = 0; i < n; i++) {
    const v = Math.max(0, Math.min(1, Number(lastBands[i]) || 0));
    const barH = Math.max(1, v * (h - 2));
    ctx.fillRect(i * (barW + gap), h - barH, barW, barH);
  }

  // 峰值指示：一条横线标出瞬时峰值音量（peak 是整体标量，不是分频段峰值）
  if (lastPeak > 0) {
    ctx.fillStyle = PEAK_COLOR;
    ctx.fillRect(0, h - lastPeak * h, w, 2);
  }
}

/** 切换数据源：环回 ⇄ 麦克风（麦克风侧即「启动 / 停止轮询」的开关） */
function setSpecSource(src) {
  if (specSource === src) return;
  specSource = src;
  specErr = '';
  lastBands = new Array(24).fill(0);
  lastPeak = 0;
  renderSpectrumMeta();
}

let specBusy = false; // 防重入：40ms 间隔内上一次未返回则跳过本帧

async function specTick() {
  // 页面隐藏时暂停轮询：省 CPU，且环回/麦克风闲置约 30 秒后由后端自动释放设备
  if (document.hidden || specBusy) return;
  specBusy = true;
  const cmd = specSource === 'mic' ? 'get_mic_spectrum' : 'get_audio_spectrum';
  try {
    const r = await call(cmd, { bands: 24 });
    lastBands = Array.isArray(r?.bands) && r.bands.length ? r.bands : lastBands;
    lastPeak = Math.max(0, Math.min(1, Number(r?.peak) || 0));
    specErr = '';
  } catch (err) {
    // 例如：未声明 mic 权限 / 无麦克风设备 / 系统隐私设置禁用了麦克风
    specErr = String(err);
    lastBands = new Array(24).fill(0); // 出错时柱条归零，不停格在旧数据
    lastPeak = 0;
  } finally {
    specBusy = false;
  }
  drawSpectrum();
  renderSpectrumMeta();
}

/* ── 通知卡片：show_notification ──────────────────────────── */

function renderNotify() {
  if (!canInvoke) {
    setLine($('notify-status'), t('bridgeOff'));
    return;
  }
  setLine($('notify-status'), notifyOkShown ? t('notifyOk') : '');
  setLine($('notify-error'), notifyErr);
}

async function sendNotification() {
  try {
    await call('show_notification', { title: t('demoTitle'), body: t('demoBody') });
    notifyOkShown = true;
    notifyErr = '';
  } catch (err) {
    notifyOkShown = false;
    notifyErr = String(err); // 例如：未声明 system 权限
  }
  renderNotify();
}

/* ── 整体渲染与语言切换 ───────────────────────────────────── */

function renderAll() {
  renderStatic();
  renderVolume();
  renderMedia();
  renderSpectrumMeta();
  renderNotify();
}

function setLang(next) {
  lang = I18N[next] ? next : 'zh-CN';
  document.documentElement.lang = lang;
  renderAll();
}

// 管理器切换语言后实时推送：皮肤界面语言跟随管理器（§4.5）
document.addEventListener('desk-language-changed', (e) => {
  setLang(e.detail?.language ?? bridge?.language);
});

/* ── 交互事件 ─────────────────────────────────────────────── */

// 拖动滑块不应触发窗口拖动（§3.2：pointerdown 被干扰时 stopPropagation）
$('vol-slider').addEventListener('pointerdown', (e) => e.stopPropagation());
// input 只更新本地显示，change（松手）才调用 set_volume——避免拖动过程中密集写系统音量
$('vol-slider').addEventListener('input', () => {
  $('vol-pct').textContent = `${$('vol-slider').value}%`;
});
$('vol-slider').addEventListener('change', async () => {
  try {
    await call('set_volume', { volumePct: Number($('vol-slider').value) });
    volErr = '';
  } catch (err) {
    volErr = String(err); // 例如：未声明 system 权限
  }
  await refreshVolume(); // 无论成败都回读真实状态（失败时滑块回弹）
});

$('btn-mute').addEventListener('click', async () => {
  try {
    await call('set_mute', { muted: !(volState?.muted) });
    volErr = '';
  } catch (err) {
    volErr = String(err);
  }
  await refreshVolume();
});

$('btn-prev').addEventListener('click', () => sendMediaControl('previous'));
$('btn-playpause').addEventListener('click', () => sendMediaControl('play_pause'));
$('btn-next').addEventListener('click', () => sendMediaControl('next'));

$('src-loop').addEventListener('click', () => setSpecSource('loop'));
$('src-mic').addEventListener('click', () => setSpecSource('mic'));

$('btn-notify').addEventListener('click', () => sendNotification());

/* ── 启动 ─────────────────────────────────────────────────── */

(async function init() {
  lang = I18N[bridge?.language] ? bridge.language : 'zh-CN';
  document.documentElement.lang = lang;
  renderAll();      // 无桥时各区域在此渲染「桥不可用」占位
  drawSpectrum();   // 全零初始帧
  if (!canInvoke) return;
  await refreshVolume(); // get_volume 免权限：启动读一次
  await pollMedia();
  setInterval(pollMedia, 1000); // 媒体信息 1 秒轮询
  setInterval(specTick, 40);    // 频谱 40ms 轮询（文档建议 30–50ms）
})();
