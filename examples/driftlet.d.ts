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

// ── driftlet.js 封装本体 ──

declare const Driftlet: {
  /** 当前生效的设置值（桥缺失时为空对象） */
  readonly settings: Record<string, unknown>;
  /** 管理器界面语言（桥缺失时为 "zh-CN"） */
  readonly language: string;
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

  /** 管理器改了本皮肤的设置后触发；返回解绑函数 */
  onSettingChanged(fn: (key: string, value: unknown) => void): () => void;
  /** 管理器切换界面语言后触发；返回解绑函数 */
  onLanguageChanged(fn: (language: string) => void): () => void;
};
