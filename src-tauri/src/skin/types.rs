use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Skin manifest — read from skin.json, never written back
///
/// 注意：tools/pack-skin/src/main.rs 手工镜像了本结构（校验所需字段）、
/// SkinSettingKind/SkinSettingDef/WindowDefaults 与安全上限——改动必须同步，
/// 并重新构建 tools/pack-skin.exe（打包放行 ≠ 安装放行 的漂移即源于此失守）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinManifest {
    /// 皮肤唯一 ID（小写字母/数字/中划线）。打包分发（.dskin）时必填，
    /// 决定安装文件夹名与用户数据的归属键；文件夹直装缺省时按文件夹名派生。
    #[serde(default)]
    pub id: Option<String>,
    /// 中文皮肤名：界面中文时优先显示；为空/缺省时回退 name_en。对称命名
    /// 一改前的旧字段名 `name` 经 serde alias 继续被接受——存量皮肤零迁移
    /// （两个名字同时写会解析报 duplicate field，创作者二选一）。
    /// 单语言英文皮肤可不写本字段、只填 name_en（选取规则见 display_name）
    #[serde(default, alias = "name")]
    pub name_zh: Option<String>,
    /// 英文皮肤名：界面英文时优先显示；界面中文且 name_zh 缺失/为空时回退
    /// 到它——单语言皮肤（只填一种语言）在两种界面语言下都显示创作者提供
    /// 的语言
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// 最低宿主版本要求（可选，形如 "1.0.5"）：宿主版本低于它时安装向导
    /// 提示「部分功能可能不可用」（不拦截安装）；皮肤运行期可经桥烘焙的
    /// __DESK_PP__.hostVersion 自行探测降级。
    #[serde(default)]
    pub min_host_version: Option<String>,
    /// 中文简介（旧字段名 `description` 经 alias 继续被接受，同 name_zh）
    #[serde(default, alias = "description")]
    pub description_zh: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    #[serde(default)]
    pub description_en: Option<String>,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub window: WindowDefaults,
    /// 敏感能力声明（12 种："registry" / "shell" / "system" / "clipboard" /
    /// "mic" / "file_system" / "control" / "media" / "notify" / "sys_info" /
    /// "network" / "open_link"，对应 skin_api 的 PERM_* 常量）。皮肤调用对应的
    /// 后端命令前必须在此声明，否则后端拒绝并返回 PermissionDenied。
    /// 未知名一律忽略；皮肤自身目录内的文件读写（沙箱隔离）与自己 schema 的
    /// 设置读写等免权限基线不需要声明。
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Declarative custom settings; the manager renders one control per entry
    #[serde(default)]
    pub settings: Vec<SkinSettingDef>,
}

impl SkinManifest {
    /// 显示名/简介选取：界面语言优先取对应语言字段，缺失（None 或空串）
    /// 回退另一语言——单语言皮肤（只填 name_zh 或只填 name_en）在中/英
    /// 界面下都显示创作者提供的那种语言；两个都填 = 双语皮肤随界面切换。
    /// 任何字段组合都是合法状态，无声明开关（前端 dom.js dispName/dispDesc
    /// 同款规则，两边勿漂移）。全缺归一为空串，调用方不用二次判空。
    pub fn display_name(&self, lang: &str) -> String {
        pick_skin_text(lang, self.name_zh.as_deref(), self.name_en.as_deref())
    }

    pub fn display_description(&self, lang: &str) -> Option<String> {
        let d = pick_skin_text(
            lang,
            self.description_zh.as_deref(),
            self.description_en.as_deref(),
        );
        // 简介是全空时不下发（前端按 falsy 决定渲染不渲染）
        if d.is_empty() { None } else { Some(d) }
    }
}

/// 皮肤文案选取器（display_name/display_description 共用的单向回退链）：
/// 英文界面 en 优先回退 zh；中文界面 zh 优先回退 en。
fn pick_skin_text(lang: &str, zh: Option<&str>, en: Option<&str>) -> String {
    let zh = zh.filter(|s| !s.is_empty());
    let en = en.filter(|s| !s.is_empty());
    let pick = if lang == "en" { en.or(zh) } else { zh.or(en) };
    pick.unwrap_or("").to_string()
}

/// One option of a "select" setting.  `label_zh` falls back to `value` in the UI
/// (旧字段名 `label` 经 alias 继续被接受，同 SkinManifest::name_zh).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinSettingOption {
    pub value: String,
    #[serde(default, alias = "label")]
    pub label_zh: Option<String>,
    /// 英文显示名：界面英文时优先显示；缺失/为空时回退 label_zh（单语言
    /// 皮肤只填一种语言即可，见 SkinManifest::display_name 的选取规则）
    #[serde(default)]
    pub label_en: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkinSettingKind {
    /// 单选开关
    Boolean,
    /// 数字输入（min/max/step）
    Number,
    /// 数字步进器（−/＋ 按钮按 step 增减，min/max 可选夹取）
    Stepper,
    /// 短文本
    Text,
    /// 长文本（多行）
    LongText,
    /// 24 小时时间，值 "HH:MM" 或 "HH:MM:SS"
    Time,
    /// 日期，值 "YYYY-MM-DD"
    Date,
    /// 调色板（预设色块 + 取色器 + 透明度），值 "#rrggbb" 或 "#rrggbbaa"
    Palette,
    /// 下拉选择
    Select,
    /// 多选开关组，值 = options 子集数组
    MultiSelect,
    /// 互斥开关组，值 = options 之一
    Radio,
    /// 星期多选，值 = ["mon","wed",...]（固定周一至周日）
    Weekdays,
    /// 系统字体选择，值 = 字体族名字符串
    Font,
    /// 通用滑动条（min/max/step，缺省 0/100/1）
    Slider,
    /// 时间范围，值 {"start": "YYYY-MM-DD HH:MM:SS", "end": "..."}，空串 = 未设
    TimeRange,
    /// 任务列表，值 = 字符串数组（上限 500 条，不对用户暴露）
    TaskList,
    /// 待办任务列表，值 = [{"text": "...", "done": bool}]（上限同 TaskList）
    TodoList,
    /// 日期时间（单点），值 "YYYY-MM-DD HH:MM:SS"，空串 = 未设
    DateTime,
    /// 密码输入（掩码显示），值 = 字符串（≤256 字符，存储同 text）
    Password,
    /// 日期任务列表，值 = [{"time": "YYYY-MM-DD HH:MM:SS", "text": "..."}]
    DateTaskList,
    /// 文件选择器（管理器弹系统打开对话框），值 = 绝对路径字符串，空串 = 未选
    File,
    /// 文件夹选择器，值 = 绝对路径字符串，空串 = 未选
    Directory,
    /// GPU 适配器选择器（管理器运行时枚举本机 GPU 生成下拉项），
    /// 值 = 适配器 LUID 字符串（"0xHHHHHHHH_0xLLLLLLLL"），空串 = 首项（自动）
    #[serde(rename = "gpu_adapter")]
    // 显式 rename：lowercase 规则会生成 gpuadapter，保留下划线名
    GpuAdapter,
}

/// A custom setting declared by the skin author in skin.json "settings".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinSettingDef {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: SkinSettingKind,
    #[serde(default, alias = "label")]
    pub label_zh: Option<String>,
    /// 英文控件标题：界面英文时优先显示；缺失/为空时回退 label_zh（选取
    /// 规则同 SkinManifest::display_name）
    #[serde(default)]
    pub label_en: Option<String>,
    /// 控件下方的说明文字，展示在配置面板（旧字段名 `description` 经
    /// alias 继续被接受）
    #[serde(default, alias = "description")]
    pub description_zh: Option<String>,
    /// 英文说明文字（同 label_en 的选取规则）
    #[serde(default)]
    pub description_en: Option<String>,
    /// 分组名：同组控件在「皮肤设置」页归为一张卡片（旧字段名 `group`
    /// 经 alias 继续被接受）
    #[serde(default, alias = "group")]
    pub group_zh: Option<String>,
    /// 英文分组名（同 label_en 的选取规则）
    #[serde(default)]
    pub group_en: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Number / stepper settings only: inclusive bounds applied on save.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Number / slider / stepper settings only: step between values (default 1).
    #[serde(default)]
    pub step: Option<f64>,
    /// Select / multiselect / radio / palette settings only.
    #[serde(default)]
    pub options: Vec<SkinSettingOption>,
    /// file 选择器专用：允许的扩展名列表（不含点，如 ["png","jpg"]）；
    /// 空数组 = 不过滤。directory 与其他控件忽略此字段
    #[serde(default)]
    pub filters: Vec<String>,
}

fn default_entry() -> String {
    "index.html".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowDefaults {
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
    #[serde(default = "default_true")]
    pub transparent: bool,
    /// "always_on_top" and "on_desktop" are mutually exclusive and exactly
    /// one is always on.  The default placement is on-desktop.
    #[serde(default)]
    pub always_on_top: bool,
    #[serde(default = "default_true")]
    pub on_desktop: bool,
    #[serde(default)]
    pub resizable: bool,
    /// 缩放比例默认值（0.5–2.0）；实际窗口 = 基础尺寸 × zoom，内容同倍
    /// 缩放。「窗口」页可覆盖。默认必须回 1.0——落 0.0 会把窗口乘没。
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    /// 网页皮肤（entry 为 http(s) URL）的自动刷新间隔（秒）；0/缺省 = 不自动
    /// 刷新。仅网页皮肤有意义（本地皮肤的内容刷新走右键/热重载）。
    #[serde(default)]
    pub refresh_seconds: Option<u32>,
    /// 边缘吸附默认值：拖动窗口靠近屏幕边缘或其他皮肤窗口边缘时自动对齐
    ///（屏幕边缘优先）。仅作用于交互式拖动。「窗口」页可覆盖。
    #[serde(default)]
    pub edge_snap: bool,
    /// 吸附间距默认值（逻辑像素）：吸附后与屏幕边缘/其他窗口之间保留的空隙。
    #[serde(default)]
    pub snap_gap: u32,
}

/// entry 是否是网页皮肤入口（http/https URL）：网页皮肤的窗口直接加载
/// 站点页面，不走 skin:// 协议与桥注入。
pub(crate) fn is_url_entry(entry: &str) -> bool {
    let e = entry.trim_start();
    e.starts_with("https://") || e.starts_with("http://")
}

/// 窗口尺寸/不透明度钳制的单一锚点（审查 G7：字面量曾散落 commands/
/// factory/loader/protocol 多处——改阈值只许动这里；pack-skin 侧为字面量
/// 镜像，由 tools/check-pack-skin-mirror.py 对拍盯守）。
pub const MAX_DIMENSION: u32 = 10000;
pub const MIN_OPACITY: f64 = 0.1;

/// 缩放比例上下限（「窗口」页滑块同范围）。与 MAX_DIMENSION/MIN_OPACITY
/// 同层收口（2026-09 审查：此前住 commands.rs，下层 loader.rs 反向引用
/// 顶层——依赖方向违反）。
pub const MIN_ZOOM: f64 = 0.5;
pub const MAX_ZOOM: f64 = 2.0;

/// 把缩放比例钳制到支持范围；NaN/无穷回落 1.0。
pub fn clamp_zoom(z: f64) -> f64 {
    if z.is_finite() {
        z.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// 皮肤窗口入场/出场淡入淡出时长（毫秒，入场略慢于出场）。单一锚点：
/// 桥 CSS 烘焙（protocol.rs bridge_css 的 deskFadeIn）、运行时 eval
///（window/factory.rs fade_in_js/fade_out_js）、网页皮肤初始化脚本
///（web_fade_in_script）、销毁前阻塞等待（fade_out_for_destroy）四处
/// 共用——销毁等待必须覆盖动画时长，字面量散落会改一处漏其余
///（2026-09 审查 B 面）。
pub const FADE_IN_MS: u64 = 180;
pub const FADE_OUT_MS: u64 = 150;

fn default_width() -> u32 {
    300
}
fn default_height() -> u32 {
    200
}
fn default_opacity() -> f64 {
    1.0
}
fn default_zoom() -> f64 {
    1.0
}
fn default_true() -> bool {
    true
}

/// Full runtime representation of a skin
#[derive(Debug, Clone, Serialize)]
pub struct Skin {
    pub id: String,
    pub manifest: SkinManifest,
    pub directory: PathBuf,
    /// 副本来源标记（skin.json 的 `x-driftlet-origin`，形如 "<源id>@<创建时源版本>"）。
    /// loader 单独从 manifest 文本提取——不放进 SkinManifest（它在
    /// pack-skin 有手工镜像，不为副本元数据动镜像）。非副本皮肤为 None。
    pub origin: Option<String>,
}

/// Lightweight info sent to frontend for listing
#[derive(Debug, Clone, Serialize)]
pub struct SkinInfo {
    pub id: String,
    /// 中文皮肤名（manifest 的 name_zh——旧字段名 name 经 alias 解析进这里；
    /// 前端 dom.js dispName 按界面语言选取，缺失/为空回退 name_en）
    pub name_zh: String,
    /// 英文皮肤名（同 name_zh 的选取规则）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    /// 中文简介（manifest 的 description_zh，旧字段名 description 经 alias）
    pub description_zh: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    pub loaded: bool,
    /// 已加载但窗口当前不可见（全局快捷键/托盘/Alt+F4 隐藏）——真实窗口
    /// 状态（IsWindowVisible），不是「按没按过热键」的簿记
    pub hidden: bool,
    /// Absolute path to preview image (preview.png / preview.jpg) if present
    pub preview: Option<String>,
}

/// Full detail for config panel
#[derive(Debug, Clone, Serialize)]
pub struct SkinDetail {
    pub id: String,
    /// 同 SkinInfo.name_zh 的口径
    pub name_zh: String,
    /// 英文皮肤名（同 SkinInfo.name_en 的选取规则）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    /// 同 SkinInfo.description_zh 的口径
    pub description_zh: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    pub directory: String,
    pub loaded: bool,
    /// 同 SkinInfo.hidden：已加载但窗口当前不可见（真实窗口状态）
    pub hidden: bool,
    /// skin.json 声明的敏感权限（原样透传，前端按分级展示）
    pub permissions: Vec<String>,
    pub config: SkinRuntimeConfig,
    /// Custom settings schema declared in skin.json
    pub settings_schema: Vec<SkinSettingDef>,
    /// Effective custom setting values (schema defaults merged with persisted
    /// overrides), keyed by setting key
    pub settings_values: serde_json::Map<String, serde_json::Value>,
    /// 副本来源标记原串（"<源id>@<创建时源版本>"；非副本为 None）
    pub origin: Option<String>,
    /// 源皮肤显示名（供「副本 · 源自 X」展示；源已删除为 None）
    pub origin_name: Option<String>,
    /// 源当前版本号——仅当与 origin 记录版本不同时为 Some（= 可同步）；
    /// 源不存在或与记录一致时为 None
    pub origin_update: Option<String>,
}

/// Per-skin runtime configuration (persisted)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinRuntimeConfig {
    pub opacity: f64,
    pub always_on_top: bool,
    pub on_desktop: bool,
    /// **已移除（deprecated）**：壁纸层功能已随本版本删除（维护成本定论，
    /// 见 docs/关键机制.md「壁纸层（已移除）」）。此字段仅为 serde 读取
    /// 旧 config 并交由 `normalize_mode_flags` 迁移为贴桌面而保留——任何
    /// 新代码不得使用；迁移后保存即恒为 false。
    #[serde(default)]
    #[deprecated = "壁纸层已移除；仅为读取并迁移旧配置保留，勿用于新代码"]
    pub wallpaper_layer: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub position_locked: bool,
    /// 拖拽边框缩放开关：None = 跟随 skin.json 的 window.resizable（老配置
    /// 升级后字段缺失即 None）；Some(v) = 用户在「窗口」页的显式选择。
    #[serde(default)]
    pub resizable: Option<bool>,
    /// 缩放比例：None = 跟随 skin.json 的 window.zoom（老配置缺失即 None）；
    /// Some(v) = 用户在「窗口」页的显式选择（0.5–2.0）。实际窗口尺寸 =
    /// 基础尺寸 × zoom，内容经 WebView2 ZoomFactor 同倍缩放。
    #[serde(default)]
    pub zoom: Option<f64>,
    /// 鼠标穿透开关：开启后皮肤窗口不再响应鼠标（点击直达下层窗口/桌面），
    /// 交互（拖动、点按）只能先回管理器关闭。实现机制见 docs/关键机制.md
    /// 「鼠标穿透」——tao 的 set_ignore_cursor_events 给顶层窗口置
    /// WS_EX_TRANSPARENT|WS_EX_LAYERED，无边框子类按 HWND 登记放行这两位。
    #[serde(default)]
    pub click_through: bool,
    /// 边缘吸附开关：None = 跟随 skin.json 的 window.edge_snap（老配置
    /// 升级后字段缺失即 None）；Some(v) = 用户在「窗口」页/右键快捷段的
    /// 显式选择。（与 resizable/zoom 同一 Option 跟随模式——审查 F2-A：
    /// 裸 bool 让旧持久化条目永不跟随 manifest 新声明）
    #[serde(default)]
    pub edge_snap: Option<bool>,
    /// 吸附间距（逻辑像素）：None = 跟随 manifest 默认；Some(v) = 用户显式选择。
    #[serde(default)]
    pub snap_gap: Option<u32>,
    /// 皮肤专属显隐热键（"" = 未设置）：按下切换该皮肤窗口显隐——
    /// 注册表常驻（hotkey.rs 的 SKIN_HOTKEYS），皮肤未加载时按下静默
    /// 无效果（无窗可切），不需加载/卸载钩子。
    #[serde(default)]
    pub hotkey: String,
    /// 专注模式白名单（豁免）：专注模式进入时不隐藏/不卸载该皮肤。
    /// 专注模式面板集中管理（docs/proposals/专注模式方案-2026-09.md）。
    #[serde(default)]
    pub focus_exempt: bool,
    // 「皮肤设置」页的用户值存在皮肤文件夹的 settings.json 里，不在此结构
    // （旧配置的 custom 键由 v1→v2 迁移处理，serde 读入时自动忽略）。
}

impl SkinRuntimeConfig {
    /// 按 skin.json 的 window 默认值构造（尚未持久化过配置的皮肤）：
    /// 位置 x/y 与用户覆盖项（resizable/zoom 等）保持未设，跟随默认值。
    #[allow(deprecated)]
    pub fn from_manifest(manifest: &SkinManifest) -> Self {
        Self {
            opacity: manifest.window.opacity,
            always_on_top: manifest.window.always_on_top,
            on_desktop: manifest.window.on_desktop,
            wallpaper_layer: false,
            x: None,
            y: None,
            width: manifest.window.width,
            height: manifest.window.height,
            position_locked: false,
            resizable: None,
            zoom: None,
            click_through: false,
            // 边缘吸附：None = 跟随 manifest（皮肤可声明开启，如屿族六张）
            edge_snap: None,
            snap_gap: None,
            hotkey: String::new(),
            focus_exempt: false,
        }
    }
}

impl Default for SkinRuntimeConfig {
    #[allow(deprecated)]
    fn default() -> Self {
        Self {
            opacity: 1.0,
            // Mutually exclusive with on_desktop; the default placement is
            // on-desktop, so a fresh skin starts pinned, not topmost.
            always_on_top: false,
            on_desktop: true,
            wallpaper_layer: false,
            x: None,
            y: None,
            width: 300,
            height: 200,
            position_locked: false,
            resizable: None,
            zoom: None,
            click_through: false,
            edge_snap: None,
            snap_gap: None,
            hotkey: String::new(),
            focus_exempt: false,
        }
    }
}

/// Global application config persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub loaded_skins: Vec<String>,
    pub skin_settings: HashMap<String, SkinRuntimeConfig>,
    /// Launch on system startup
    #[serde(default)]
    pub autostart: bool,
    /// UI theme: "auto", "light", or "dark"
    #[serde(default = "default_theme")]
    pub theme: String,
    /// UI language: "zh-CN" or "en"
    #[serde(default = "default_language")]
    pub language: String,
    /// 皮肤分组（有序数组 = 显示顺序）。未在 skin_group_map 中出现的
    /// 皮肤归属内置虚拟组「未分组」（不落盘、恒在末尾）。
    #[serde(default)]
    pub skin_groups: Vec<SkinGroup>,
    /// 皮肤 id → 组 id 的归属表；指向已删除组的条目在保存时归一剔除
    #[serde(default)]
    pub skin_group_map: HashMap<String, String>,
    /// 布局方案（有序数组 = 显示顺序）：命名的「桌面布置」快照——
    /// 加载集 + 各皮肤几何（位置/尺寸）+ 显隐。皮肤行为配置
    ///（透明度/缩放/层级等）不属于布局，仍归 skin_settings 各皮肤自管。
    #[serde(default)]
    pub layouts: Vec<LayoutPreset>,
    /// Global hotkey that hides/shows all skin windows ("Ctrl+Alt+D"
    /// style; empty string = disabled).
    #[serde(default = "default_hotkey_toggle_skins")]
    pub hotkey_toggle_skins: String,
    /// Skin hot reload while developing (debug builds only — release builds
    /// never start the watcher).  Default off; skin authors enable it in the
    /// settings panel while developing.
    #[serde(default = "default_hot_reload")]
    pub hot_reload: bool,
    /// 启动时自动检测 GitHub 新版本（默认开）。设置页可关；更新弹窗里勾选
    /// 「不再提示更新」后取消也会关掉它。
    #[serde(default = "default_update_check")]
    pub update_check: bool,
    /// 允许提权运行（持久放行标记）：降级失败/降无可降（真 Administrator）
    /// 时用户在明示框选「继续」后写入——后续启动直接跳过降级不再提示
    ///（elevation.rs 的 should_demote 读它）。
    #[serde(default)]
    pub allow_elevated: bool,
    /// WebView2 地板提醒已弹过（一次性标记；此后由标题栏黄色徽标常驻
    /// 提示——webview2.rs 的 runtime_floor_notice / get_titlebar_warnings）。
    /// 必须是 AppConfig 字段而非游离键：config 保存按结构体重写整个文件，
    /// 游离键会在下一次保存时被冲掉（allow_elevated 同款教训）。
    #[serde(default)]
    pub webview2_floor_noticed: bool,
    /// 标题栏警告徽标总开关（默认开）：红 = 提权运行（在前），黄 =
    /// WebView2 运行时低于渲染地板（在后）。设置页「通用」可关。
    #[serde(default = "default_titlebar_warnings")]
    pub titlebar_warnings: bool,
    /// 内置族皮肤首装种子已落（安装包打包的 isles-* 已装进 skins 目录并归入
    /// 「默认皮肤」组）：一次性标记——用户删过的皮肤不复活、组被删过不重建。
    #[serde(default)]
    pub bundled_skins_seeded: bool,
    /// 专注模式动作偏好（"hide" | "unload"）：隐藏档秒回不回收内存；卸载档
    /// 回收内存但运行时状态丢失。默认隐藏（老用户的热键习惯无感）。
    #[serde(default = "default_focus_mode_action")]
    pub focus_mode_action: String,
    /// 全屏时自动进入专注模式（默认关——升级用户不被打扰）。
    #[serde(default)]
    pub focus_mode_auto_fullscreen: bool,
    /// 专注模式态（config.json 持久化，崩溃安全）：模式激活期间应用退出/
    /// 崩溃后下次启动按快照动作恢复并清除（隐藏档无需恢复动作——隐藏不落盘
    /// loaded_skins，重启后照常全部加载显示）。启动 = 全新桌面，不带着模式复活。
    #[serde(default)]
    pub focus_mode: FocusModeState,
}

fn default_hot_reload() -> bool {
    false
}

fn default_update_check() -> bool {
    true
}

fn default_titlebar_warnings() -> bool {
    true
}

/// 专注模式默认动作档：隐藏（老用户的热键体验与旧版逐字节一致）。
fn default_focus_mode_action() -> String {
    "hide".to_string()
}

/// 专注模式态（config.json 持久化，崩溃安全）：active 标志 + 进入时生效的
/// 动作档 + 受影响皮肤快照。用户偏好档在 AppConfig.focus_mode_action；模式
/// 期间改档不影响本次模式的还原口径（退出按进入档还原）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FocusModeState {
    #[serde(default)]
    pub active: bool,
    #[serde(default = "default_focus_mode_action")]
    pub action: String,
    #[serde(default)]
    pub snapshot: Vec<String>,
}

/// Default toggle-visibility hotkey. Rarely taken by other apps; users can
/// rebind or clear it in the settings panel.
fn default_hotkey_toggle_skins() -> String {
    "Ctrl+Shift+Alt+D".to_string()
}

fn default_theme() -> String {
    "auto".to_string()
}

/// 皮肤分组。`collapsed` 持久化折叠态；组顺序 = AppConfig.skin_groups 数组序
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
}

/// 布局方案（命名的桌面布置快照）：加载集 + 几何 + 显隐。
/// 数组序 = 面板/托盘菜单的显示顺序。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutPreset {
    pub id: String,
    pub name: String,
    /// 皮肤 id → 布置快照
    #[serde(default)]
    pub skins: HashMap<String, LayoutSkin>,
}

/// 布局里单个皮肤的布置快照：位置（None = 未记录位置，应用时居中/默认）、
/// 基础尺寸（100% 基准；应用时按当前有效 zoom 折算实际尺寸）与显隐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutSkin {
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default = "default_layout_visible")]
    pub visible: bool,
}

fn default_layout_visible() -> bool {
    true
}

/// 首次启动的默认语言（随 OS UI 语言）。pub(crate)：elevation.rs 的
/// 提权降权失败提示框在 AppState 建立之前也要它兜底语言。
pub(crate) fn default_language() -> String {
    // First-run default follows the OS UI language: Chinese systems get
    // "zh-CN", everything else "en" — mirroring the NSIS installer's
    // automatic language selection (zh → SimpChinese, fallback English)
    // so the app starts in the same language the installer ran in.
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        // LANGID primary language id = low 10 bits; LANG_CHINESE = 0x04
        let primary = unsafe { GetUserDefaultUILanguage() } & 0x3FF;
        if primary == 0x04 { "zh-CN" } else { "en" }.to_string()
    }
    #[cfg(not(windows))]
    "en".to_string() // 非 Windows 无 UI 语言探测，按「everything else en」
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 2,
            loaded_skins: Vec::new(),
            skin_settings: HashMap::new(),
            autostart: false,
            theme: "auto".to_string(),
            language: default_language(),
            skin_groups: Vec::new(),
            skin_group_map: HashMap::new(),
            layouts: Vec::new(),
            hotkey_toggle_skins: default_hotkey_toggle_skins(),
            hot_reload: default_hot_reload(),
            update_check: default_update_check(),
            allow_elevated: false,
            webview2_floor_noticed: false,
            titlebar_warnings: default_titlebar_warnings(),
            bundled_skins_seeded: false,
            focus_mode_action: default_focus_mode_action(),
            focus_mode_auto_fullscreen: false,
            focus_mode: FocusModeState::default(),
        }
    }
}
