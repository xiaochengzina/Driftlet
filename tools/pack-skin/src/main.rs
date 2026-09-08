//! pack-skin — Driftlet 皮肤打包工具
//!
//! 把皮肤文件夹打成 `<id>-<version>.dskin`（zip 格式）。
//! 打包前按与 Driftlet 管理器一致的规则校验 skin.json：
//! 用与安装端（src-tauri/src/skin/types.rs）相同的强类型 SkinManifest
//! 完整反序列化（settings[].type 非法、window.width 非数字等一律拒绝），
//! 再检查 id 合法（含 Windows 保留设备名）、入口文件名合法且存在。
//!
//! 用法：pack-skin <皮肤文件夹> [输出目录]

use serde::Deserialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// 与安装端（src-tauri/src/skin/package.rs）一致的安全上限：防恶意/损坏包耗尽磁盘
const MAX_PACKAGE_BYTES: u64 = 256 * 1024 * 1024; // 压缩包 256 MB
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024; // 解压后合计 1 GB
const MAX_FILES: usize = 10000;
/// 对齐安装端 loader.rs：skin.json 体积上限，超限即视为异常
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024; // 1 MB
/// 对齐安装端 package.rs：创作者自带预览图的尺寸上限（像素）。解码内存 =
/// 宽×高×4B 驻留管理器渲染进程，112px 高的卡片用不到巨型原图
const MAX_PREVIEW_DIMENSION: u32 = 1280;

/// loader.rs 认的预览文件名（与安装端 package.rs 同顺序）
const PREVIEW_FILE_NAMES: [&str; 3] = ["preview.png", "preview.jpg", "preview.jpeg"];

/// 只读图片头取尺寸（PNG 看 IHDR，JPEG 扫 SOF 段），不解码像素——
/// 与安装端 package.rs 的 image_dimensions 手工镜像，改动必须同步
fn image_dimensions(path: &Path) -> Option<(u32, u32)> {
    const PROBE_BYTES: u64 = 512 * 1024;
    let mut head = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(PROBE_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    png_dimensions(&head).or_else(|| jpeg_dimensions(&head))
}

fn png_dimensions(head: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if head.len() < 24 || head[..8] != SIG {
        return None;
    }
    if &head[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes(head[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(head[20..24].try_into().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

fn jpeg_dimensions(head: &[u8]) -> Option<(u32, u32)> {
    if head.len() < 4 || head[0] != 0xff || head[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < head.len() {
        if head[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = head[i + 1];
        i += 2;
        if marker == 0xd8 || marker == 0xd9 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0xda {
            return None;
        }
        if i + 2 > head.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([head[i], head[i + 1]]) as usize;
        if seg_len < 2 {
            return None;
        }
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            if i + 7 > head.len() {
                return None;
            }
            let h = u16::from_be_bytes([head[i + 3], head[i + 4]]) as u32;
            let w = u16::from_be_bytes([head[i + 5], head[i + 6]]) as u32;
            return (w > 0 && h > 0).then_some((w, h));
        }
        i += seg_len;
    }
    None
}

// ---------------------------------------------------------------
// 以下 serde 结构复制精简自安装端 src-tauri/src/skin/types.rs，
// 两边字段/默认值保持一致 —— 保证「打包放行 = 安装放行」。
// 安装端结构改动时这里要同步。
// ---------------------------------------------------------------

/// Skin manifest（对应安装端 SkinManifest，只保留校验所需字段；
/// 对称字段名 name_zh/name_en——旧无后缀名 name 经 serde alias 继续被接受，
/// 与安装端口径一致）
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SkinManifest {
    #[serde(default)]
    id: Option<String>,
    /// 中文皮肤名（旧字段名 name 经 alias 接受；单语言英文皮肤可省略本
    /// 字段只填 name_en）
    #[serde(default, alias = "name")]
    name_zh: Option<String>,
    /// 英文皮肤名（界面英文时优先显示；界面中文且 name_zh 为空时回退——
    /// 单语言皮肤只填一种语言即可，同安装端 SkinManifest 注释）
    #[serde(default)]
    name_en: Option<String>,
    #[serde(default)]
    author: Option<String>,
    /// 中文简介（旧字段名 description 经 alias 接受）
    #[serde(default, alias = "description")]
    description_zh: Option<String>,
    /// 英文简介（同 name_en 的选取规则）
    #[serde(default)]
    description_en: Option<String>,
    #[serde(default = "default_entry")]
    entry: String,
    #[serde(default)]
    version: Option<String>,
    /// 对应安装端 SkinManifest.min_host_version（格式校验见 validate 阶段）
    #[serde(default)]
    min_host_version: Option<String>,
    #[serde(default)]
    window: WindowDefaults,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    settings: Vec<SkinSettingDef>,
}

/// 对应安装端 SkinSettingOption
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SkinSettingOption {
    value: String,
    /// 旧字段名 label 经 alias 接受
    #[serde(default, alias = "label")]
    label_zh: Option<String>,
    #[serde(default)]
    label_en: Option<String>,
}

/// 对应安装端 SkinSettingKind：settings[].type 的合法取值
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SkinSettingKind {
    Boolean,
    Number,
    Stepper,
    Text,
    LongText,
    Time,
    Date,
    Palette,
    Select,
    MultiSelect,
    Radio,
    Weekdays,
    Font,
    Slider,
    TimeRange,
    TaskList,
    TodoList,
    DateTime,
    Password,
    DateTaskList,
    File,
    Directory,
    /// 对应安装端 GpuAdapter（GPU 适配器选择器，管理器运行时枚举生成选项）
    #[serde(rename = "gpu_adapter")]   // 同安装端：显式 rename 保下划线名
    GpuAdapter,
}

/// 对应安装端 SkinSettingDef
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SkinSettingDef {
    key: String,
    #[serde(rename = "type")]
    kind: SkinSettingKind,
    /// 旧字段名 label/description/group 经 alias 接受
    #[serde(default, alias = "label")]
    label_zh: Option<String>,
    #[serde(default)]
    label_en: Option<String>,
    #[serde(default, alias = "description")]
    description_zh: Option<String>,
    #[serde(default)]
    description_en: Option<String>,
    #[serde(default, alias = "group")]
    group_zh: Option<String>,
    #[serde(default)]
    group_en: Option<String>,
    #[serde(default)]
    default: Option<serde_json::Value>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    options: Vec<SkinSettingOption>,
    #[serde(default)]
    #[allow(dead_code)]
    filters: Vec<String>,
}

/// 对应安装端 WindowDefaults（width/height 等类型不符会被拒绝）；值域钳制
/// 在 validate 段做提示式镜像（安装端归一化不改包内容，打包侧只提示）
#[allow(dead_code)]
#[derive(Debug, Deserialize, Default)]
struct WindowDefaults {
    #[serde(default = "default_width")]
    width: u32,
    #[serde(default = "default_height")]
    height: u32,
    #[serde(default = "default_opacity")]
    opacity: f64,
    #[serde(default = "default_true")]
    transparent: bool,
    #[serde(default)]
    always_on_top: bool,
    #[serde(default = "default_true")]
    on_desktop: bool,
    #[serde(default)]
    resizable: bool,
    #[serde(default = "default_zoom")]
    zoom: f64,
    #[serde(default)]
    #[allow(dead_code)]
    refresh_seconds: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    edge_snap: bool,
    #[serde(default)]
    snap_gap: u32,
}

fn default_entry() -> String {
    "index.html".to_string()
}
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

fn fail(msg: &str) -> ! {
    eprintln!("打包失败：{}", msg);
    std::process::exit(1);
}

/// 与 Driftlet 管理器一致：小写字母/数字/中划线，字母或数字开头，≤64 字符，
/// 且不是 Windows 保留设备名（id 会作安装文件夹名）
fn validate_skin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && id.chars().next().map_or(false, |c| c.is_ascii_alphanumeric())
        && !is_reserved_device_name(id)
}

/// 镜像安装端 loader.rs：Windows 保留设备名黑名单（大小写不敏感）——这些名字
/// 不能作文件夹名，连「加扩展名」的形式（con.txt）同样被系统保留，故按基名判断
fn is_reserved_device_name(id: &str) -> bool {
    let base = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    matches!(
        base.as_str(),
        "con" | "prn" | "aux" | "nul"
            | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9"
            | "lpt1" | "lpt2" | "lpt3" | "lpt4" | "lpt5" | "lpt6" | "lpt7" | "lpt8" | "lpt9"
    )
}

/// 镜像安装端 loader.rs：entry 必须是皮肤文件夹内的单一文件名，
/// 拒绝目录穿越（".."）、子目录分隔符与 ADS/盘符冒号
fn is_valid_entry_name(entry: &str) -> bool {
    !entry.is_empty()
        && !entry.contains("..")
        && !entry.contains('/')
        && !entry.contains('\\')
        && !entry.contains(':')
}

/// 镜像安装端 update.rs 的 parse_version：数字前缀截断、非数字起始段计 0
///（"v1.2.3" → [1,2,3]、"1.2-beta" → [1,2]、"1.0.x" → [1,0,0]）。
/// 安装端用同一函数「提示不拦截」，打包侧同口径后两端正反都无分歧。
fn parse_version(s: &str) -> Vec<u64> {
    s.trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// 递归收集要打包的文件（相对路径）；读取目录/条目出错记入 errs，不再静默跳过
fn collect(dir: &Path, base: &Path, out: &mut Vec<PathBuf>, errs: &mut Vec<String>) {
    // settings.json* 是用户的设置值数据（应用运行时生成），不打进分发包；
    // *.dskin 是旧打包产物，防止滚进新包。比较大小写不敏感（全小写常量）
    const SKIP_FILES: [&str; 6] = [
        ".ds_store",
        "thumbs.db",
        "desktop.ini",
        "settings.json",
        "settings.json.bak",
        "settings.json.tmp",
    ];
    // 版本控制与依赖目录不属于皮肤资源
    const SKIP_DIRS: [&str; 3] = [".git", ".svn", "node_modules"];

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errs.push(format!("无法读取目录 {}：{}", dir.display(), e));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errs.push(format!("无法读取 {} 下的条目：{}", dir.display(), e));
                continue;
            }
        };
        let p = entry.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if SKIP_DIRS.contains(&name) {
                    continue;
                }
            }
            collect(&p, base, out, errs);
        } else if p.is_file() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_ascii_lowercase();
                if SKIP_FILES.contains(&lower.as_str()) || lower.ends_with(".dskin") {
                    // 根目录的排除项属预期（运行时数据/旧产物），静默跳过；
                    // 子目录里出现同名条目多半是误放，静默丢弃易误导，提示一下
                    if dir != base {
                        eprintln!("提示：跳过子目录中的排除条目 {}", p.display());
                    }
                    continue;
                }
            }
            if let Ok(rel) = p.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("用法：pack-skin <皮肤文件夹> [输出目录]");
        println!("把皮肤文件夹打包成 <id>-<version>.dskin（zip 格式）。");
        std::process::exit(if args.is_empty() { 1 } else { 0 });
    }

    let skin_dir = PathBuf::from(&args[0]);
    if !skin_dir.is_dir() {
        fail(&format!("文件夹不存在：{}", skin_dir.display()));
    }

    // 校验 skin.json（容忍 UTF-8 BOM）：先查体积上限（对齐安装端 loader.rs），
    // 再用与安装端同一套强类型 SkinManifest 完整反序列化，
    // settings[].type 非法、window.width 非数字等在此拒绝，错误信息自带 serde 行列号
    let skin_json_path = skin_dir.join("skin.json");
    let size = fs::metadata(&skin_json_path)
        .unwrap_or_else(|_| fail("找不到 skin.json —— 这不是一个皮肤文件夹"))
        .len();
    if size > MAX_MANIFEST_BYTES {
        fail(&format!(
            "skin.json 体积超限：{} 字节（上限 {} 字节）",
            size, MAX_MANIFEST_BYTES
        ));
    }
    let raw = fs::read_to_string(&skin_json_path)
        .unwrap_or_else(|e| fail(&format!("无法读取 skin.json：{}", e)));
    let manifest: SkinManifest = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .unwrap_or_else(|e| fail(&format!("skin.json 校验失败：{}", e)));
    // CLI 展示名：中文缺失回退英文（与管理器显示同规则）
    let name = manifest
        .name_zh
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| manifest.name_en.clone().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    let id = manifest
        .id
        .as_deref()
        .filter(|s| validate_skin_id(s))
        .unwrap_or_else(|| fail("skin.json 缺少合法的 id 字段（小写字母、数字、中划线，以字母或数字开头，且非 Windows 保留设备名，如 \"my-skin\"）"));
    let is_web = {
        let e = manifest.entry.trim_start();
        e.starts_with("https://") || e.starts_with("http://")
    };
    if !is_web && !is_valid_entry_name(&manifest.entry) {
        fail(&format!(
            "入口文件 '{}' 不是合法的文件名（不能包含 \"..\"、'/'、'\\'、':'）",
            manifest.entry
        ));
    }
    if !is_web && !skin_dir.join(&manifest.entry).exists() {
        fail(&format!("入口文件 '{}' 不存在", manifest.entry));
    }
    // 预览图尺寸上限（与安装端 package.rs 同口径）：头解析不出尺寸
    // （损坏/格式不明）只警告——那种图浏览器同样解不出，无内存风险
    for name in PREVIEW_FILE_NAMES {
        let path = skin_dir.join(name);
        if !path.is_file() {
            continue;
        }
        match image_dimensions(&path) {
            Some((w, h)) if w.max(h) > MAX_PREVIEW_DIMENSION => {
                fail(&format!(
                    "预览图 '{}' 尺寸 {}×{} 超过上限（最大边长 {} 像素）——管理器列表按 112px 高显示，超大预览只会白占内存",
                    name, w, h, MAX_PREVIEW_DIMENSION
                ));
            }
            Some(_) => {}
            None => eprintln!("警告：无法读取预览图 '{}' 的尺寸，跳过上限检查", name),
        }
        break;
    }
    let version = manifest.version.as_deref();
    match version {
        // 指南 §8 要求声明 version（更新包据此判断升级/降级），缺了只警告不拦
        None => eprintln!("警告：skin.json 未声明 version —— 发布检查清单（指南 §8）要求声明版本号"),
        // version 会拼进输出文件名，路径分隔符会让文件写到意外位置
        Some(v) if v.contains('/') || v.contains('\\') => {
            fail(&format!("version 不能包含 '/' 或 '\\\\'（用于输出文件名）：\"{}\"", v))
        }
        _ => {}
    }
    if let Some(v) = manifest.min_host_version.as_deref() {
        // 与安装端 update::parse_version 同口径（见本文件 parse_version
        // 镜像）——安装端「提示不拦截」，旧的严格数字段校验会拦下安装端本可
        // 接受的 "1.2-beta" / "1.0.x" 等写法（能装却打不出包）。非纯数字
        // 点分形式只作创作规范提示，不拦截。
        let segs = parse_version(v);
        let strict = v
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()));
        if !strict {
            eprintln!(
                "提示：min_host_version \"{}\" 不是纯数字点分形式（如 \"1.0.5\"）——安装端将按 {:?} 解析",
                v, segs
            );
        }
    }
    // 窗口默认值归一化镜像（安装端 loader.rs 加载时钳制：宽高 [1,10000]、
    // opacity 非有限/越界回落 [0.1,1.0]、zoom [0.5,2.0] 非有限归 1.0、
    // refresh_seconds ≤24h、snap_gap ≤200）：打包不改包内容，但声明值会被钳
    // 时提示创作者「安装生效值 ≠ 声明值」（所见即所得；归一化非拒绝，无放行分歧）
    {
        let w = &manifest.window;
        let cw = w.width.clamp(1, 10000);
        let ch = w.height.clamp(1, 10000);
        let cop = if w.opacity.is_finite() { w.opacity.clamp(0.1, 1.0) } else { 1.0 };
        let czoom = if w.zoom.is_finite() { w.zoom.clamp(0.5, 2.0) } else { 1.0 };
        let crs = w.refresh_seconds.map(|s| s.min(86400));
        let cgap = w.snap_gap.min(200);
        let mut notes = Vec::new();
        if cw != w.width {
            notes.push(format!("width {} → {}", w.width, cw));
        }
        if ch != w.height {
            notes.push(format!("height {} → {}", w.height, ch));
        }
        if cop != w.opacity {
            notes.push(format!("opacity {} → {}", w.opacity, cop));
        }
        if czoom != w.zoom {
            notes.push(format!("zoom {} → {}", w.zoom, czoom));
        }
        if crs != w.refresh_seconds {
            notes.push(format!("refresh_seconds {:?} → {:?}", w.refresh_seconds, crs));
        }
        if cgap != w.snap_gap {
            notes.push(format!("snap_gap {} → {}", w.snap_gap, cgap));
        }
        if !notes.is_empty() {
            eprintln!("提示：window 默认值超出范围，安装端加载时将归一化为：{}", notes.join("、"));
        }
    }

    // 收集文件（排序保证可复现）；目录/条目读取错误汇总后一并报出
    let mut files = Vec::new();
    let mut errs = Vec::new();
    collect(&skin_dir, &skin_dir, &mut files, &mut errs);
    if !errs.is_empty() {
        fail(&format!("收集文件时出错：\n  {}", errs.join("\n  ")));
    }
    if files.is_empty() {
        fail("皮肤文件夹是空的");
    }
    files.sort();

    // 与安装端一致的安全上限：超限的包即使打出也装不上，直接拦下
    if files.len() > MAX_FILES {
        fail(&format!(
            "文件数超过安装上限：{} 个（上限 {} 个）",
            files.len(),
            MAX_FILES
        ));
    }
    let mut total_bytes: u64 = 0;
    for rel in &files {
        let len = fs::metadata(skin_dir.join(rel))
            .unwrap_or_else(|e| fail(&format!("读取文件信息失败 {}：{}", rel.display(), e)))
            .len();
        total_bytes += len;
    }
    if total_bytes > MAX_TOTAL_BYTES {
        fail(&format!(
            "文件总体积超过安装上限：{:.1} MB（上限 {} MB）",
            total_bytes as f64 / 1024.0 / 1024.0,
            MAX_TOTAL_BYTES / 1024 / 1024
        ));
    }

    let out_dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if let Err(e) = fs::create_dir_all(&out_dir) {
        fail(&format!("无法创建输出目录：{}", e));
    }
    let base = match version {
        Some(v) => format!("{}-{}", id, v),
        None => id.to_string(),
    };
    let out_path = out_dir.join(format!("{}.dskin", base));

    // 写 zip（deflate 压缩；非 ASCII 文件名自动置 UTF-8 标志）
    let file = fs::File::create(&out_path)
        .unwrap_or_else(|e| fail(&format!("无法创建输出文件：{}", e)));
    let mut zw = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for rel in &files {
        // zip 内路径统一用正斜杠
        let rel_unix = rel.to_string_lossy().replace('\\', "/");
        zw.start_file(&rel_unix, opts)
            .unwrap_or_else(|e| fail(&format!("写入失败：{}", e)));
        let data = fs::read(skin_dir.join(rel))
            .unwrap_or_else(|e| fail(&format!("读取文件失败 {}：{}", rel_unix, e)));
        zw.write_all(&data)
            .unwrap_or_else(|e| fail(&format!("写入失败：{}", e)));
    }
    zw.finish().unwrap_or_else(|e| fail(&format!("写入失败：{}", e)));

    let size = fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    if size > MAX_PACKAGE_BYTES {
        let _ = fs::remove_file(&out_path); // 超限包装不上，不留残次品
        fail(&format!(
            "压缩包体积超过安装上限：{:.1} MB（上限 {} MB）—— 请精简皮肤资源",
            size as f64 / 1024.0 / 1024.0,
            MAX_PACKAGE_BYTES / 1024 / 1024
        ));
    }

    println!("打包完成：{}", out_path.display());
    println!(
        "  皮肤：{}（{}{}）",
        name,
        id,
        version.map(|v| format!(" v{}", v)).unwrap_or_default()
    );
    println!("  文件：{} 个，{:.1} KB", files.len(), size as f64 / 1024.0);
}
