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
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
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
    Ok(UpdateCheckResult {
        has_update: is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
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
