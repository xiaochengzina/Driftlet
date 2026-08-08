use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Skin manifest — read from skin.json, never written back
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinManifest {
    /// 皮肤唯一 ID（小写字母/数字/中划线）。打包分发（.dskin）时必填，
    /// 决定安装文件夹名与用户数据的归属键；文件夹直装缺省时按文件夹名派生。
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// 英文皮肤名（bilingual 皮肤专用；留空时英文界面回退 name）
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 英文简介（bilingual 皮肤专用；留空回退 description）
    #[serde(default)]
    pub description_en: Option<String>,
    /// 中英双语声明（作者侧开关，非用户选项）：true = 皮肤为「皮肤设置」页
    /// 文案提供了英文（各 *_en 字段），管理器语言为英文时优先显示英文；
    /// false/缺省 = 单语皮肤，所有 *_en 字段一律忽略
    #[serde(default)]
    pub bilingual: bool,
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub window: WindowDefaults,
    /// 敏感能力声明（"files" / "registry" / "shell" / "system" / "clipboard"
    /// / "mic"，对应 skin_api 的 PERM_* 常量）。皮肤调用对应的后端命令前
    /// 必须在此声明，否则后端拒绝并返回 PermissionDenied。
    /// 未知名一律忽略；只读系统信息命令不需要声明。
    #[serde(default)]
    pub permissions: Vec<String>,
    /// Declarative custom settings; the manager renders one control per entry
    #[serde(default)]
    pub settings: Vec<SkinSettingDef>,
}

/// One option of a "select" setting.  `label` falls back to `value` in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinSettingOption {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
    /// 英文显示名（bilingual 皮肤专用；留空时英文界面回退 label）
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
}

/// A custom setting declared by the skin author in skin.json "settings".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkinSettingDef {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: SkinSettingKind,
    #[serde(default)]
    pub label: Option<String>,
    /// 英文控件标题（bilingual 皮肤专用；留空时英文界面回退 label）
    #[serde(default)]
    pub label_en: Option<String>,
    /// 控件下方的说明文字，展示在配置面板
    #[serde(default)]
    pub description: Option<String>,
    /// 英文说明文字（留空回退 description）
    #[serde(default)]
    pub description_en: Option<String>,
    /// 分组名：同组控件在「皮肤设置」页归为一张卡片
    #[serde(default)]
    pub group: Option<String>,
    /// 英文分组名（留空回退 group）
    #[serde(default)]
    pub group_en: Option<String>,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Number settings only: inclusive bounds applied on save.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Number / slider settings only: step between values (default 1).
    #[serde(default)]
    pub step: Option<f64>,
    /// Select / multiselect / radio / palette settings only.
    #[serde(default)]
    pub options: Vec<SkinSettingOption>,
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
}

fn default_width() -> u32 { 300 }
fn default_height() -> u32 { 200 }
fn default_opacity() -> f64 { 1.0 }
fn default_zoom() -> f64 { 1.0 }
fn default_true() -> bool { true }

/// Full runtime representation of a skin
#[derive(Debug, Clone, Serialize)]
pub struct Skin {
    pub id: String,
    pub manifest: SkinManifest,
    pub directory: PathBuf,
}

/// Lightweight info sent to frontend for listing
#[derive(Debug, Clone, Serialize)]
pub struct SkinInfo {
    pub id: String,
    pub name: String,
    /// 英文皮肤名（bilingual 皮肤专用；前端按语言选取，留空回退 name）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    /// skin.json 声明的中英双语开关：决定前端是否启用 *_en 文案
    pub bilingual: bool,
    pub loaded: bool,
    pub has_error: bool,
    pub error_msg: Option<String>,
    /// Absolute path to preview image (preview.png / preview.jpg) if present
    pub preview: Option<String>,
}

/// Full detail for config panel
#[derive(Debug, Clone, Serialize)]
pub struct SkinDetail {
    pub id: String,
    pub name: String,
    /// 英文皮肤名（bilingual 皮肤专用；前端按语言选取，留空回退 name）
    pub name_en: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    pub description_en: Option<String>,
    /// skin.json 声明的中英双语开关：决定「皮肤设置」页是否启用 *_en 文案
    pub bilingual: bool,
    pub directory: String,
    pub loaded: bool,
    pub config: SkinRuntimeConfig,
    /// Custom settings schema declared in skin.json
    pub settings_schema: Vec<SkinSettingDef>,
    /// Effective custom setting values (schema defaults merged with persisted
    /// overrides), keyed by setting key
    pub settings_values: serde_json::Map<String, serde_json::Value>,
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
    /// 边缘吸附开关：拖动窗口靠近屏幕边缘或其他皮肤窗口边缘时自动对齐
    ///（屏幕边缘优先）。仅作用于交互式拖动（WM_MOVING），不影响面板
    /// 输入的精确坐标。
    #[serde(default)]
    pub edge_snap: bool,
    /// 吸附间距（逻辑像素）：吸附后与屏幕边缘/其他窗口之间保留的空隙。
    #[serde(default)]
    pub snap_gap: u32,
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
            edge_snap: false,
            snap_gap: 0,
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
            edge_snap: false,
            snap_gap: 0,
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
}

fn default_hot_reload() -> bool {
    false
}

fn default_update_check() -> bool {
    true
}

/// Default toggle-visibility hotkey. Rarely taken by other apps; users can
/// rebind or clear it in the settings panel.
fn default_hotkey_toggle_skins() -> String {
    "Ctrl+Shift+Alt+D".to_string()
}

fn default_theme() -> String {
    "auto".to_string()
}

fn default_language() -> String {
    // First-run default follows the OS UI language: Chinese systems get
    // "zh-CN", everything else "en" — mirroring the NSIS installer's
    // automatic language selection (zh → SimpChinese, fallback English)
    // so the app starts in the same language the installer ran in.
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::GetUserDefaultUILanguage;
        // LANGID primary language id = low 10 bits; LANG_CHINESE = 0x04
        let primary = unsafe { GetUserDefaultUILanguage() } & 0x3FF;
        if primary == 0x04 {
            "zh-CN"
        } else {
            "en"
        }
        .to_string()
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
            hotkey_toggle_skins: default_hotkey_toggle_skins(),
            hot_reload: default_hot_reload(),
            update_check: default_update_check(),
        }
    }
}
