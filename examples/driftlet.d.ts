/**
 * driftlet.d.ts — Driftlet 皮肤桥与 driftlet.js 封装的类型定义
 *
 * 用法（任选其一）：
 *   1. 把本文件放进皮肤文件夹，VS Code 等编辑器会自动加载（JS 文件即可获得补全）；
 *   2. 显式引用：在主 JS 文件首行加 `/// <reference path="driftlet.d.ts" />`。
 * 命令契约以《皮肤开发指南》§5 为准；此处类型与后端返回结构一一对应。
 */

/** 注入桥本体（window.driftlet 与 window.__DESK_PP__ 是同一对象） */
interface DriftletBridge {
  /** 当前生效的设置值；password 类型的键恒为空串（经 Driftlet.getSetting 读取） */
  settings: Record<string, unknown>;
  /** 管理器界面语言："zh-CN" / "en" */
  language: string;
  /** 管理器当前主题："light" / "dark"（auto 已由后端折算成具体值）；运行时切换派发 desk-theme-changed 事件 */
  theme: string;
  /** 宿主版本号，如 "1.0.5"（按数字段比较做能力探测） */
  hostVersion: string;
  /** 位置锁定状态（桥内部使用，只读参考） */
  positionLocked: boolean;
  /** 边框缩放开关状态（桥内部使用，只读参考） */
  resizable: boolean;
  /** 调用后端命令；失败 reject（值为可读错误文案，语言随管理器界面） */
  invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
  /** 桥内部使用，皮肤不要直接调用 */
  setOpacity(v: number): void;
  /** 桥内部使用，皮肤不要直接调用 */
  setResizable(on: boolean): void;
}

interface Window {
  /** 历史名称，永久兼容；新代码建议用 window.driftlet */
  __DESK_PP__?: DriftletBridge;
  /** 推荐入口（与 __DESK_PP__ 同一对象） */
  driftlet?: DriftletBridge;
}

// ── 命令返回结构（与后端 skin_api 的 Serialize 结构对应）──

interface DriftletCpuInfo {
  name: string;
  physical_cores: number;
  logical_cores: number;
  /** 当前频率（任务管理器「速度」同口径，turbo 时可超基准） */
  frequency_mhz: number;
  /** 总使用率 % */
  usage: number;
  usage_per_core: number[];
}

interface DriftletGpuInfo {
  name: string;
  gpu_type: 'discrete' | 'integrated' | string;
  usage: number;
  vram_total: number;
  vram_used: number;
  vram_usage_pct: number;
}

interface DriftletMemoryGroup {
  total: number;
  used: number;
  free: number;
  usage_pct: number;
  free_pct: number;
}

interface DriftletMemoryInfo {
  ram: DriftletMemoryGroup;
  swap: DriftletMemoryGroup;
  /** 虚拟内存（已提交）；仅 Windows 提供，其他平台为 null */
  commit: DriftletMemoryGroup | null;
}

interface DriftletDiskInfo {
  name: string;
  mount_point: string;
  fs: string;
  total: number;
  used: number;
  free: number;
  usage_pct: number;
  /** 字节/秒（首调为 0 基线；非盘符挂载点为 0） */
  read_bps: number;
  write_bps: number;
}

interface DriftletDiskSpace {
  total: number;
  used: number;
  free: number;
  usage_pct: number;
  free_pct: number;
}

interface DriftletNetworkAdapter {
  name: string;
  ips: string[];
  mac: string;
  upload_bps: number;
  download_bps: number;
}

interface DriftletNetworkInfo {
  adapters: DriftletNetworkAdapter[];
  local_ips: string[];
}

interface DriftletSpectrum {
  /** 各频段能量 0–1（30Hz–16kHz 对数分布） */
  bands: number[];
  /** 瞬时峰值音量 0–1 */
  peak: number;
}

interface DriftletOsInfo {
  os_name: string;
  os_version: string;
  build: number | null;
  is_windows_11: boolean;
  host_name: string;
  user_name: string;
  uptime_secs: number;
}

interface DriftletProcessEntry {
  pid: number;
  name: string;
  /** 整机占比 0–100（首调为 0 基线） */
  cpu: number;
  memory_bytes: number;
}

interface DriftletProcessList {
  total: number;
  processes: DriftletProcessEntry[];
}

interface DriftletVolumeInfo {
  /** 0–100 */
  volume_pct: number;
  muted: boolean;
}

interface DriftletMediaInfo {
  title: string;
  artist: string;
  album: string;
  status: 'playing' | 'paused' | 'stopped' | string;
  position_secs: number;
  duration_secs: number;
  cover_base64: string | null;
  cover_mime: string | null;
}

interface DriftletBatteryInfo {
  /** 台式机为 false，其余字段此时无意义 */
  has_battery: boolean;
  ac_online: boolean;
  charging: boolean;
  percent: number | null;
  secs_remaining: number | null;
}

interface DriftletForegroundWindowInfo {
  title: string;
  pid: number;
  process_name: string;
}

interface DriftletRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface DriftletMonitorInfo {
  name: string;
  /** 物理像素（与窗口配置的逻辑像素差一个 scale_factor） */
  rect: DriftletRect;
  work_area: DriftletRect;
  is_primary: boolean;
  scale_factor: number;
}

interface DriftletDirEntry {
  name: string;
  is_dir: boolean;
  size: number;
}

interface DriftletRegistryValue {
  kind: 'string' | 'expand_string' | 'multi_string' | 'dword' | 'qword' | 'binary' | string;
  value: unknown;
}

interface DriftletCommandOutput {
  /** 进程退出码（异常退出可能为负数） */
  code: number;
  /** 各截断 1MB；GBK 输出已自动转码 */
  stdout: string;
  stderr: string;
}

/** http_request 的返回结构（HTTP 错误状态 4xx/5xx 照返，不 reject） */
interface DriftletHttpResponse {
  status: number;
  /** 文本响应体（非法 UTF-8 以替换字符兜底）；binary: true 时为 base64 */
  body: string;
  /** 响应头（同名多头只保留首个值） */
  headers: Record<string, string>;
  /** 响应体超 4MB 被截断时为 true */
  truncated: boolean;
}

/** skin_list_skins 的条目结构 */
interface DriftletSkinListEntry {
  id: string;
  name: string;
  name_en?: string | null;
  version?: string | null;
  author?: string | null;
  loaded: boolean;
  hidden: boolean;
}

/** 皮肤窗口配置项（skin_get_window_config 的返回结构；skin_set_window_config 的 patch 键子集） */
interface DriftletWindowConfigInfo {
  /** 目标皮肤是否已加载 */
  loaded: boolean;
  /** 不透明度 0.1–1.0 */
  opacity: number;
  /** 层级二态：恰好一真 */
  always_on_top: boolean;
  on_desktop: boolean;
  click_through: boolean;
  position_locked: boolean;
  resizable: boolean;
  /** 缩放 0.5–2.0 */
  zoom: number;
  edge_snap: boolean;
  snap_gap: number;
  /** 配置坐标（逻辑像素；从未拖过为 null） */
  x: number | null;
  y: number | null;
  /** 所见实际尺寸 = 基础尺寸 × 有效 zoom（逻辑像素） */
  width: number;
  height: number;
}

// ── driftlet.js 封装本体 ──

declare const Driftlet: {
  /** 当前生效的设置值（桥缺失时为空对象） */
  readonly settings: Record<string, unknown>;
  /** 管理器界面语言（桥缺失时为 "zh-CN"） */
  readonly language: string;
  /** 管理器当前主题 "light" / "dark"（桥缺失时为 "light"） */
  readonly theme: string;
  /** 宿主版本号（桥缺失时为空串） */
  readonly hostVersion: string;

  getCpuInfo(): Promise<DriftletCpuInfo[]>;
  getGpuInfo(): Promise<DriftletGpuInfo[]>;
  getMemoryInfo(): Promise<DriftletMemoryInfo>;
  getDisksInfo(): Promise<DriftletDiskInfo[]>;
  getDiskSpace(path: string): Promise<DriftletDiskSpace>;
  getNetworkInfo(): Promise<DriftletNetworkInfo>;
  getAudioSpectrum(bands?: number): Promise<DriftletSpectrum>;
  getOsInfo(): Promise<DriftletOsInfo>;
  getProcesses(sort?: 'cpu' | 'memory', limit?: number): Promise<DriftletProcessList>;
  getVolume(): Promise<DriftletVolumeInfo>;
  /** 无播放会话时 resolve null（不是错误） */
  getMediaInfo(): Promise<DriftletMediaInfo | null>;
  getBatteryInfo(): Promise<DriftletBatteryInfo>;
  /** 距用户上次键鼠输入的毫秒数 */
  getIdleTime(): Promise<number>;
  getForegroundWindowInfo(): Promise<DriftletForegroundWindowInfo | null>;
  getMonitors(): Promise<DriftletMonitorInfo[]>;
  /** Windows 系统级主题（AppsUseLightTheme 注册表值）："light" / "dark" */
  getSystemTheme(): Promise<'light' | 'dark' | string>;

  /** 读文本；binary: true 时返回 base64（≤32MB） */
  readFile(path: string, binary?: boolean): Promise<string>;
  /** 写文本（子目录自动创建）；binary: true 时 data 为 base64（≤16MB） */
  writeFile(path: string, data: string, binary?: boolean): Promise<void>;
  listDir(path?: string): Promise<DriftletDirEntry[]>;
  /** 仅限文件，不能删目录 */
  deleteFile(path: string): Promise<void>;

  getSetting<T = unknown>(key: string): Promise<T>;
  setSetting(key: string, value: unknown): Promise<void>;
  /** 发一条消息到宿主日志窗口；level 只认 "warn" / "error"，缺省 info */
  log(message: string, level?: 'info' | 'warn' | 'error'): Promise<void>;
  /** 隐藏任意已加载皮肤的窗口：省略 id 或传自己 id 时免权限；指定其他皮肤需 control 权限（唤回 = 全局热键/托盘勾选） */
  hideSkin(skinId?: string): Promise<void>;
  /** 显示任意已加载皮肤的窗口（同上；只显示不抢焦点） */
  showSkin(skinId?: string): Promise<void>;
  /** 皮肤间广播（免权限）：所有已加载皮肤（含自己）的 desk-skin-message 都会收到 */
  broadcast(channel: string, payload: unknown): Promise<void>;

  /** 权限 registry */
  readRegistryValue(root: string, path: string, name: string): Promise<DriftletRegistryValue>;
  /** 权限 shell；timeoutMs 默认 30000，钳制 100–120000；超时杀进程并 reject */
  runCommand(command: string, args?: string[], timeoutMs?: number): Promise<DriftletCommandOutput>;
  /** 权限 system；0–100，越界钳制 */
  setVolume(volumePct: number): Promise<void>;
  /** 权限 system */
  setMute(muted: boolean): Promise<void>;
  /** 权限 system；无播放会话时 reject */
  mediaControl(action: 'play' | 'pause' | 'play_pause' | 'next' | 'previous'): Promise<boolean>;
  /** 权限 clipboard */
  readClipboardText(): Promise<string>;
  /** 权限 clipboard */
  writeClipboardText(text: string): Promise<void>;
  /** 权限 system；http(s)://、mailto: 或本机绝对路径（可执行文件/UNC 被拒） */
  openExternal(target: string): Promise<void>;
  /** 权限 system；title ≤64、body ≤256 字符（超长截断） */
  showNotification(title: string, body?: string): Promise<void>;
  /** 权限 mic；返回结构与 getAudioSpectrum 相同 */
  getMicSpectrum(bands?: number): Promise<DriftletSpectrum>;
  /** 权限 file_system（高危）：读取任意绝对路径的文本；binary: true 时返回 base64（≤32MB）。失败 reject 系统错误原文 */
  readAnyFile(path: string, binary?: boolean): Promise<string>;
  /** 权限 file_system（高危）：写入任意绝对路径（父目录自动创建）；binary: true 时 data 为 base64（≤16MB）。失败 reject 系统错误原文 */
  writeAnyFile(path: string, data: string, binary?: boolean): Promise<void>;
  /** 权限 file_system（高危）：列任意目录（目录项排前、名称排序；与 listDir 同款结构） */
  listAnyDir(path: string): Promise<DriftletDirEntry[]>;
  /** 权限 file_system（高危）：建任意目录（含多级，已存在视为成功） */
  createAnyDir(path: string): Promise<void>;
  /** 权限 file_system（高危）：删任意路径；目录默认只删空目录，整棵目录树须 recursive: true */
  deleteAnyPath(path: string, recursive?: boolean): Promise<void>;
  /** 权限 file_system（高危）：外部文件的引用 URL（__fs__ 端点，<img src> / CSS url() 直接用，不经 JS 内存） */
  fileUrl(path: string): string;
  /** 权限 control（中危）：枚举全部已安装皮肤（id/name/version/loaded/hidden）——跨皮肤操作的入口 */
  listSkins(): Promise<DriftletSkinListEntry[]>;
  /** 权限 control（中危）：读取窗口配置项；省略 id = 读自己（免权限） */
  getWindowConfig(skinId?: string): Promise<DriftletWindowConfigInfo>;
  /** 权限 control（中危）：patch 按键部分更新窗口配置项；未知键/类型不符整批拒绝。
      省略 id = 改自己——**全键免权限**；指定其他皮肤一律 control。
      opacity / x,y / width,height / position_locked / resizable 要求目标已加载（否则 reject 未加载） */
  setWindowConfig(skinId: string | undefined, patch: Partial<DriftletWindowConfigInfo> & { placement?: 'top' | 'desktop' }): Promise<void>;
  /** 加载皮肤；省略 id = 自己（免权限），指定他人需 control */
  loadSkin(skinId?: string): Promise<void>;
  /** 卸载皮肤；省略 id = 自己（免权限，fire-and-forget——发起窗口随即销毁，返回值不可依赖），指定他人需 control */
  unloadSkin(skinId?: string): Promise<void>;
  /** 重载皮肤；省略 id = 自己（免权限，fire-and-forget 同上），指定他人需 control */
  reloadSkin(skinId?: string): Promise<void>;
  /** 免权限：通用 HTTP 请求（仅 http/https；HTTP 4xx/5xx 照返状态与响应体，网络层失败才 reject） */
  httpRequest(url: string, opts?: {
    method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE' | 'HEAD';
    headers?: Record<string, string>;
    /** 文本负载；binary: true 时按 base64 解码发送 */
    body?: string;
    /** 默认 15000ms，钳制 1000–60000 */
    timeoutMs?: number;
    /** true：请求体按 base64 发送、响应体按 base64 返回（图片/字体等二进制不被毁损） */
    binary?: boolean;
  }): Promise<DriftletHttpResponse>;

  /** 管理器改了本皮肤的设置后触发；返回解绑函数 */
  onSettingChanged(fn: (key: string, value: unknown) => void): () => void;
  /** 管理器切换界面语言后触发；返回解绑函数 */
  onLanguageChanged(fn: (language: string) => void): () => void;
  /** 管理器切换主题后触发（theme 为折算值 "light" / "dark"）；返回解绑函数 */
  onThemeChanged(fn: (theme: string) => void): () => void;
  /** 皮肤经 broadcast 发来消息时触发；from 为发送方皮肤 id；返回解绑函数 */
  onSkinMessage(fn: (channel: string, payload: unknown, from: string) => void): () => void;
  /** 本皮肤的窗口配置被改时触发（管理器面板或 control 命令均可），fn(key, value)；返回解绑函数。
      拖拽/边框缩放引起的位置尺寸变化不派发（皮肤自己拖的） */
  onWindowConfigChanged(fn: (key: string, value: unknown) => void): () => void;
};
