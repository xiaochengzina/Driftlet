/**
 * driftlet.js — Driftlet 皮肤桥的可选封装（copy 进你的皮肤文件夹即可用）
 *
 * 用法：
 *   <script src="driftlet.js"></script>   <!-- 在你的主脚本之前引入 -->
 *   const [cpu] = await Driftlet.getCpuInfo();
 *
 * - 只是把 window.driftlet.invoke('命令', 参数) 包成命名函数，无任何魔法；
 *   命令契约（参数形状、返回结构、权限声明）以《皮肤开发指南》§5 为准。
 * - 桥不存在时（纯浏览器打开页面调试，指南 §7.2）所有命令 reject 一个
 *   明确的 Error —— 用 .catch 或 try/catch 兜住即可正常离线渲染。
 * - 配套 driftlet.d.ts：同目录引入即可获得编辑器自动补全与类型检查。
 */
'use strict';

window.Driftlet = (() => {
  const bridge = () => window.driftlet || window.__DESK_PP__;

  function call(cmd, args) {
    const b = bridge();
    if (!b || typeof b.invoke !== 'function') {
      return Promise.reject(new Error('Driftlet bridge is not available (not running inside the host)'));
    }
    return b.invoke(cmd, args || {});
  }

  return {
    /** 当前生效的设置值（key → value；password 类型的键恒为空串，用 getSetting 读） */
    get settings() { return bridge()?.settings || {}; },
    /** 管理器界面语言（"zh-CN" / "en"） */
    get language() { return bridge()?.language || 'zh-CN'; },
    /** 管理器当前主题（"light" / "dark"；auto 已由后端折算成具体值） */
    get theme() { return bridge()?.theme || 'light'; },
    /** 宿主版本号（如 "1.0.5"）：能力探测用，按数字段比较 */
    get hostVersion() { return bridge()?.hostVersion || ''; },

    // ── 系统信息（只读，免权限）。速率类读数首次调用返回 0（基线），建议每秒轮询 ──
    getCpuInfo: () => call('get_cpu_info'),
    getGpuInfo: () => call('get_gpu_info'),
    getMemoryInfo: () => call('get_memory_info'),
    getDisksInfo: () => call('get_disks_info'),
    getDiskSpace: (path) => call('get_disk_space', { path }),
    getNetworkInfo: () => call('get_network_info'),
    getAudioSpectrum: (bands) => call('get_audio_spectrum', { bands }),
    getOsInfo: () => call('get_os_info'),
    getProcesses: (sort, limit) => call('get_processes', { sort, limit }),
    getVolume: () => call('get_volume'),
    getMediaInfo: () => call('get_media_info'),       // 无播放会话时 resolve null
    getBatteryInfo: () => call('get_battery_info'),
    getIdleTime: () => call('get_idle_time'),
    getForegroundWindowInfo: () => call('get_foreground_window_info'),
    getMonitors: () => call('get_monitors'),
    getSystemTheme: () => call('get_system_theme'),   // Windows 系统级 "light"/"dark"（AppsUseLightTheme）

    // ── 皮肤目录文件（免权限；沙箱限自身目录，binary: true 时 base64 收发）──
    readFile: (path, binary) => call('skin_read_file', { path, binary }),
    writeFile: (path, data, binary) => call('skin_write_file', { path, data, binary }),
    listDir: (path) => call('skin_list_dir', { path }),
    deleteFile: (path) => call('skin_delete_file', { path }),

    // ── 设置读写与日志（免权限；只能读写自己声明过的 key）──
    getSetting: (key) => call('skin_get_setting', { key }),
    setSetting: (key, value) => call('skin_set_setting', { key, value }),
    log: (message, level) => call('skin_log', { level, message }),
    // 隐藏任意已加载皮肤窗口（省略 id = 自己，免权限；指定他人需 control 权限）
    hideSkin: (skinId) => call('skin_hide', { skinId }),
    // 显示任意已加载皮肤窗口（同上；只显示不抢焦点）
    showSkin: (skinId) => call('skin_show', { skinId }),
    // 皮肤间广播（免权限；所有已加载皮肤含自己都会收到 desk-skin-message）
    broadcast: (channel, payload) => call('skin_broadcast', { channel, payload }),

    // ── 敏感能力（需在 skin.json 声明对应 permissions）──
    readRegistryValue: (root, path, name) => call('read_registry_value', { root, path, name }),  // registry
    runCommand: (command, args, timeoutMs) => call('run_command', { command, args, timeoutMs }), // shell
    setVolume: (volumePct) => call('set_volume', { volumePct }),    // system（0–100，越界钳制）
    setMute: (muted) => call('set_mute', { muted }),                // system
    mediaControl: (action) => call('media_control', { action }),    // system
    readClipboardText: () => call('read_clipboard_text'),           // clipboard
    writeClipboardText: (text) => call('write_clipboard_text', { text }), // clipboard
    openExternal: (target) => call('open_external', { target }),    // system
    showNotification: (title, body) => call('show_notification', { title, body }), // system
    getMicSpectrum: (bands) => call('get_mic_spectrum', { bands }), // mic
    readAnyFile: (path, binary) => call('skin_read_any_file', { path, binary }),   // file_system（任意绝对路径；错误透传系统报错）
    writeAnyFile: (path, data, binary) => call('skin_write_any_file', { path, data, binary }), // file_system（父目录自动创建）
    listAnyDir: (path) => call('skin_list_any_dir', { path }),   // file_system（与 listDir 同款条目结构）
    createAnyDir: (path) => call('skin_create_any_dir', { path }), // file_system（含多级，已存在视为成功）
    deleteAnyPath: (path, recursive) => call('skin_delete_any_path', { path, recursive }), // file_system（目录树须 recursive: true）
    // file_system：外部文件的引用 URL（<img src> / CSS url() 直接用，不经 JS 内存）
    fileUrl: (path) => 'http://skin.localhost/__fs__?path=' + encodeURIComponent(path),
    listSkins: () => call('skin_list_skins'),       // control（已安装皮肤清单：id/name/version/loaded/hidden）
    getWindowConfig: (skinId) => call('skin_get_window_config', { skinId }),  // 省略 id = 自己，免权限；他人需 control
    setWindowConfig: (skinId, patch) => call('skin_set_window_config', { skinId, patch }), // 省略 id = 改自己全键免权限；他人需 control
    loadSkin: (skinId) => call('skin_load', { skinId }),        // 省略 id = 自己免权限；他人需 control
    unloadSkin: (skinId) => call('skin_unload', { skinId }),    // 同上（目标是自己时 fire-and-forget，返回值不可依赖）
    reloadSkin: (skinId) => call('skin_reload', { skinId }),    // 同上
    httpRequest: (url, opts) => call('http_request', {          // 免权限（突破 CORS；页面本有 fetch 通道）
      url,
      method: opts?.method,
      headers: opts?.headers,
      body: opts?.body,
      timeoutMs: opts?.timeoutMs,
      binary: opts?.binary,                                     // true 时 body 按 base64 收发，响应含 headers
    }),

    // ── 事件助手（返回解绑函数）──
    /** 管理器改了本皮肤的设置：fn(key, value)；__DESK_PP__.settings 已同步 */
    onSettingChanged(fn) {
      const h = (e) => fn(e.detail?.key, e.detail?.value);
      document.addEventListener('desk-setting-changed', h);
      return () => document.removeEventListener('desk-setting-changed', h);
    },
    /** 管理器切换了界面语言：fn(language)，language 为 "zh-CN" / "en" */
    onLanguageChanged(fn) {
      const h = (e) => fn(e.detail?.language);
      document.addEventListener('desk-language-changed', h);
      return () => document.removeEventListener('desk-language-changed', h);
    },
    /** 管理器切换了主题：fn(theme)，theme 为 "light" / "dark"（已是折算值） */
    onThemeChanged(fn) {
      const h = (e) => fn(e.detail?.theme);
      document.addEventListener('desk-theme-changed', h);
      return () => document.removeEventListener('desk-theme-changed', h);
    },
    /** 其他皮肤（或自己）经 broadcast 发来消息：fn(channel, payload, fromSkinId) */
    onSkinMessage(fn) {
      const h = (e) => fn(e.detail?.channel, e.detail?.payload, e.detail?.from);
      document.addEventListener('desk-skin-message', h);
      return () => document.removeEventListener('desk-skin-message', h);
    },
    /** 本皮肤的窗口配置被改（管理器面板或 control 命令均可触发）：fn(key, value)，
        key 为 opacity/placement/click_through/position_locked/resizable/zoom/edge_snap/snap_gap/position/size 之一 */
    onWindowConfigChanged(fn) {
      const h = (e) => fn(e.detail?.key, e.detail?.value);
      document.addEventListener('desk-window-config-changed', h);
      return () => document.removeEventListener('desk-window-config-changed', h);
    },
  };
})();
