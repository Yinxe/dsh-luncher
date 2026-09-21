//! GitHub 加速：**前缀代理**（prefix proxy）。
//!
//! 用法就是把原链接原样拼在代理前缀后面，例如：
//!
//! ```text
//! git clone https://gh-proxy.com/https://github.com/owner/repo.git
//! curl      https://gh-proxy.com/https://github.com/owner/repo/releases/download/v1/x.tgz
//! ```
//!
//! 所以这一层只做三件事：
//! 1. **探测**：对候选前缀各发一次小请求（拿 GitHub520 的 hosts 当靶子，几十 KB），
//!    记录往返毫秒；只保留真正能取到内容的，按快慢排序。
//! 2. **缓存**：结果落盘 `~/.dsh-launcher/github-accel.json`，默认 6 小时内直接用。
//! 3. **改写**：把 github 链接（git clone/fetch/pull、releases 资产、raw、gist）
//!    拼上选中的前缀——git 走 `url.<prefix>https://github.com/.insteadOf`，
//!    因此已有的克隆执行 `git pull` 也能吃到加速，命令本身仍显示原始地址。
//!
//! 前缀清单 = 内置常用代理 + 用户在设置里额外添加的；谁快谁上，实测说了算。
//! 只对 github 域名生效，其它域名一律不动。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 内置候选前缀（可用率会变，所以每次探测都重新测速，不通的直接排除）
pub const DEFAULT_PROXIES: &[&str] = &[
    "https://gh-proxy.com/",
    "https://gh.xxooo.cf/",
    "https://gh.dpik.top/",
    "https://gh.927223.xyz/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
];

/// 测速靶子：GitHub520 的 hosts（小、稳定、不消耗 GitHub API 额度）
const PROBE_TARGET: &str = "https://raw.githubusercontent.com/521xueweihan/GitHub520/main/hosts";

/// git 能力测速用的极小仓库（只取 tree，不取文件内容）
const GIT_PROBE_REPO: &str = "521xueweihan/GitHub520";

/// 缓存有效期：6 小时
pub const CACHE_TTL_SECS: u64 = 6 * 3600;
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const GIT_PROBE_TIMEOUT: Duration = Duration::from_secs(45);

/// 一个可用前缀及其测速耗时
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyNode {
    pub prefix: String,
    /// 文件下载测速：GET 一个小文件（靶子）的往返毫秒
    pub ms: u64,
    /// git 测速：真实 `git clone --filter=blob:none` 的毫秒；None = 这个前缀不支持 git
    /// （不少代理只放行文件下载，clone 会 403，所以要分开测）
    #[serde(default)]
    pub git_ms: Option<u64>,
}

impl ProxyNode {
    pub fn supports_git(&self) -> bool {
        self.git_ms.is_some()
    }
}

/// 加速状态：可用前缀 + 测速结果
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GhAccel {
    /// 秒级时间戳
    pub updated_at: u64,
    /// 按快慢排序（第一个就是自动模式用的）
    pub nodes: Vec<ProxyNode>,
    /// builtin | builtin+extra | cache
    pub source: String,
    /// 是否来自磁盘缓存
    #[serde(default)]
    pub cached: bool,
}

impl GhAccel {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn age_secs(&self) -> u64 {
        now_unix().saturating_sub(self.updated_at)
    }

    pub fn is_fresh(&self, ttl: u64) -> bool {
        !self.nodes.is_empty() && self.age_secs() < ttl
    }

    pub fn find(&self, prefix: &str) -> Option<&ProxyNode> {
        let want = normalize_prefix(prefix)?;
        self.nodes.iter().find(|n| n.prefix == want)
    }
}

// ── 全局状态 ────────────────────────────────────────────────

fn slot() -> &'static Mutex<Option<GhAccel>> {
    static SLOT: OnceLock<Mutex<Option<GhAccel>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 当前生效的加速状态（可能来自缓存）
pub fn current() -> Option<GhAccel> {
    slot().lock().ok().and_then(|g| g.clone())
}

pub fn set_current(a: GhAccel) {
    if let Ok(mut g) = slot().lock() {
        *g = Some(a);
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn cache_path() -> PathBuf {
    crate::settings::launcher_home().join("github-accel.json")
}

pub fn load_cache() -> Option<GhAccel> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    let mut a: GhAccel = serde_json::from_str(&text).ok()?;
    a.cached = true;
    if a.source.is_empty() {
        a.source = "cache".into();
    }
    Some(a)
}

pub fn save_cache(a: &GhAccel) -> Result<(), String> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(a).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("写入加速缓存失败: {e}"))
}

/// 同步拿一份能用的状态：新鲜缓存直接用；过期的也先用着（后台会刷新）
pub fn ensure_cached(ttl: u64) -> Option<GhAccel> {
    if let Some(cur) = current() {
        if cur.is_fresh(ttl) {
            return Some(cur);
        }
    }
    let cached = load_cache()?;
    set_current(cached.clone());
    Some(cached)
}

/// 后台刷新一次（不阻塞调用方）
pub fn spawn_refresh(extra: String, force: bool) {
    tauri::async_runtime::spawn(async move {
        let _ = refresh(&extra, force).await;
    });
}

// ── 候选前缀 ────────────────────────────────────────────────

/// 规范化：必须是 http(s)://，并保证以 `/` 结尾
pub fn normalize_prefix(s: &str) -> Option<String> {
    let t = s.trim();
    if !(t.starts_with("http://") || t.starts_with("https://")) {
        return None;
    }
    if t.len() < 12 {
        return None;
    }
    Some(if t.ends_with('/') { t.to_string() } else { format!("{t}/") })
}

/// 候选清单：内置 + 用户额外添加（逗号 / 空格 / 分号 / 换行分隔）
pub fn candidates(extra: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in DEFAULT_PROXIES {
        if let Some(n) = normalize_prefix(p) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    for part in extra.split(['\n', ',', ' ', '\t', ';']) {
        if let Some(n) = normalize_prefix(part) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// 是否值得走代理的 github 链接
pub fn is_github_url(url: &str) -> bool {
    // 只做「是不是 github 主机」的判定，不返回改写结果，故整体小写化后比较即可
    // （主机名与 scheme 都大小写不敏感）。
    let u = url.trim().to_ascii_lowercase();
    if let Some(rest) = u.strip_prefix("git@github.com:") {
        return !rest.is_empty();
    }
    let Some(after_scheme) = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
    else {
        return false;
    };
    // 主机名到第一个 '/'、'?'、'#' 或 ':'（端口）为止。此前用带裸 "https://github.com"
    // 前缀的 starts_with 会把 https://github.com.evil.test/... 误判成 github 链接、
    // 一并丢给第三方代理，这里改成主机名精确匹配。
    let host = after_scheme.split(['/', '?', '#', ':']).next().unwrap_or("");
    matches!(
        host,
        "github.com"
            | "raw.githubusercontent.com"
            | "gist.githubusercontent.com"
            | "codeload.github.com"
            | "objects.githubusercontent.com"
    )
}

/// 把 github 链接拼上前缀；不是 github 链接（或已带前缀）就原样返回
pub fn rewrite(url: &str, prefix: &str) -> String {
    let u = url.trim();
    let Some(p) = normalize_prefix(prefix) else {
        return u.to_string();
    };
    if u.starts_with(&p) {
        return u.to_string();
    }
    if let Some(rest) = u.strip_prefix("git@github.com:") {
        let rest = rest.trim_end_matches(".git");
        return format!("{p}https://github.com/{rest}.git");
    }
    if is_github_url(u) {
        return format!("{p}{u}");
    }
    u.to_string()
}

/// 最快且满足能力要求的前缀
pub fn best_for(accel: &GhAccel, need_git: bool) -> Option<&ProxyNode> {
    if need_git {
        accel.nodes.iter().find(|n| n.supports_git())
    } else {
        accel.nodes.first()
    }
}

/// 选定实际使用的前缀：per-job/设置指定 > 自动（最快）
///
/// - `preferred` 为 `Some("")` 表示「本次不用加速」→ None
/// - `need_git`：git 操作必须挑支持 git 的前缀（只放行下载的代理会 403）；
///   若显式指定的前缀不支持 git，则退回自动挑一个支持的，避免把 clone 弄失败
pub fn pick_prefix(accel: &GhAccel, preferred: Option<&str>, need_git: bool) -> Option<String> {
    match preferred {
        Some(p) if p.trim().is_empty() => None,
        Some(p) => {
            let want = normalize_prefix(p)?;
            match accel.find(&want) {
                Some(n) if !need_git || n.supports_git() => Some(n.prefix.clone()),
                // 指定的前缀不支持 git（或没测过）：退回能满足要求的最快前缀
                _ => best_for(accel, need_git).map(|n| n.prefix.clone()),
            }
        }
        None => best_for(accel, need_git).map(|n| n.prefix.clone()),
    }
}

// ── 测速 + 刷新 ─────────────────────────────────────────────

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .user_agent("dsh-launcher")
        .build()
        .map_err(|e| format!("构造 HTTP 客户端失败: {e}"))
}

/// 文件下载测速：GET 靶子，返回毫秒（不可用返回 None）
pub async fn probe_download(client: &reqwest::Client, prefix: &str) -> Option<u64> {
    let url = format!("{prefix}{PROBE_TARGET}");
    let t0 = Instant::now();
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // 必须真把内容读回来：有些代理是先返回 200 再慢慢吐字节
    let body = resp.text().await.ok()?;
    if body.trim().is_empty() {
        return None;
    }
    Some(t0.elapsed().as_millis() as u64)
}

/// git 能力测速：真克隆一个极小的仓库（只取 commit/tree，不取文件内容、不检出），
/// 返回毫秒；不支持 git 的前缀（常见：只做文件下载）返回 None。
pub fn probe_git(prefix: &str, tag: usize) -> Option<u64> {
    let url = format!("{prefix}https://github.com/{GIT_PROBE_REPO}.git");
    let dir = std::env::temp_dir().join(format!("dsh-ghaccel-git-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let args: Vec<String> = vec![
        "clone".into(),
        "--quiet".into(),
        "--depth".into(),
        "1".into(),
        "--filter=blob:none".into(),
        "--no-checkout".into(),
        url,
        dir.to_string_lossy().into_owned(),
    ];
    let t0 = Instant::now();
    let out = crate::util::run_captured_in(
        std::path::Path::new("git"),
        &args,
        None,
        &[("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())],
        GIT_PROBE_TIMEOUT,
    );
    let ms = t0.elapsed().as_millis() as u64;
    let _ = std::fs::remove_dir_all(&dir);
    match out {
        Some((true, _)) => Some(ms),
        _ => None,
    }
}

/// 刷新：并发测速所有候选前缀 → 排序 → 落盘 + 置为当前
pub async fn refresh(extra: &str, force: bool) -> Result<GhAccel, String> {
    if !force {
        if let Some(cur) = current() {
            if cur.is_fresh(CACHE_TTL_SECS) {
                return Ok(cur);
            }
        }
    }
    let list = candidates(extra);
    if list.is_empty() {
        return Err("没有可用的候选代理前缀".into());
    }
    let candidates_count = list.len();
    crate::diag::info(
        "network",
        &format!("GitHub 加速测速开始：{candidates_count} 个候选前缀（各测下载 + git）"),
    );
    let client = client()?;
    let mut tasks = Vec::new();
    for (i, p) in list.into_iter().enumerate() {
        let c = client.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            let ms = probe_download(&c, &p).await?;
            // git 能力单独测（阻塞的 git 子进程丢到 blocking 池）
            let p2 = p.clone();
            let git_ms = tauri::async_runtime::spawn_blocking(move || probe_git(&p2, i))
                .await
                .ok()
                .flatten();
            Some(ProxyNode { prefix: p, ms, git_ms })
        }));
    }
    let mut nodes: Vec<ProxyNode> = Vec::new();
    for t in tasks {
        if let Ok(Some(n)) = t.await {
            nodes.push(n);
        }
    }
    if nodes.is_empty() {
        crate::diag::warn(
            "network",
            &format!("GitHub 加速测速：{} 个候选前缀全部失败（网络不通或前缀已失效）", candidates_count),
        );
        return Err("所有候选代理都取不到内容（网络不通或前缀已失效）".into());
    }
    // 文件下载快的排前面（自动模式即取第一）；git 另用 best_for 挑第一个支持 git 的
    nodes.sort_by(|a, b| a.ms.cmp(&b.ms).then(a.prefix.cmp(&b.prefix)));
    crate::diag::info(
        "network",
        &format!(
            "GitHub 加速测速结果：{}",
            nodes
                .iter()
                .take(6)
                .map(|n| format!(
                    "{}（下载 {}ms{}）",
                    n.prefix,
                    n.ms,
                    n.git_ms.map(|g| format!("，git {g}ms")).unwrap_or_else(|| "，git 不可用".into())
                ))
                .collect::<Vec<_>>()
                .join("、")
        ),
    );
    let accel = GhAccel {
        updated_at: now_unix(),
        nodes,
        source: if extra.trim().is_empty() { "builtin".into() } else { "builtin+extra".into() },
        cached: false,
    };
    let _ = save_cache(&accel);
    set_current(accel.clone());
    Ok(accel)
}

// ── 注入 ────────────────────────────────────────────────────

/// 给 git 子进程的 `insteadOf` 环境变量：已有克隆执行 pull 也会走代理
pub fn git_env(accel: &GhAccel, preferred: Option<&str>) -> Vec<(String, String)> {
    match pick_prefix(accel, preferred, true) {
        Some(prefix) => git_env_for(&prefix),
        None => Vec::new(),
    }
}

/// 已知前缀时的同一份环境变量。
///
/// 单独暴露是为了让**日志与 git 用的是同一个前缀**：调用方先 `pick_prefix` 一次，
/// 既拿它写「实际请求 <url> → <改写后地址>」，又拿它构造 env —— 两边各算一次的话，
/// 将来任何一处改了挑选规则都会出现「日志写 A、git 走 B」。
///
/// ⚠️ 只覆盖 `https://github.com/`：`git@github.com:owner/repo`（SSH）不在
/// `insteadOf` 的匹配范围里，想走加速必须换成 https 地址（`rewrite` 只用于下载直链）。
pub fn git_env_for(prefix: &str) -> Vec<(String, String)> {
    let Some(prefix) = normalize_prefix(prefix) else {
        return Vec::new();
    };
    vec![
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        (
            "GIT_CONFIG_KEY_0".into(),
            format!("url.{prefix}https://github.com/.insteadOf"),
        ),
        ("GIT_CONFIG_VALUE_0".into(), "https://github.com/".into()),
    ]
}

/// 一行摘要（写进任务日志 / 界面）
pub fn summary(accel: &GhAccel, preferred: Option<&str>, need_git: bool) -> String {
    match pick_prefix(accel, preferred, need_git) {
        Some(p) => {
            let node = accel.find(&p);
            let ms = match (need_git, node.and_then(|n| n.git_ms)) {
                (true, Some(g)) => format!("git {g}ms"),
                _ => format!("下载 {}ms", node.map(|n| n.ms).unwrap_or(0)),
            };
            format!("{p}（{ms}，候选 {} 个）", accel.nodes.len())
        }
        None => "未启用".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accel() -> GhAccel {
        GhAccel {
            updated_at: now_unix(),
            source: "test".into(),
            cached: false,
            nodes: vec![
                ProxyNode { prefix: "https://fast.example/".into(), ms: 80, git_ms: None },
                ProxyNode { prefix: "https://slow.example/".into(), ms: 900, git_ms: Some(700) },
            ],
        }
    }

    #[test]
    fn normalize_and_candidates() {
        assert_eq!(normalize_prefix("https://a.example").unwrap(), "https://a.example/");
        assert_eq!(normalize_prefix("https://a.example/").unwrap(), "https://a.example/");
        assert!(normalize_prefix("ftp://a.example").is_none());
        assert!(normalize_prefix("a.example").is_none());

        let list = candidates("https://mine.example\nhttps://gh-proxy.com/ , bad");
        assert!(list.contains(&"https://mine.example/".to_string()));
        // 与内置重复的只留一个
        assert_eq!(list.iter().filter(|p| *p == "https://gh-proxy.com/").count(), 1);
        assert!(list.iter().all(|p| p.starts_with("https://")));
    }

    /// insteadOf 的 key 形态很容易写坏（一个字符不对 git 就整串忽略、静默直连），
    /// 所以把三件套逐字钉住 —— 这条断言就是「加速到底有没有生效」的第一道防线。
    #[test]
    fn git_env_pins_the_insteadof_key() {
        let env: std::collections::HashMap<_, _> =
            git_env_for("https://gh-proxy.com").into_iter().collect();
        assert_eq!(env.get("GIT_CONFIG_COUNT").map(String::as_str), Some("1"));
        assert_eq!(
            env.get("GIT_CONFIG_KEY_0").map(String::as_str),
            Some("url.https://gh-proxy.com/https://github.com/.insteadOf")
        );
        assert_eq!(
            env.get("GIT_CONFIG_VALUE_0").map(String::as_str),
            Some("https://github.com/")
        );
        // 非法前缀 → 不注入任何变量（宁可不加速，也不能给 git 塞坏配置）
        assert!(git_env_for("a.example").is_empty());
        assert!(git_env_for("  ").is_empty());
    }

    #[test]
    fn rewrite_prefixes_only_github() {
        let p = "https://gh-proxy.com/";
        assert_eq!(
            rewrite("https://github.com/o/r/releases/download/v1/x.tgz", p),
            "https://gh-proxy.com/https://github.com/o/r/releases/download/v1/x.tgz"
        );
        assert_eq!(
            rewrite("https://raw.githubusercontent.com/o/r/main/f.txt", p),
            "https://gh-proxy.com/https://raw.githubusercontent.com/o/r/main/f.txt"
        );
        // 非 github 一律不动
        assert_eq!(rewrite("https://registry.npmjs.org/x", p), "https://registry.npmjs.org/x");
        // 已带前缀不重复拼
        let once = rewrite("https://github.com/o/r", p);
        assert_eq!(rewrite(&once, p), once);
        // ssh 形式转成 https 再拼
        assert_eq!(
            rewrite("git@github.com:o/r.git", p),
            "https://gh-proxy.com/https://github.com/o/r.git"
        );
    }

    #[test]
    fn github_host_matched_exactly_not_by_prefix() {
        // 真正的 github 主机：带不带路径、带端口、大写都要认
        assert!(is_github_url("https://github.com"));
        assert!(is_github_url("https://github.com/o/r"));
        assert!(is_github_url("https://github.com:443/o/r"));
        assert!(is_github_url("HTTPS://GitHub.com/o/r"));
        assert!(is_github_url("https://raw.githubusercontent.com/o/r/f"));
        assert!(is_github_url("git@github.com:o/r.git"));
        // 伪装成 github 的相似主机：绝不认，避免被丢给第三方代理
        assert!(!is_github_url("https://github.com.evil.test/x"));
        assert!(!is_github_url("https://evilgithub.com/x"));
        assert!(!is_github_url("https://github.como/x"));
        assert!(!is_github_url("git@github.com.evil.test:o/r.git"));
        assert!(!is_github_url("https://example.invalid/https://github.com/x"));
        // 相似主机也不会被改写
        let p = "https://gh-proxy.com/";
        assert_eq!(rewrite("https://github.com.evil.test/x", p), "https://github.com.evil.test/x");
    }

    #[test]
    fn pick_prefix_respects_capability() {
        let a = accel();
        // 下载：自动 → 最快
        assert_eq!(pick_prefix(&a, None, false).unwrap(), "https://fast.example/");
        // git：自动 → 第一个支持 git 的（最快那个只支持下载）
        assert_eq!(pick_prefix(&a, None, true).unwrap(), "https://slow.example/");
        // 显式指定的前缀不支持 git → 退回能满足要求的最快前缀
        assert_eq!(pick_prefix(&a, Some("https://fast.example/"), true).unwrap(), "https://slow.example/");
        // 显式指定为「不用」
        assert!(pick_prefix(&a, Some(""), false).is_none());
    }

    #[test]
    fn git_env_uses_instead_of() {
        let envs = git_env(&accel(), None);
        assert_eq!(envs[0], ("GIT_CONFIG_COUNT".to_string(), "1".to_string()));
        // git 环境变量用支持 git 的那个前缀
        assert_eq!(
            envs[1].1,
            "url.https://slow.example/https://github.com/.insteadOf".to_string()
        );
        assert_eq!(envs[2].1, "https://github.com/".to_string());
        // 关掉加速 → 不注入任何东西
        assert!(git_env(&accel(), Some("")).is_empty());
    }

    #[test]
    fn freshness() {
        let mut a = accel();
        assert!(a.is_fresh(60));
        a.updated_at = now_unix().saturating_sub(7200);
        assert!(!a.is_fresh(60));
    }

    /// 真网络：并发测前几个内置前缀的下载与 git 能力（全都不通时跳过，不判失败）
    #[test]
    fn builtin_prefixes_smoke() {
        let list: Vec<String> = DEFAULT_PROXIES
            .iter()
            .take(3)
            .filter_map(|p| normalize_prefix(p))
            .collect();
        let handles: Vec<_> = list
            .into_iter()
            .enumerate()
            .map(|(i, p)| {
                std::thread::spawn(move || {
                    let client = client().ok()?;
                    let dl = tauri::async_runtime::block_on(probe_download(&client, &p))?;
                    let git = probe_git(&p, i);
                    Some(format!(
                        "{p} 下载 {dl}ms · git {}",
                        git.map(|g| format!("{g}ms")).unwrap_or_else(|| "不支持".into())
                    ))
                })
            })
            .collect();
        let out: Vec<String> = handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect();
        if out.is_empty() {
            eprintln!("跳过：内置前缀当前都不可达（网络原因）");
            return;
        }
        println!("{}", out.join(" | "));
    }
}
