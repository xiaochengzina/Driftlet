//! 启动更新检测：查询公开分发仓库的 GitHub releases API，与当前版本做
//! 数字段比较。阻塞式 HTTPS（ureq/rustls），调用方必须放 spawn_blocking；
//! 网络失败/无 release/解析失败一律 Err 返回，前端静默忽略（不打断启动）。
//!
//! 安装包下载（国内提速设计，详见 docs/关键机制.md「更新检测与自动下载」）：
//! 直连 GitHub 与多个加速镜像**多源竞速** + 单源内 **8 段 Range 分段并行**。
//! 信任根只有一个——api.github.com 的 release 数据：镜像源只有在 release
//! 说明带官方 SHA-256（`SHA256: <hex>` 行）时才进入候选列表，落盘内容必须
//! 与官方哈希一致才算成功（eq_ignore_ascii_case 一票否决，fail-closed）；
//! 无官方哈希的老 release 只走 GitHub 直连（TLS 即信任锚）。版本号与哈希
//! 绝不取自第三方镜像——否则镜像可同时伪造版本+哈希+安装包，整条更新链
//! 被 MITM。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 公开分发仓库（仅此仓库发 release，更新检测比对它的最新 release）
const REPO: &str = "xiaochengzina/Driftlet";

/// 「前往下载」固定打开最新 release 页（GitHub 自动重定向到最新 tag），
/// 后端写死、不接受前端入参
pub const RELEASES_LATEST_URL: &str = "https://github.com/xiaochengzina/Driftlet/releases/latest";

/// 公开仓库主页（关于页「仓库地址」），后端写死、不接受前端入参
pub const REPO_URL: &str = "https://github.com/xiaochengzina/Driftlet";

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
    /// 官方 SHA-256（解析自 release 说明的 `SHA256: <hex>` 行；缺失 =
    /// 老 release——下载只走 GitHub 直连、不启用镜像加速，fail-closed）
    pub installer_sha256: Option<String>,
    /// 该版本的安装包是否已躺在更新目录且通过哈希核对（上次下载完成或
    /// 用户点过「稍后」）——true 时前端跳过下载直接「立即安装」
    pub installer_ready: bool,
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// 阻塞式获取最新 release 并比较版本（GitHub API 要求 User-Agent）。
/// `config_dir` 用于判定该版本安装包是否已下载就位（installer_ready）。
pub fn fetch_latest_release(config_dir: &Path) -> Result<UpdateCheckResult, String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", REPO);
    let body = ureq::get(&url)
        .set(
            "User-Agent",
            concat!("Driftlet/", env!("CARGO_PKG_VERSION")),
        )
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let release: GithubRelease = serde_json::from_str(&body).map_err(|e| e.to_string())?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    // 挑 NSIS 安装包资产（命名固定 <Product>_X.Y.Z_x64-setup.exe）
    let installer_url = release
        .assets
        .iter()
        .find(|a| a.name.ends_with("_x64-setup.exe"))
        .map(|a| a.browser_download_url.clone());
    let installer_sha256 = extract_installer_sha256(&release.body);
    let mut result = UpdateCheckResult {
        has_update: is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        release_url: release.html_url,
        installer_url,
        installer_sha256,
        installer_ready: false,
    };
    // 已就位判定按「最终」latest 版本算（与 result 里的版本同一份）
    result.installer_ready = installer_ready(&update_dir(config_dir), &result.latest_version);
    Ok(result)
}

/// 从 release 说明解析安装包官方 SHA-256（64 位十六进制，大小写皆可）。
/// 发布流程要求 notes 里写 `SHA256: <hex>` 一行（见 docs/公开发布流程.md，
/// 仅开发仓库留存）：
/// 只认**含 sha256 字样行**里的「恰好 64 位、左右均被非十六进制字符夹住」
/// 的十六进制串——自动生成的 release 说明里有 40 位 commit SHA，绝不能被
/// 误当安装包哈希；64 位全数字串在真实 SHA-256 中概率 ~3e-13，再要求至少
/// 含一个字母，排除从更长串里切出数字段的误匹配。
fn extract_installer_sha256(body: &str) -> Option<String> {
    for line in body.lines() {
        if !line.to_ascii_lowercase().contains("sha256") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !bytes[i].is_ascii_hexdigit() {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            let hex = &line[start..i];
            if hex.len() == 64 && hex.chars().any(|c| c.is_ascii_alphabetic()) {
                return Some(hex.to_ascii_lowercase());
            }
        }
    }
    None
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

/// 更新下载目录：固定文件名的安装包 + 版本标记 + 瞬时分段残留
/// （`.s<源>.part`，任何下载尝试起手全清，不堆积）
pub fn update_dir(config_dir: &std::path::Path) -> std::path::PathBuf {
    config_dir.parent().unwrap_or(config_dir).join("update")
}

/// GitHub release 加速镜像前缀（用法：`{前缀}{github 直链}`）。公共镜像
/// 来去无常，所以这是一份**尽力而为的候选清单**而非可信基础设施——安全
/// 不靠镜像靠哈希：只有 release 说明带官方 SHA-256 时镜像才进候选列表，
/// 落盘内容与官方哈希不符即废（见文首信任模型）。失效镜像只是慢，不会错。
const MIRRORS: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://github.moeyy.xyz/",
];

/// 单源分段并行数：国内对 GitHub 单连接常限速到十几 KB/s，8 段并行约
/// 一个量级提速；镜像单流往往已是满速，分段探测失败自动退回单流
const SEGMENTS: usize = 8;
/// 首个（探测）段的请求长度，也作为最小分段尺度
const PROBE_BYTES: u64 = 256 * 1024;
/// 单请求总时限（连不上/下不动一律到时即废，换其他源竞速）
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// 连接超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 读停滞超时：连续这么久一个字节都读不到即废（比总时限更快的僵死判定）
const READ_STALL_TIMEOUT: Duration = Duration::from_secs(15);
/// 手工跟跳上限（禁掉 ureq 自动重定向，Range 头逐跳重发——镜像 302 布局
/// 无常，自动跟跳对自定义头的处理不可控；同 skin_api/http_request 先例）
const MAX_REDIRECTS: usize = 5;
/// 进度事件节流间隔
const PROGRESS_TICK: Duration = Duration::from_millis(500);

/// 单个下载源的进度快照（下发前端进度条）
#[derive(Clone, Copy, Debug)]
pub struct SourceProgress {
    /// 源序号：0 = GitHub 直连，其余 = MIRRORS 序 + 1
    pub index: usize,
    pub downloaded: u64,
    /// 已知总大小；0 = 未知（单流且服务器未给 content-length）
    pub total: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressStage {
    /// 各源正在建立连接/等首字节
    Connecting,
    /// 下载中（进度条按「最领先的源」推进）
    Downloading,
    /// 全部源收尾/校验哈希
    Verifying,
    /// 已就位（标记已写）
    Done,
}

#[derive(Clone, Debug)]
pub struct DownloadProgress {
    pub stage: ProgressStage,
    pub sources: Vec<SourceProgress>,
}

/// 候选源清单：GitHub 直连永远第一；**仅当 release 说明带官方 SHA-256
/// 时**加速镜像才进列表（无哈希 = 无校验手段 = 不信任第三方链路）。
fn source_urls(url: &str, expected_sha256: Option<&str>) -> Vec<String> {
    let mut v = vec![url.to_string()];
    if expected_sha256.is_some() {
        v.extend(MIRRORS.iter().map(|m| format!("{m}{url}")));
    }
    v
}

/// 首个探测段（0..=first_end）落地后的剩余分段计划：`(start, end)` 闭区间，
/// 段数 ≤ SEGMENTS、每段 ≥ PROBE_BYTES（文件不足时只有探测段一个）。
fn plan_ranges(total: u64, first_end: u64) -> Vec<(u64, u64)> {
    let mut ranges = Vec::new();
    if total <= first_end + 1 {
        return ranges;
    }
    let remaining = total - (first_end + 1);
    let n = remaining.div_ceil(PROBE_BYTES).min(SEGMENTS as u64) as usize;
    let mut start = first_end + 1;
    for k in 0..n {
        let end = if k == n - 1 {
            total - 1
        } else {
            start + (remaining / n as u64) - 1
        };
        ranges.push((start, end));
        start = end + 1;
    }
    ranges
}

struct SourceState {
    downloaded: AtomicU64,
    total: AtomicU64,
    done: AtomicBool,
}

/// 阻塞式多源竞速下载安装包到更新目录（调用方放 spawn_blocking）。
/// 直连 + 镜像同时开工，谁先**通过 SHA-256 校验**谁就位（其余源随即中止，
/// 校验保证胜者与官方哈希一致——多个源同时成功时字节必然相同）；全部源
/// 失败才整体报错，前端降级「前往下载」。固定文件名覆盖旧包，分段写在
/// `.s<源>.part` 里、任何时机起手全清，不堆积。成功后写版本标记（含
/// SHA-256——「立即安装」执行前复核用）。
pub fn download_installer(
    config_dir: &Path,
    url: &str,
    version: &str,
    expected_sha256: Option<&str>,
    on_progress: &(dyn Fn(DownloadProgress) + Send + Sync),
) -> Result<PathBuf, String> {
    // 来源钉死在公开仓库的 release 下载域——下载链接只能来自 GitHub API
    // 的 assets（前端把 check_update 的结果原样传回），不接受任意 URL；
    // 镜像由此前缀改写派生，不可能把下载引导到仓库之外的域名
    let prefix = format!("https://github.com/{}/releases/download/", REPO);
    if !url.starts_with(&prefix) {
        return Err(format!("installer url must be under {}", prefix));
    }
    let sources = source_urls(url, expected_sha256);
    let dir = update_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(INSTALLER_FILENAME);
    // 起手清残留：旧的已就位安装包、半截 .tmp（旧版残留）、全部分段 part
    let _ = std::fs::remove_file(&dest);
    let _ = std::fs::remove_file(dir.join(format!("{}.tmp", INSTALLER_FILENAME)));
    clear_part_files(&dir);

    let stop = Arc::new(AtomicBool::new(false));
    let winner = Arc::new(AtomicUsize::new(usize::MAX));
    let states: Vec<Arc<SourceState>> = (0..sources.len())
        .map(|_| {
            Arc::new(SourceState {
                downloaded: AtomicU64::new(0),
                total: AtomicU64::new(0),
                done: AtomicBool::new(false),
            })
        })
        .collect();
    let part_path = |i: usize| dir.join(format!("{INSTALLER_FILENAME}.s{i}.part"));

    let snapshot = |stage: ProgressStage| {
        let sources = states
            .iter()
            .enumerate()
            .map(|(i, s)| SourceProgress {
                index: i,
                downloaded: s.downloaded.load(Ordering::Relaxed),
                total: s.total.load(Ordering::Relaxed),
            })
            .collect();
        DownloadProgress { stage, sources }
    };

    on_progress(snapshot(ProgressStage::Connecting));

    let results: Mutex<Vec<(usize, Result<String, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for (i, src) in sources.iter().enumerate() {
            let stop = Arc::clone(&stop);
            let winner = Arc::clone(&winner);
            let state = Arc::clone(&states[i]);
            let path = part_path(i);
            let on_progress = &*on_progress;
            let results = &results;
            scope.spawn(move || {
                let result = download_one(src, &path, expected_sha256, &state, &stop, on_progress);
                state.done.store(true, Ordering::Relaxed);
                // 只有通过校验的源才有资格 claim；失败/被超车的源静默出局
                if result.is_ok()
                    && winner
                        .compare_exchange(usize::MAX, i, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok()
                {
                    stop.store(true, Ordering::Relaxed);
                }
                results
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((i, result));
            });
        }
        // 进度心跳：节流下发快照；胜者诞生或全部源出局即停（借用捕获，
        // winner/states/snapshot 出 scope 后还要用，不能 move）
        scope.spawn(|| {
            loop {
                std::thread::sleep(PROGRESS_TICK);
                on_progress(snapshot(ProgressStage::Downloading));
                if winner.load(Ordering::Relaxed) != usize::MAX
                    || states.iter().all(|s| s.done.load(Ordering::Relaxed))
                {
                    break;
                }
            }
        });
    });
    let outcomes = results.into_inner().unwrap_or_else(|e| e.into_inner());

    let w = winner.load(Ordering::Relaxed);
    if w == usize::MAX {
        clear_part_files(&dir);
        let errs: Vec<String> = outcomes
            .iter()
            .filter_map(|(_, r)| r.as_ref().err().cloned())
            .collect();
        return Err(format!("all download sources failed: {}", errs.join(" | ")));
    }
    let hash = outcomes
        .iter()
        .find(|(i, _)| *i == w)
        .and_then(|(_, r)| r.as_ref().ok().cloned())
        .ok_or_else(|| "winner outcome missing".to_string())?;
    std::fs::rename(part_path(w), &dest).map_err(|e| e.to_string())?;
    clear_part_files(&dir);
    // 版本标记：启动清理按 version 判定装上了没有；sha256 供
    // 「立即安装」执行前复核（verified_installer）
    std::fs::write(
        dir.join(MARKER_FILENAME),
        serde_json::json!({ "version": version, "sha256": hash }).to_string(),
    )
    .map_err(|e| e.to_string())?;
    on_progress(snapshot(ProgressStage::Done));
    Ok(dest)
}

/// 清掉更新目录里全部分段残留（`.s<源>.part`）
fn clear_part_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(INSTALLER_FILENAME) && name.ends_with(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// 单源下载：首个 Range 请求兼作探测——206 拿 content-range 总大小后分段
/// 并行（预分配文件 + 偏移写），200 说明 Range 被忽略改单流直存。落盘后
/// 尺寸闸门 + 与官方哈希核对（有哈希时）才返回哈希值。
fn download_one(
    url: &str,
    path: &Path,
    expected_sha256: Option<&str>,
    state: &SourceState,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(DownloadProgress) + Send + Sync),
) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(REQUEST_TIMEOUT)
        .timeout_connect(CONNECT_TIMEOUT)
        // 读停滞超时挂在 agent 上（ureq 2 的请求级只支持总时限）：
        // 连续 N 秒一个字节读不到即废，比总时限更快的僵死判定
        .timeout_read(READ_STALL_TIMEOUT)
        .build();
    let resp = get_range_follow(&agent, url, 0, PROBE_BYTES - 1)?;
    match resp.status() {
        206 => {
            let (first_end, total) = parse_content_range(resp.header("content-range"))?;
            if total == 0 || total > MAX_INSTALLER_BYTES {
                return Err(format!("bad installer size: {} bytes", total));
            }
            state.total.store(total, Ordering::Relaxed);
            let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
            file.set_len(total).map_err(|e| e.to_string())?;
            drop(file);
            write_range_at(resp, path, 0, first_end, state, stop)?;
            std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for (start, end) in plan_ranges(total, first_end) {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let agent = &agent;
                    let state = &*state;
                    let stop = &*stop;
                    let path = &*path;
                    handles.push(scope.spawn(move || {
                        let resp = get_range_follow(agent, url, start, end)?;
                        write_range_at(resp, path, start, end, state, stop)
                    }));
                }
                let mut got = first_end + 1;
                for h in handles {
                    got += h
                        .join()
                        .map_err(|_| "segment thread panicked".to_string())??;
                }
                if got != total {
                    return Err(format!("short download: {got}/{total} bytes"));
                }
                Ok::<(), String>(())
            })?;
        }
        200 => {
            // Range 被忽略（部分镜像不支持）：单流直存，尺寸闸门照旧
            if let Some(len) = resp.header("content-length") {
                if let Ok(len) = len.parse::<u64>() {
                    if len > 0 && len <= MAX_INSTALLER_BYTES {
                        state.total.store(len, Ordering::Relaxed);
                    }
                }
            }
            stream_to_file(resp, path, MAX_INSTALLER_BYTES + 1, state, stop)?;
        }
        s => return Err(format!("unexpected status {s}")),
    }
    let got = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if got == 0 || got > MAX_INSTALLER_BYTES {
        return Err(format!("bad installer size: {got} bytes"));
    }
    on_progress(DownloadProgress {
        stage: ProgressStage::Verifying,
        sources: vec![],
    });
    let hash = sha256_file(path)?;
    if let Some(expected) = expected_sha256 {
        if !hash.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(path);
            return Err("installer checksum mismatch".to_string());
        }
    }
    Ok(hash)
}

/// 手工跟跳的 Range GET：每跳重发 Range/User-Agent；3xx 无 location 或
/// 超跳数即废。重定向目标只允许 http(s)——安装包下载域钉死在 GitHub 前缀，
/// 跳出去（如被镜像改写到 ftp/内网）一律拒。
fn get_range_follow(
    agent: &ureq::Agent,
    url: &str,
    start: u64,
    end: u64,
) -> Result<ureq::Response, String> {
    let mut cur = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let resp = agent
            .get(&cur)
            .set(
                "User-Agent",
                concat!("Driftlet/", env!("CARGO_PKG_VERSION")),
            )
            .set("Range", &format!("bytes={start}-{end}"))
            .timeout(REQUEST_TIMEOUT)
            .call()
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if (300..400).contains(&status) {
            let loc = resp
                .header("location")
                .ok_or_else(|| format!("redirect {status} without location"))?;
            cur = absolutize(&cur, loc)?;
            continue;
        }
        return Ok(resp);
    }
    Err(format!("more than {} redirects", MAX_REDIRECTS))
}

/// 相对 location（`/path`）补全成绝对 URL；其余形态原样返回（相对路径
/// 引用如 `./x` 按同域名根路径拼接）。
fn absolutize(current: &str, location: &str) -> Result<String, String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_string());
    }
    let scheme_end = current
        .find("://")
        .ok_or_else(|| format!("bad redirect base {current}"))?;
    let rest = &current[scheme_end + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let path = if location.starts_with('/') {
        location.to_string()
    } else {
        format!("/{location}")
    };
    Ok(format!("{}{}", &current[..scheme_end + 3 + host_end], path))
}

/// 解析 `content-range: bytes 0-262143/5242880` → (首个段末偏移, 总大小)。
/// `*/` 形态或缺头一律按错误处理。
fn parse_content_range(header: Option<&str>) -> Result<(u64, u64), String> {
    let h = header.ok_or_else(|| "missing content-range".to_string())?;
    let after_slash = h
        .rsplit('/')
        .next()
        .ok_or_else(|| format!("bad content-range {h}"))?;
    let total = after_slash
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("bad content-range {h}"))?;
    let range_part = h.split('/').next().unwrap_or("");
    let dash = range_part
        .split('-')
        .nth(1)
        .ok_or_else(|| format!("bad content-range {h}"))?;
    let first_end = dash
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("bad content-range {h}"))?;
    Ok((first_end, total))
}

/// 把（206）响应体写进 `path` 的 `[start, end]` 偏移区间，逐块核对长度；
/// `stop`（别的源已胜出）或短读即废。
fn write_range_at(
    resp: ureq::Response,
    path: &Path,
    start: u64,
    end: u64,
    state: &SourceState,
    stop: &AtomicBool,
) -> Result<u64, String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let want = end - start + 1;
    let mut reader = resp.into_reader();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut got = 0u64;
    while got < want {
        if stop.load(Ordering::Relaxed) {
            return Err("superseded by another source".to_string());
        }
        let chunk = buf.len().min((want - got) as usize);
        let n = Read::read(&mut reader, &mut buf[..chunk]).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        state.downloaded.fetch_add(n as u64, Ordering::Relaxed);
    }
    if got != want {
        return Err(format!("short read: {got}/{want} bytes"));
    }
    Ok(got)
}

/// 单流直存（200 响应）：尺寸上限闸门 + 进度计数
fn stream_to_file(
    resp: ureq::Response,
    path: &Path,
    cap: u64,
    state: &SourceState,
    stop: &AtomicBool,
) -> Result<u64, String> {
    use std::io::{Read, Write};
    let mut reader = Read::take(resp.into_reader(), cap);
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("superseded by another source".to_string());
        }
        let n = Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        state.downloaded.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(got)
}

/// 指定版本的安装包是否已就位且通过哈希核对（版本标记匹配 + 文件 SHA-256
/// 与标记一致）——check_update 据此让前端跳过重复下载（用户上次点
/// 「稍后」或下载完成后重启，不再重下整个安装包）。
pub fn installer_ready(update_dir: &Path, version: &str) -> bool {
    let dest = update_dir.join(INSTALLER_FILENAME);
    if !dest.is_file() {
        return false;
    }
    let Ok(marker) = std::fs::read_to_string(update_dir.join(MARKER_FILENAME)) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<serde_json::Value>(&marker) else {
        return false;
    };
    if marker.get("version").and_then(|v| v.as_str()) != Some(version) {
        return false;
    }
    let sha256 = marker.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
    if sha256.is_empty() {
        return false;
    }
    sha256_file(&dest)
        .map(|actual| actual.eq_ignore_ascii_case(sha256))
        .unwrap_or(false)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 「立即安装」执行前的核对：返回的安装包必须是我们自己下载的那一份——
/// 版本标记存在、标记版本新于当前运行版本、文件 SHA-256 与下载时记录的
/// 哈希一致，三者缺一即拒（用户走「重新下载」自愈，下载是自动的）。
/// 防的是可信动作（用户点「立即安装」）执行被第三方改写过的安装包：
/// 更新目录已入 file_system 禁写根（审查 H2 的正面修复），本核对是
/// 纵深——哈希一票否决，与改写途径无关（含旧版下载的无哈希标记：
/// fail closed）。
pub fn verified_installer(config_dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let dir = update_dir(config_dir);
    let dest = dir.join(INSTALLER_FILENAME);
    if !dest.is_file() {
        return Err("installer not downloaded yet".to_string());
    }
    let marker = std::fs::read_to_string(dir.join(MARKER_FILENAME))
        .map_err(|_| "download marker missing; please download again".to_string())?;
    let marker: serde_json::Value = serde_json::from_str(&marker).map_err(|e| e.to_string())?;
    let version = marker.get("version").and_then(|v| v.as_str()).unwrap_or("");
    let sha256 = marker.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
    if version.is_empty() || sha256.is_empty() {
        return Err("download marker incomplete; please download again".to_string());
    }
    if !is_newer(version, env!("CARGO_PKG_VERSION")) {
        return Err("downloaded installer is not newer than the running version".to_string());
    }
    let actual = sha256_file(&dest)?;
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err("installer checksum mismatch; please download again".to_string());
    }
    Ok(dest)
}

/// 流式计算文件 SHA-256（小写十六进制）
fn sha256_file(p: &std::path::Path) -> Result<String, String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
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

    /// verified_installer 的核对面（审查 H2）：无标记 / 标记缺 sha256 /
    /// 版本不新 / 哈希不符一律拒绝，全对才放行；放行后篡改文件再拒。
    #[test]
    fn verified_installer_checks_marker_and_hash() {
        let base = std::env::temp_dir().join(format!("driftlet-upd-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let config = base.join("config");
        let upd = super::update_dir(&config);
        std::fs::create_dir_all(&upd).unwrap();
        let installer = upd.join(super::INSTALLER_FILENAME);
        let marker = upd.join(super::MARKER_FILENAME);
        std::fs::write(&installer, b"fake-installer").unwrap();
        let good_sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(b"fake-installer");
            super::hex_lower(&h.finalize())
        };
        let future = "999.0.0";

        // 无版本标记 → 拒（安装包可能来自任何途径）
        assert!(super::verified_installer(&config).is_err());
        // 旧版下载的标记没有 sha256 字段 → fail closed（重新下载自愈）
        std::fs::write(
            &marker,
            serde_json::json!({ "version": future }).to_string(),
        )
        .unwrap();
        assert!(super::verified_installer(&config).is_err());
        // 哈希不符（安装包被改写）→ 拒
        std::fs::write(
            &marker,
            serde_json::json!({ "version": future, "sha256": "00" }).to_string(),
        )
        .unwrap();
        assert!(super::verified_installer(&config).is_err());
        // 标记版本不新于当前运行版本 → 拒（陈旧残留不应可执行）
        std::fs::write(
            &marker,
            serde_json::json!({ "version": "0.0.1", "sha256": good_sha }).to_string(),
        )
        .unwrap();
        assert!(super::verified_installer(&config).is_err());
        // 全部核对通过 → 放行（哈希大小写差异容忍）
        std::fs::write(
            &marker,
            serde_json::json!({ "version": future, "sha256": good_sha.to_uppercase() }).to_string(),
        )
        .unwrap();
        assert_eq!(super::verified_installer(&config).unwrap(), installer);
        // 放行后文件被改一个字节 → 哈希一票否决
        std::fs::write(&installer, b"fake-installer!").unwrap();
        assert!(super::verified_installer(&config).is_err());

        let _ = std::fs::remove_dir_all(&base);
    }

    /// release 说明里的官方 SHA-256 解析：认 `SHA256: <hex>` 行（大小写
    /// 皆可），40 位 commit SHA / 65 位长串 / 无 sha256 字样的 64 位串 /
    /// 64 位纯数字段一律不认。
    #[test]
    fn extract_installer_sha256_parses_marker_line() {
        use super::extract_installer_sha256;
        let sha = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678abcdef0123456789abcdef01";
        assert_eq!(
            extract_installer_sha256(&format!("## Driftlet 9.9.9\n\nSHA256: {sha}\n")),
            Some(sha.to_string())
        );
        // 大小写混合标记 + 反引号包裹 + 大写哈希 → 归一小写
        let upper = sha.to_uppercase();
        assert_eq!(
            extract_installer_sha256(&format!("sha256: `{upper}`")),
            Some(sha.to_string())
        );
        // 自动生成的 release 说明带 40 位 commit SHA：绝不能误当安装包哈希
        let body = "What's Changed\n* fix something in deadbeefcafebabe1337c0ffee1234567890abcd\n";
        assert_eq!(extract_installer_sha256(body), None);
        // 65 位十六进制长串（前缀 0 夹住的 64 位子串）不认
        let long = format!("0{sha}");
        assert_eq!(extract_installer_sha256(&format!("SHA256: {long}")), None);
        // 无 sha256 字样的行里躺 64 位串不认（防误伤）
        assert_eq!(extract_installer_sha256(sha), None);
        // 64 位纯数字（无字母）——真实 SHA-256 概率 ~3e-13，按误匹配拒
        let digits = "1".repeat(64);
        assert_eq!(extract_installer_sha256(&format!("SHA256: {digits}")), None);
        // 多行取首个命中
        let multi = format!("sha256sum: {sha}\nsha256: {sha}");
        assert_eq!(extract_installer_sha256(&multi), Some(sha.to_string()));
    }

    /// 候选源：直连永远第一且恒在；只有官方哈希在时镜像才进候选（fail
    /// closed——无哈希无校验手段，不信任第三方链路）。
    #[test]
    fn source_urls_gated_by_official_hash() {
        use super::source_urls;
        let url = "https://github.com/xiaochengzina/Driftlet/releases/download/v9.9.9/Driftlet_9.9.9_x64-setup.exe";
        let direct_only = source_urls(url, None);
        assert_eq!(direct_only, vec![url.to_string()]);
        let with_mirrors = source_urls(url, Some("ab"));
        assert_eq!(with_mirrors.len(), 1 + super::MIRRORS.len());
        assert_eq!(with_mirrors[0], url);
        for (m, u) in super::MIRRORS.iter().zip(with_mirrors.iter().skip(1)) {
            assert!(u.starts_with(m) && u.ends_with(url));
        }
    }

    /// 分段计划：探测段之后的剩余区间不重不漏、段数与最小段尺度有界。
    #[test]
    fn plan_ranges_covers_without_overlap() {
        use super::plan_ranges;
        // 文件不超出探测段：无剩余分段
        assert!(plan_ranges(100 * 1024, 100 * 1024 - 1).is_empty());
        assert!(plan_ranges(256 * 1024, 256 * 1024 - 1).is_empty());
        // 5MB 文件：8 段
        let total = 5 * 1024 * 1024;
        let first_end = 256 * 1024 - 1;
        let ranges = plan_ranges(total, first_end);
        assert_eq!(ranges.len(), 8);
        assert_eq!(ranges[0].0, first_end + 1);
        assert_eq!(ranges.last().unwrap().1, total - 1);
        for (a, b) in &ranges {
            assert!(a <= b);
        }
        for w in ranges.windows(2) {
            assert_eq!(w[0].1 + 1, w[1].0); // 连续不重不漏
        }
    }

    /// content-range 解析：正常头 / `*` 形态 / 缺头 / 坏头。
    #[test]
    fn parse_content_range_shapes() {
        use super::parse_content_range;
        assert_eq!(
            parse_content_range(Some("bytes 0-262143/5242880")),
            Ok((262143, 5242880))
        );
        assert!(parse_content_range(Some("bytes */5242880")).is_err());
        assert!(parse_content_range(None).is_err());
        assert!(parse_content_range(Some("garbage")).is_err());
    }

    /// 重定向绝对化：绝对 location 原样、相对 location 补 scheme+host。
    #[test]
    fn absolutize_redirect_locations() {
        use super::absolutize;
        let base = "https://github.com/xiaochengzina/Driftlet/releases/download/v9.9.9/x.exe";
        assert_eq!(
            absolutize(base, "https://objects.githubusercontent.com/x/y").unwrap(),
            "https://objects.githubusercontent.com/x/y"
        );
        assert_eq!(
            absolutize(
                base,
                "/xiaochengzina/Driftlet/releases/download/v9.9.9/x.exe"
            )
            .unwrap(),
            "https://github.com/xiaochengzina/Driftlet/releases/download/v9.9.9/x.exe"
        );
    }

    /// installer_ready：就位 + 版本匹配 + 哈希一致才算 true；版本不符 /
    /// 标记缺失 / 文件被改写 / 无 sha256 字段一律 false（与
    /// verified_installer 同纪律，fail closed）。
    #[test]
    fn installer_ready_requires_matching_verified_marker() {
        let base = std::env::temp_dir().join(format!("driftlet-ready-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let upd = base.join("update");
        std::fs::create_dir_all(&upd).unwrap();
        let installer = upd.join(super::INSTALLER_FILENAME);
        let marker = upd.join(super::MARKER_FILENAME);
        std::fs::write(&installer, b"fake-installer").unwrap();
        let sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(b"fake-installer");
            super::hex_lower(&h.finalize())
        };
        // 文件在、标记缺 → false
        assert!(!super::installer_ready(&upd, "9.9.9"));
        // 版本不符 → false
        std::fs::write(
            &marker,
            serde_json::json!({ "version": "8.8.8", "sha256": sha }).to_string(),
        )
        .unwrap();
        assert!(!super::installer_ready(&upd, "9.9.9"));
        // 文件被改写（哈希不符）→ false
        std::fs::write(
            &marker,
            serde_json::json!({ "version": "9.9.9", "sha256": sha }).to_string(),
        )
        .unwrap();
        std::fs::write(&installer, b"fake-installer!").unwrap();
        assert!(!super::installer_ready(&upd, "9.9.9"));
        // 全部一致 → true
        std::fs::write(&installer, b"fake-installer").unwrap();
        assert!(super::installer_ready(&upd, "9.9.9"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
