/**
 * api.js — Tauri IPC wrapper
 * Uses @tauri-apps/api v2 for all communication with the Rust backend.
 */
import { invoke } from '@tauri-apps/api/core';

const API = {
  // Skin discovery
  listSkins() {
    return invoke('list_skins');
  },

  getSkinDetail(skinId) {
    return invoke('get_skin_detail', { skinId });
  },

  // Skin lifecycle
  loadSkin(skinId) {
    return invoke('load_skin', { skinId });
  },

  unloadSkin(skinId) {
    return invoke('unload_skin', { skinId });
  },

  reloadSkin(skinId) {
    return invoke('reload_skin', { skinId });
  },

  // Configuration
  setOpacity(skinId, opacity) {
    return invoke('set_skin_opacity', { skinId, opacity });
  },

  setPlacement(skinId, placement) {
    return invoke('set_skin_placement', { skinId, placement });
  },

  setClickThrough(skinId, on) {
    return invoke('set_skin_click_through', { skinId, on });
  },

  setPositionLocked(skinId, locked) {
    return invoke('set_skin_position_locked', { skinId, locked });
  },

  setResizable(skinId, resizable) {
    return invoke('set_skin_resizable', { skinId, resizable });
  },

  // 缩放比例：整体缩放窗口与内容（0.5–2.0）
  setZoom(skinId, zoom) {
    return invoke('set_skin_zoom', { skinId, zoom });
  },

  // 边缘吸附开关：拖动靠近屏幕/其他皮肤窗口边缘时自动对齐
  setEdgeSnap(skinId, on) {
    return invoke('set_skin_edge_snap', { skinId, on });
  },

  // 吸附间距（逻辑像素）
  setSnapGap(skinId, gap) {
    return invoke('set_skin_snap_gap', { skinId, gap });
  },

  setPosition(skinId, x, y) {
    return invoke('set_skin_position', { skinId, x, y });
  },

  setSize(skinId, width, height) {
    return invoke('set_skin_size', { skinId, width, height });
  },

  setSkinCustomSetting(skinId, key, value) {
    return invoke('set_skin_custom_setting', { skinId, key, value });
  },

  // 重置皮肤全部持久化数据（窗口配置 + 自定义设置），恢复 skin.json 默认值
  resetSkinConfig(skinId) {
    return invoke('reset_skin_config', { skinId });
  },

  // 皮肤包安装（.dskin）
  pickSkinPackage() {
    return invoke('pick_skin_package');
  },

  inspectSkinPackage(packagePath) {
    return invoke('inspect_skin_package', { packagePath });
  },

  installSkinPackage(packagePath) {
    return invoke('install_skin_package', { packagePath });
  },

  // 双击 .dskin 冷启动时后端暂存的待安装包路径（消费型，只取到一次）
  takePendingPackageInstall() {
    return invoke('take_pending_package_install');
  },

  removeSkin(skinId) {
    return invoke('remove_skin', { skinId });
  },

  // Preview
  capturePreview(skinId) {
    return invoke('capture_skin_preview', { skinId });
  },

  /** 皮肤预览图 URL（<img> 用）：assetProtocol 已删，改走 skin:// 协议直出
   * （处理器自带 settings.json 拦截与 canonicalize 防护）。
   *  preview 路径形如 <skins_dir>/<皮肤文件夹>/preview.<ext>，协议按皮肤
   *  文件夹下的相对路径取文件，故取路径末两段拼 URL。
   *  Windows 上自定义协议以 http://<scheme>.localhost 形式代理（wry
   *  workaround，skin:// 在子资源加载里不可解析），其余平台用原始 scheme。 */
  assetUrl(filePath) {
    const segments = String(filePath).split(/[\\/]/).filter(Boolean);
    const relPath = segments.slice(-2).join('/');
    const origin = navigator.userAgent.includes('Windows') ? 'http://skin.localhost' : 'skin://localhost';
    return `${origin}/${relPath}`;
  },

  // App config
  getAppConfig() {
    return invoke('get_app_config');
  },

  // Settings
  setAutostart(on) {
    return invoke('set_autostart', { on });
  },

  getAutostart() {
    return invoke('get_autostart');
  },

  setTheme(theme) {
    return invoke('set_theme', { theme });
  },

  setLanguage(language) {
    return invoke('set_language', { language });
  },

  // 全局快捷键（空串 = 禁用）
  setHotkey(hotkey) {
    return invoke('set_hotkey', { hotkey });
  },

  // 启动时快捷键注册失败的组合（消费型，只取到一次）
  takeHotkeyError() {
    return invoke('take_hotkey_error');
  },

  // 布局备份：导出 config/ + skins/ 为 zip（返回保存路径，取消为 null）；
  // 导入备份 zip（返回是否已导入，取消为 false）
  exportConfig() {
    return invoke('export_config');
  },

  importConfig() {
    return invoke('import_config');
  },

  // 皮肤热重载开关（仅开发构建生效）
  setHotReload(on) {
    return invoke('set_hot_reload', { on });
  },

  // 更新检测：查 GitHub 最新 release（网络失败 reject，调用方静默处理）；
  // 开关持久化；「前往下载」打开后端固定的最新 release 页
  checkUpdate() {
    return invoke('check_update');
  },

  setUpdateCheck(on) {
    return invoke('set_update_check', { on });
  },

  openReleasePage() {
    return invoke('open_release_page');
  },

  // 打开日志窗口（已开着则提到前台；窗口由后端创建，label "log"）
  openLogWindow() {
    return invoke('open_log_window');
  },

  // Utility
  openSkinsFolder() {
    return invoke('open_skins_folder');
  },

  listSystemFonts() {
    return invoke('list_system_fonts');
  },

  openSkinFolder(skinId) {
    return invoke('open_skin_folder', { skinId });
  },
};

export default API;
