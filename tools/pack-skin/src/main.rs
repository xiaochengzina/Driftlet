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
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// 与安装端（src-tauri/src/skin/package.rs）一致的安全上限：防恶意/损坏包耗尽磁盘
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024; // 压缩包 64 MB
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024; // 解压后合计 256 MB
const MAX_FILES: usize = 5000;
/// 对齐安装端 loader.rs：skin.json 体积上限，超限即视为异常
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024; // 1 MB

// ---------------------------------------------------------------
// 以下 serde 结构复制精简自安装端 src-tauri/src/skin/types.rs，
// 两边字段/默认值保持一致 —— 保证「打包放行 = 安装放行」。
// 安装端结构改动时这里要同步。
// ---------------------------------------------------------------

/// Skin manifest（对应安装端 SkinManifest，只保留校验所需字段）
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SkinManifest {
    #[serde(default)]
    id: Option<String>,
    name: String,
    /// 英文皮肤名（bilingual 皮肤专用；留空时英文界面回退 name）
    #[serde(default)]
    name_en: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// 英文简介（bilingual 皮肤专用；留空回退 description）
    #[serde(default)]
    description_en: Option<String>,
    /// 中英双语声明（作者侧开关）：false/缺省 = 单语皮肤，所有 *_en 字段一律忽略
    #[serde(default)]
    bilingual: bool,
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
    #[serde(default)]
    label: Option<String>,
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
}

/// 对应安装端 SkinSettingDef
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SkinSettingDef {
    key: String,
    #[serde(rename = "type")]
    kind: SkinSettingKind,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    label_en: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    description_en: Option<String>,
    #[serde(default)]
    group: Option<String>,
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

/// 对应安装端 WindowDefaults（width/height 等类型不符会被拒绝）
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
    let name = &manifest.name;
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
        // 宽松数字段格式（与安装端 update::is_newer 的解析口径一致：1.2 / 1.2.3 / v1.2.3）
        let valid = v
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('.')
            .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()));
        if !valid {
            fail(&format!(
                "min_host_version 格式非法（应为 \"1.0.5\" 这类数字段版本号）：\"{}\"",
                v
            ));
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
