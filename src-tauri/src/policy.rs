//! 命令策略表：每一条经 `invoke_handler` 暴露的 IPC 命令都在这里登记它的
//! 调用方策略——全应用攻击面的唯一事实源，安全审查只读本文件即可枚举
//! 「谁能调什么」。
//!
//! 本表不改变运行时行为：闸门仍在各命令函数体内（`require_manager` /
//! `require_perm` / …）。表的价值在完备性测试——新增命令必须在此登记，
//! 且其函数体必须出现对应闸门标记，否则
//! `tests::policies_complete_and_marked` 失败。「忘了设防」由此从人肉
//! 审查项变成构建失败（审查报告 2026-08-24 的结构性回应）。
//!
//! 新增命令的纪律（AGENTS.md 硬性约定 #4 的落点）：
//!   1. 写命令，函数体首行放闸门；
//!   2. 在下方 `COMMAND_POLICIES` 表登记一行（闸门档 + 权限常量名）；
//!   3. `cargo test` 会双向核对：表 ↔ lib.rs 的 generate_handler! 清单、
//!      表 ↔ 命令函数体内的闸门标记。
//!
//! 新功能上线前的两个问题（取代「想象所有坏法」）：
//!   - 它在策略表的哪一行？（= 谁被允许调它）
//!   - 它让宿主以更高信任消费皮肤可影响的数据了吗？（执行 / 渲染进管理器
//!     DOM / eval——跨了这个边界的进 `docs/关键机制.md` 的宿主信任清单）

/// 闸门档。`marker()` 给出该档在命令函数体内必须出现的源码标记
/// （完备性测试按文本核对——重构闸门调用形态时改这里一处即可）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// 管理器窗口专属：`require_manager`（皮肤窗口调用一律拒绝）
    ManagerOnly,
    /// 日志窗口专属：`require_log_window`
    LogWindow,
    /// 皮肤须声明对应权限：`require_perm` 二元校验（档位仅安装页展示层，
    /// 后端不分档）。成员是**权限常量名**（如 "PERM_SYS_INFO"）——与命令
    /// 源码里的写法一致，测试按文本核对；常量名 → 权限值的映射见
    /// `perm_const_value`（编译期锚定：常量改名/删除会编译失败）。
    Perm(&'static str),
    /// 分层闸 `require_any_perm`：open_external 的 http(s) 目标过
    /// open_link 低危或 system 高危任一，其余 URI 目标只过 system
    AnyPerm,
    /// `resolve_control_target`：作用于自己免权限、指定他人一律
    /// control 中危门（skin_list_skins 例外恒走 control，用 Perm 登记）
    ControlTarget,
    /// 免权限基线：`caller_skin` 身份 + 沙箱内操作（自身目录文件 /
    /// 自己 schema 的设置 / 日志 / 广播）
    CallerSkin,
    /// 皮肤窗口 label 身份校验（strip_prefix "skin-"）：无害自作用命令
    /// 与高频日志/DevTools 通道（身份不扫盘重验；open_skin_devtools 另有
    /// 运行时 dev 开关，release 构建恒 no-op）
    SkinLabel,
    /// 无闸无害：函数体只作用于调用方自身窗口，连身份校验都无必要
    ///（仅 start_skin_drag / start_skin_resize 两条——拖动/缩放自己）
    Ungated,
}

impl Gate {
    /// 函数体内必须出现的闸门标记；None = 无标记可校（Ungated 档）。
    /// 不带结尾 `?`——take_hotkey_error 这类返回 Option 的命令用
    /// `.ok()?` 形态消费闸门结果，标记按调用点而非收尾核对。
    /// 仅完备性测试调用——非测试构建下豁免死代码警告。
    #[cfg_attr(not(test), allow(dead_code))]
    fn marker(&self) -> Option<String> {
        match self {
            Gate::ManagerOnly => Some("require_manager(&window)".into()),
            Gate::LogWindow => Some("require_log_window(&window)".into()),
            Gate::Perm(p) => Some(format!("require_perm(&state, &window, {})", p)),
            Gate::AnyPerm => Some("require_any_perm(&state, &window".into()),
            Gate::ControlTarget => Some("resolve_control_target(&state, &window".into()),
            Gate::CallerSkin => Some("caller_skin(".into()),
            Gate::SkinLabel => Some("strip_prefix(\"skin-\")".into()),
            Gate::Ungated => None,
        }
    }
}

use crate::skin_api::{PERM_CLIPBOARD, PERM_CONTROL, PERM_FILE_SYSTEM, PERM_MEDIA, PERM_MIC,
    PERM_NETWORK, PERM_NOTIFY, PERM_OPEN_LINK, PERM_REGISTRY, PERM_SHELL, PERM_SYS_INFO,
    PERM_SYSTEM};

/// 权限常量名 → 权限值。表行 `Gate::Perm` 存的是常量名（与源码文本一致），
/// 此映射把名字锚回真实常量：表里的名字拼错会在测试期 panic，常量被
/// 改名/删除则这里直接编译失败——两侧都不可能静默漂移。
#[allow(dead_code)]
fn perm_const_value(name: &str) -> &'static str {
    match name {
        "PERM_REGISTRY" => PERM_REGISTRY,
        "PERM_SHELL" => PERM_SHELL,
        "PERM_SYSTEM" => PERM_SYSTEM,
        "PERM_MEDIA" => PERM_MEDIA,
        "PERM_NOTIFY" => PERM_NOTIFY,
        "PERM_SYS_INFO" => PERM_SYS_INFO,
        "PERM_NETWORK" => PERM_NETWORK,
        "PERM_OPEN_LINK" => PERM_OPEN_LINK,
        "PERM_CLIPBOARD" => PERM_CLIPBOARD,
        "PERM_MIC" => PERM_MIC,
        "PERM_FILE_SYSTEM" => PERM_FILE_SYSTEM,
        "PERM_CONTROL" => PERM_CONTROL,
        other => panic!("unknown permission const in policy table: {}", other),
    }
}

/// 命令名 → 闸门档。与 lib.rs 的 generate_handler! 清单一一对应
/// （完备性测试强制双向匹配）。
/// 顺序约定（审查 S4 定案）：闸门必须先于取数与副作用——唯一合规前导 =
/// `let lang`/`let state` 取句柄、或闸门输入的归一化（如 open_external 的
/// target.trim()）。曾尝试把顺序断言写进测试，被「归一化后gate」这类
/// 合法前导误伤（误报率高于价值），退回文档约定 + 人工审查。
#[allow(dead_code)] // 运行时不读表——价值在测试与审查；勿删
pub const COMMAND_POLICIES: &[(&str, Gate)] = &[
    // ─── 管理器命令（ManagerOnly）与皮肤自作用例外 ───
    // （例外 = Ungated 拖动/缩放 + SkinLabel 只作用自己窗口的项；
    //  SkinLabel 的函数体分散在 commands.rs 与 skin_api/，不按文件分区）
    ("start_skin_drag", Gate::Ungated),
    ("start_skin_resize", Gate::Ungated),
    ("list_skins", Gate::ManagerOnly),
    ("get_skin_detail", Gate::ManagerOnly),
    ("load_skin", Gate::ManagerOnly),
    ("unload_skin", Gate::ManagerOnly),
    ("reload_skin", Gate::ManagerOnly),
    ("set_skin_opacity", Gate::ManagerOnly),
    ("set_skin_placement", Gate::ManagerOnly),
    ("set_skin_click_through", Gate::ManagerOnly),
    ("set_skin_position_locked", Gate::ManagerOnly),
    ("set_skin_resizable", Gate::ManagerOnly),
    ("set_skin_zoom", Gate::ManagerOnly),
    ("set_skin_edge_snap", Gate::ManagerOnly),
    ("set_skin_snap_gap", Gate::ManagerOnly),
    ("set_skin_position", Gate::ManagerOnly),
    ("bring_skin_onscreen", Gate::ManagerOnly),
    ("show_skin_context_menu", Gate::SkinLabel),
    // 皮肤自定义右键菜单项：只影响本皮肤菜单，自作用无害（SkinLabel）
    ("skin_set_menu_items", Gate::SkinLabel),
    ("set_skin_size", Gate::ManagerOnly),
    ("set_skin_custom_setting", Gate::ManagerOnly),
    ("reset_skin_config", Gate::ManagerOnly),
    ("pick_skin_package", Gate::ManagerOnly),
    ("package_skin", Gate::ManagerOnly),
    ("inspect_skin_package", Gate::ManagerOnly),
    ("install_skin_package", Gate::ManagerOnly),
    ("remove_skin", Gate::ManagerOnly),
    ("duplicate_skin", Gate::ManagerOnly),
    ("sync_skin_copy", Gate::ManagerOnly),
    ("get_app_config", Gate::ManagerOnly),
    ("set_autostart", Gate::ManagerOnly),
    ("get_autostart", Gate::ManagerOnly),
    ("set_theme", Gate::ManagerOnly),
    ("set_language", Gate::ManagerOnly),
    ("set_skin_visibility", Gate::ManagerOnly),
    ("set_skin_groups", Gate::ManagerOnly),
    ("capture_layout", Gate::ManagerOnly),
    ("apply_layout", Gate::ManagerOnly),
    ("set_layouts", Gate::ManagerOnly),
    ("set_hotkey", Gate::ManagerOnly),
    ("set_skin_hotkey", Gate::ManagerOnly),
    ("set_hot_reload", Gate::ManagerOnly),
    ("check_update", Gate::ManagerOnly),
    ("set_update_check", Gate::ManagerOnly),
    ("open_release_page", Gate::ManagerOnly),
    ("download_update", Gate::ManagerOnly),
    ("install_update", Gate::ManagerOnly),
    ("take_hotkey_error", Gate::ManagerOnly),
    ("open_skins_folder", Gate::ManagerOnly),
    ("pick_path", Gate::ManagerOnly),
    ("open_skin_folder", Gate::ManagerOnly),
    ("list_system_fonts", Gate::ManagerOnly),
    // GPU 适配器枚举：管理器 gpu_adapter 控件的数据源（只读系统信息，无写面）
    ("list_gpu_adapters", Gate::ManagerOnly),
    ("capture_skin_preview", Gate::ManagerOnly),
    ("take_pending_package_install", Gate::ManagerOnly),
    ("export_config", Gate::ManagerOnly),
    ("inspect_backup", Gate::ManagerOnly),
    ("import_config", Gate::ManagerOnly),
    ("open_log_window", Gate::ManagerOnly),
    ("get_app_log", Gate::LogWindow),
    ("clear_app_log", Gate::LogWindow),
    // 皮肤 F12 转发通道：SkinLabel 身份 + 运行时 dev 开关双重门槛
    //（release 构建恒 no-op），不是管理器命令
    ("open_skin_devtools", Gate::SkinLabel),
    // ─── skin_api：皮肤命令（权限闸 / 免权限基线） ───
    ("get_cpu_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_gpu_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_memory_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_disks_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_disk_space", Gate::Perm("PERM_SYS_INFO")),
    ("get_network_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_audio_spectrum", Gate::Perm("PERM_MEDIA")),
    ("skin_read_file", Gate::CallerSkin),
    ("skin_write_file", Gate::CallerSkin),
    ("skin_list_dir", Gate::CallerSkin),
    ("skin_delete_file", Gate::CallerSkin),
    ("skin_set_setting", Gate::CallerSkin),
    ("skin_get_setting", Gate::CallerSkin),
    ("read_registry_value", Gate::Perm("PERM_REGISTRY")),
    ("run_command", Gate::Perm("PERM_SHELL")),
    ("get_os_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_processes", Gate::Perm("PERM_SYS_INFO")),
    ("get_volume", Gate::Perm("PERM_MEDIA")),
    ("set_volume", Gate::Perm("PERM_MEDIA")),
    ("set_mute", Gate::Perm("PERM_MEDIA")),
    ("get_media_info", Gate::Perm("PERM_MEDIA")),
    ("media_control", Gate::Perm("PERM_MEDIA")),
    ("media_seek", Gate::Perm("PERM_MEDIA")),
    ("read_clipboard_text", Gate::Perm("PERM_CLIPBOARD")),
    ("write_clipboard_text", Gate::Perm("PERM_CLIPBOARD")),
    ("open_external", Gate::AnyPerm),
    ("show_notification", Gate::Perm("PERM_NOTIFY")),
    ("lock_workstation", Gate::Perm("PERM_SYSTEM")),
    ("monitor_off", Gate::Perm("PERM_SYSTEM")),
    ("sleep", Gate::Perm("PERM_SYSTEM")),
    ("power_control", Gate::Perm("PERM_SYSTEM")),
    ("empty_recycle_bin", Gate::Perm("PERM_SYSTEM")),
    ("get_mic_spectrum", Gate::Perm("PERM_MIC")),
    ("get_battery_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_idle_time", Gate::Perm("PERM_SYS_INFO")),
    ("get_foreground_window_info", Gate::Perm("PERM_SYS_INFO")),
    ("get_monitors", Gate::Perm("PERM_SYS_INFO")),
    ("get_system_theme", Gate::Perm("PERM_SYS_INFO")),
    ("skin_log", Gate::CallerSkin),
    ("skin_console_log", Gate::SkinLabel),
    ("skin_read_any_file", Gate::Perm("PERM_FILE_SYSTEM")),
    ("skin_write_any_file", Gate::Perm("PERM_FILE_SYSTEM")),
    ("skin_list_any_dir", Gate::Perm("PERM_FILE_SYSTEM")),
    ("skin_create_any_dir", Gate::Perm("PERM_FILE_SYSTEM")),
    ("skin_delete_any_path", Gate::Perm("PERM_FILE_SYSTEM")),
    ("skin_get_window_config", Gate::ControlTarget),
    ("skin_set_window_config", Gate::ControlTarget),
    ("skin_load", Gate::ControlTarget),
    ("skin_unload", Gate::ControlTarget),
    ("skin_reload", Gate::ControlTarget),
    ("skin_list_skins", Gate::Perm("PERM_CONTROL")),
    ("http_request", Gate::Perm("PERM_NETWORK")),
    ("skin_broadcast", Gate::CallerSkin),
    ("skin_hide", Gate::ControlTarget),
    ("skin_show", Gate::ControlTarget),
];

#[cfg(test)]
mod tests {
    use super::{COMMAND_POLICIES, Gate};

    /// 从 lib.rs 源码提取 generate_handler![...] 清单（模块前缀, 命令名）。
    /// 解析按文本进行：清单内无嵌套方括号，条目恒为 `module::name,`。
    fn handler_entries() -> Vec<(String, String)> {
        let src = include_str!("lib.rs");
        let start = src
            .find("generate_handler![")
            .expect("generate_handler! not found in lib.rs");
        let rest = &src[start + "generate_handler![".len()..];
        let end = rest.find(']').expect("handler list not closed");
        rest[..end]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.split_once("::")
                    .map(|(m, n)| (m.to_string(), n.to_string()))
                    .expect("handler entry must be module::name")
            })
            .collect()
    }

    /// 命令模块名 → 模块源码。新模块（第三个命令文件）在此登记。
    fn module_source(module: &str) -> &'static str {
        match module {
            "commands" => include_str!("commands.rs"),
            "skin_api" => include_str!("skin_api/mod.rs"),
            other => panic!(
                "policy test: unknown command module '{}' — 新命令模块需在 policy::tests::module_source 登记",
                other
            ),
        }
    }

    /// 截取命令函数体段：从 `fn name(` 起，到最近的下一个顶层项边界
    ///（下一条命令 / 下一个 fn / 常量 / 静态量 / 测试模块 / 下一项的文档
    /// 注释行）为止。复审 D-C：原先只认 `#[tauri::command]`，末条命令的段
    /// 会延伸进后续 helper 与整个 tests 模块——helper/测试里恰好出现的
    /// 同档标记文本会造成「误判放行」（命令丢了闸门、测试仍绿）。
    /// 行注释剥离后再返回：注释里抄一行闸门文本不得骗过核对（复审 D-F）。
    fn fn_segment(src: &str, name: &str) -> String {
        let sig = format!("fn {}(", name);
        let start = src
            .find(&sig)
            .unwrap_or_else(|| panic!("command fn '{}' not found in module source", name));
        let rest = &src[start..];
        let end = ["\nfn ", "\npub", "\nstatic", "\nconst", "\nstruct", "\nimpl", "\nmod", "\n///", "\n//", "\n#["]
            .iter()
            .filter_map(|m| rest[1..].find(m).map(|i| i + 1))
            .min()
            .unwrap_or(rest.len());
        rest[..end]
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 完备性 + 标记核对，三重断言：
    ///  1. 策略表与 generate_handler! 清单是同一个集合（双向无遗漏、
    ///     各自无重复）——新增命令忘了登记、或删命令忘了摘行，都红；
    ///  2. 每条命令的函数体内出现其登记档位的闸门标记——登记了但函数
    ///     体没设防，红；
    ///  3. 闸门标记错档（比如该 PERM_MEDIA 写成 PERM_SYSTEM）同样红——
    ///     标记串含权限常量名。
    #[test]
    fn policies_complete_and_marked() {
        let entries = handler_entries();
        assert_eq!(
            entries.len(),
            COMMAND_POLICIES.len(),
            "handler list ({}) and policy table ({}) differ in size",
            entries.len(),
            COMMAND_POLICIES.len()
        );
        let mut handler_sorted: Vec<&(String, String)> = entries.iter().collect();
        handler_sorted.sort_by(|a, b| a.1.cmp(&b.1));
        handler_sorted.windows(2).for_each(|w| {
            assert_ne!(w[0].1, w[1].1, "duplicate handler entry '{}'", w[0].1)
        });
        let mut table_sorted: Vec<&str> = COMMAND_POLICIES.iter().map(|(n, _)| *n).collect();
        table_sorted.sort_unstable();
        table_sorted.windows(2).for_each(|w| {
            assert_ne!(w[0], w[1], "duplicate policy row '{}'", w[0])
        });
        for (module, name) in &entries {
            let row = COMMAND_POLICIES
                .iter()
                .find(|(n, _)| n == name)
                .unwrap_or_else(|| panic!("command '{}' is registered but has no policy row", name));
            let Some(marker) = row.1.marker() else { continue };
            let seg = fn_segment(module_source(module), name);
            assert!(
                seg.contains(&marker),
                "command '{}' is registered as {:?} but its body lacks the gate marker {:?}",
                name,
                row.1,
                marker
            );
        }
        for (name, _) in COMMAND_POLICIES {
            assert!(
                entries.iter().any(|(_, n)| n == name),
                "policy row '{}' has no matching handler entry",
                name
            );
        }
    }

    /// 表内每个 Perm 常量名都锚定回真实权限常量（拼错的名字在
    /// perm_const_value 里 panic；常量改名/删除则编译期就失败）
    #[test]
    fn perm_const_names_resolve() {
        for (cmd, gate) in COMMAND_POLICIES {
            if let Gate::Perm(name) = gate {
                let v = super::perm_const_value(name);
                assert!(!v.is_empty(), "{}: empty permission value", cmd);
            }
        }
    }

    /// 档位分布快照：意外改档（比如某命令从 ManagerOnly 降成免权限）
    /// 会被计数变化钉出来。改档位时同步更新这些数字。
    #[test]
    fn gate_distribution_snapshot() {
        let count = |pred: fn(&Gate) -> bool| COMMAND_POLICIES.iter().filter(|(_, g)| pred(g)).count();
        assert_eq!(count(|g| matches!(g, Gate::ManagerOnly)), 55);
        assert_eq!(count(|g| matches!(g, Gate::LogWindow)), 2);
        assert_eq!(count(|g| matches!(g, Gate::Ungated)), 2);
        assert_eq!(count(|g| matches!(g, Gate::SkinLabel)), 4);
        assert_eq!(count(|g| matches!(g, Gate::CallerSkin)), 8);
        assert_eq!(count(|g| matches!(g, Gate::ControlTarget)), 7);
        assert_eq!(count(|g| matches!(g, Gate::AnyPerm)), 1);
        assert_eq!(count(|g| matches!(g, Gate::Perm(_))), 38);
        assert_eq!(COMMAND_POLICIES.len(), 117);
    }
}
