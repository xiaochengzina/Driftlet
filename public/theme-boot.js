// 首帧主题引导：管理器窗口创建时，后端把当前生效主题烘进入口 URL
//（index.html?theme=light|dark——auto 已在 Rust 侧按小时折算成具体值，
// 见 config::current_theme，与日志窗 log.html?theme=… 同一模式）。
// 本脚本以经典脚本（非 module）同步阻塞执行，赶在样式表加载与首次绘制
// 之前把 data-theme 落到 <html>——深色主题下管理器唤出不再闪浅色一帧
//（此前 renderShell 先以浅色默认主题首绘，initTheme 的 IPC 往返后才切
// 深色）。CSP script-src 'self' 禁内联脚本，故须独立文件。
// 无参数或参数非法（如浏览器直开 dev server）时不动作：沿用默认浅色，
// 由 settings.js initTheme 随后按配置矫正。
(function () {
  var theme = new URLSearchParams(location.search).get('theme');
  if (theme === 'light' || theme === 'dark') {
    document.documentElement.setAttribute('data-theme', theme);
  }
})();
