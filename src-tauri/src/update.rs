//! 启动更新检测：查询公开分发仓库的 GitHub releases API，与当前版本做
//! 数字段比较。阻塞式 HTTPS（ureq/rustls），调用方必须放 spawn_blocking；
//! 网络失败/无 release/解析失败一律 Err 返回，前端静默忽略（不打断启动）。

use serde::{Deserialize, Serialize};

/// 公开分发仓库（仅此仓库发 release，更新检测比对它的最新 release）
const REPO: &str = "xiaochengzina/Driftlet";

/// 「前往下载」固定打开最新 release 页（GitHub 自动重定向到最新 tag），
/// 后端写死、不接受前端入参
pub const RELEASES_LATEST_URL: &str = "https://github.com/xiaochengzina/Driftlet/releases/latest";

#[derive(Serialize, Clone, Debug)]
pub struct UpdateCheckResult {
    pub current_version: String,
    /// 归一化后的最新版本号（已去 v 前缀），弹窗直接展示
    pub latest_version: String,
    /// 该 release 的 GitHub 页面（来自 API 的 html_url；前端实际跳转走
    /// 后端固定的 RELEASES_LATEST_URL，此字段仅供展示/调试）
    pub release_url: String,
    pub has_update: bool,
    /// NSIS 安装包的直链（assets 里 x64 setup 的 browser_download_url；
    /// 没找到安装包资产时为 None——前端走「前往下载」降级）
    pub installer_url: Option<String>,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// 阻塞式获取最新 release 并比较版本（GitHub API 要求 User-Agent）。
pub fn fetch_latest_release() -> Result<UpdateCheckResult, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let body = ureq::get(&url)
        .set("User-Agent", concat!("Driftlet/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let release: GithubRelease = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = release.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    // 挑 NSIS 安装包资产（命名固定 <Product>_X.Y.Z_x64-setup.exe）
    let installer_url = release
        .assets
        .iter()
        .find(|a| a.name.ends_with("_x64-setup.exe"))
        .map(|a| a.browser_download_url.clone());
    Ok(UpdateCheckResult {
        has_update: is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        installer_url,
    })
}

/// "v1.2.3" / "1.2.3" → [1, 2, 3]；段内非数字后缀（"1.2-beta" → [1, 2]）
/// 从首个非数字字符截断，非数字起始的段按 0 计。
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

/// latest 是否新于 current：逐段数字比较，短号段补 0（1.1 与 1.1.0 相等）。
pub fn is_newer(latest: &str, current: &str) -> bool {
    let l = parse_version(latest);
    let c = parse_version(current);
    for i in 0..l.len().max(c.len()) {
        let a = l.get(i).copied().unwrap_or(0);
        let b = c.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

// ─── 安装包自动下载（发现新版本后后台预载，完成才提示安装） ───

/// 安装包固定文件名——下次下载覆盖旧的，安装包不随版本更新堆积。
pub const INSTALLER_FILENAME: &str = "Driftlet-update-setup.exe";
/// 下载完成时写入的版本标记（启动清理据此判定「装上了没有」）
const MARKER_FILENAME: &str = "downloaded-version.json";
/// 安装器体积上限（远超当前 ~5MB 的异常响应即拒）
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;

/// 更新下载目录（固定文件名 + 单一 .tmp 都在里面，任何时刻最多两件）
pub fn update_dir(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir
        .parent()
        .unwrap_or(config_dir)
        .join("update")
}

/// 阻塞式下载安装包到更新目录：先写 .tmp 再 rename 就位（中断只留一个
/// .tmp，下次下载覆盖，不堆积）。成功后写版本标记。调用方放 spawn_blocking。
pub fn download_installer(
    config_dir: &std::path::Path,
    url: &str,
    version: &str,
) -> Result<std::path::PathBuf, String> {
    // 来源钉死在公开仓库的 release 下载域——下载链接只能来自 GitHub API
    // 的 assets（前端把 check_update 的结果原样传回），不接受任意 URL
    let prefix = format!("https://github.com/{}/releases/download/", REPO);
    if !url.starts_with(&prefix) {
        return Err(format!("installer url must be under {}", prefix));
    }
    let dir = update_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{}.tmp", INSTALLER_FILENAME));
    let dest = dir.join(INSTALLER_FILENAME);
    // 起手清掉旧的半截 .tmp（上次中断的残留）
    let _ = std::fs::remove_file(&tmp);

    let result = (|| -> Result<(), String> {
        let resp = ureq::get(url)
            .set("User-Agent", concat!("Driftlet/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(120))
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = std::io::Read::take(resp.into_reader(), MAX_INSTALLER_BYTES + 1);
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let written = std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
        if written == 0 || written > MAX_INSTALLER_BYTES {
            return Err(format!("bad installer size: {} bytes", written));
        }
        drop(file);
        std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
        // 版本标记：启动清理按它判定这次下载对应的版本装上了没有
        std::fs::write(
            dir.join(MARKER_FILENAME),
            serde_json::json!({ "version": version }).to_string(),
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|_| dest)
}

/// 启动清理：当前版本 ≥ 标记版本（= 新版本装上了）→ 删安装包与标记。
/// 装完新版本的用户不会再需要旧安装包；没装的（当前版本仍旧）保留等提示。
pub fn cleanup_downloaded_installer(config_dir: &std::path::Path) {
    let dir = update_dir(config_dir);
    let Ok(marker) = std::fs::read_to_string(dir.join(MARKER_FILENAME)) else {
        return;
    };
    let version = serde_json::from_str::<serde_json::Value>(&marker)
        .ok()
        .and_then(|v| v.get("version")?.as_str().map(String::from));
    let Some(version) = version else { return };
    if is_newer(&version, env!("CARGO_PKG_VERSION")) {
        return; // 标记版本更新——下载的还没装，保留
    }
    let _ = std::fs::remove_file(dir.join(INSTALLER_FILENAME));
    let _ = std::fs::remove_file(dir.join(MARKER_FILENAME));
    // 顺带清半截 .tmp（极端时序：下载中断后用户直接装了别的渠道版本）
    let _ = std::fs::remove_file(dir.join(format!("{}.tmp", INSTALLER_FILENAME)));
}

#[cfg(test)]
mod tests {
    use super::{is_newer, parse_version};

    #[test]
    fn parses_common_shapes() {
        assert_eq!(parse_version("1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("V2.0"), vec![2, 0]);
        assert_eq!(parse_version(" 1.0.4 "), vec![1, 0, 4]);
        // 预发布后缀从首个非数字截断
        assert_eq!(parse_version("v1.2-beta"), vec![1, 2]);
        assert_eq!(parse_version("1.0.x"), vec![1, 0, 0]);
    }

    #[test]
    fn compares_numeric_segments() {
        assert!(is_newer("1.0.5", "1.0.4"));
        assert!(is_newer("1.0.10", "1.0.9"));
        assert!(is_newer("v2.0", "1.9.9"));
        assert!(is_newer("1.1", "1.0.9"));
        // 相等 / 更旧 / 短号段补 0 均不算「有更新」
        assert!(!is_newer("1.0.4", "1.0.4"));
        assert!(!is_newer("1.0.3", "1.0.4"));
        assert!(!is_newer("1.1.0", "1.1"));
        assert!(!is_newer("v1.0.4", "1.0.4"));
    }
}
