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

  // 复位到屏幕内：完全出屏才移动，返回是否发生了移动（布尔）
  bringOnscreen(skinId) {
    return invoke('bring_skin_onscreen', { skinId });
  },

  setSize(skinId, width, height) {
    return invoke('set_skin_size', { skinId, width, height });
  },

  setSkinCustomSetting(skinId, key, value) {
    return invoke('set_skin_custom_setting', { skinId, key, value });
  },

  // 皮肤设置页 file/directory 控件：管理器弹系统选择器；mode = 'file' | 'directory'，
  // filters 为扩展名列表（不含点，仅 file 用）；取消返回 null
  pickPath(mode, filters) {
    return invoke('pick_path', { mode, filters: filters && filters.length ? filters : null });
  },

  // 重置皮肤全部持久化数据（窗口配置 + 自定义设置），恢复 skin.json 默认值
  resetSkinConfig(skinId) {
    return invoke('reset_skin_config', { skinId });
  },

  // 皮肤包安装（.dskin）
  pickSkinPackage() {
    return invoke('pick_skin_package');
  },

  /** 把已安装皮肤文件夹打成 .dskin：返回保存路径；用户取消返回 null */
  packageSkin(skinId) {
    return invoke('package_skin', { skinId });
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

  // 管理器显隐单个皮肤（编辑器「隐藏/显示皮肤」与组批量操作共用）：
  // visible=false 隐藏窗口，true 显示（后端只显示不抢焦点）
  setSkinVisibility(skinId, visible) {
    return invoke('set_skin_visibility', { skinId, visible });
  },

  // 皮肤分组整体写：前端持有完整状态，组操作后整体回写
  //（groups = [{id,name,collapsed}] 有序数组；map = {皮肤id: 组id}）
  setSkinGroups(groups, map) {
    return invoke('set_skin_groups', { groups, map });
  },

  // 布局方案：捕获当前桌面为命名方案（overwriteId 给定时覆盖既有方案）
  captureLayout(name, overwriteId) {
    return invoke('capture_layout', { name, overwriteId: overwriteId ?? null });
  },

  // 应用布局方案，返回 {applied: number, skipped: string[]}
  applyLayout(layoutId) {
    return invoke('apply_layout', { layoutId });
  },

  // 布局方案整体写（重命名/删除/排序统一入口）
  setLayouts(layouts) {
    return invoke('set_layouts', { layouts });
  },

  // 皮肤多开：克隆为新 id + 新名称的独立皮肤，返回新皮肤 SkinInfo
  duplicateSkin(skinId) {
    return invoke('duplicate_skin', { skinId });
  },

  // 从源同步副本：用源文件夹重放副本（设置值与窗口配置保留），返回源当前版本号
  syncSkinCopy(skinId) {
    return invoke('sync_skin_copy', { skinId });
  },

  // 全局快捷键（空串 = 禁用）
  setHotkey(hotkey) {
    return invoke('set_hotkey', { hotkey });
  },

  // 皮肤专属显隐热键（空串 = 清除）：切换该皮肤窗口显隐
  setSkinHotkey(skinId, hotkey) {
    return invoke('set_skin_hotkey', { skinId, hotkey });
  },

  // 启动时快捷键注册失败的组合（消费型，只取到一次）
  takeHotkeyError() {
    return invoke('take_hotkey_error');
  },

  // 布局备份：导出 config/ + skins/ 为 zip（返回保存路径，取消为 null）；
  // 导入分两段——inspectBackup 选包并返回包内皮肤清单与权限声明（取消为
  // null），importConfig(path) 在审查确认后执行导入
  exportConfig() {
    return invoke('export_config');
  },

  inspectBackup() {
    return invoke('inspect_backup');
  },

  // skinIds 为空数组/null = 全量替换式导入；非空 = 选择性合并导入
  //（只导入勾选的 skin id）。返回 {imported: string[], skipped: string[]}
  importConfig(path, skinIds) {
    return invoke('import_config', { path, skinIds: skinIds && skinIds.length ? skinIds : null });
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

  // 自动下载新版安装包（完成才 resolve 出路径；网络失败 reject——
  // 前端降级为「前往下载」）
  downloadUpdate(url, version) {
    return invoke('download_update', { url, version });
  },

  // 立即安装：启动已下载的安装包并整站退出
  installUpdate() {
    return invoke('install_update');
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
