use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PKG_SCOPE_ENCODED: &str = "@deepseek-ai%2Fdsh";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteVersion {
    pub version: String,
    /// latest / next / alpha / pre-release / stable
    pub channel: String,
    /// 命中该版本的官方 dist-tags，如 ["latest"]
    pub tags: Vec<String>,
    pub published_at: Option<String>,
    pub description: Option<String>,
    pub unpacked_size: Option<u64>,
}

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RegistryInfo {
    /// dist-tags：latest → 0.1.5-rc.2 等
    pub tags: BTreeMap<String, String>,
    /// 按版本号从新到旧排序
    pub versions: Vec<RemoteVersion>,
}

fn derive_channel(version: &str) -> &'static str {
    let pre = version.split_once('-').map(|(_, p)| p).unwrap_or("");
    if pre.starts_with("alpha") {
        "alpha"
    } else if pre.starts_with("beta") {
        "beta"
    } else if pre.starts_with("rc") {
        "rc"
    } else {
        "stable"
    }
}

/// 从 npm registry 拉取 @deepseek-ai/dsh 的完整 packument 并提取版本信息
pub async fn fetch_registry(registry_base: &str) -> Result<RegistryInfo, String> {
    let base = registry_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("registry 地址为空".into());
    }
    let url = format!("{base}/{PKG_SCOPE_ENCODED}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("请求 registry 失败（{url}）: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("registry 返回 {status}（{url}）"));
    }
    let pack: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 registry 响应失败: {e}"))?;

    let mut tags: BTreeMap<String, String> = BTreeMap::new();
    if let Some(dt) = pack.get("dist-tags").and_then(|v| v.as_object()) {
        for (k, v) in dt {
            if let Some(ver) = v.as_str() {
                tags.insert(k.clone(), ver.to_string());
            }
        }
    }

    let times: BTreeMap<String, String> = pack
        .get("time")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut versions = Vec::new();
    if let Some(vs) = pack.get("versions").and_then(|v| v.as_object()) {
        for (ver, meta) in vs {
            let mut tag_hits: Vec<String> = tags
                .iter()
                .filter(|(_, v)| v.as_str() == ver.as_str())
                .map(|(k, _)| k.clone())
                .collect();
            tag_hits.sort();

            let channel = if let Some(t) = tag_hits.iter().find(|t| t.as_str() == "latest") {
                t.clone()
            } else if let Some(t) = tag_hits.first() {
                t.clone()
            } else {
                derive_channel(ver).to_string()
            };

            versions.push(RemoteVersion {
                version: ver.clone(),
                channel,
                tags: tag_hits,
                published_at: times.get(ver).cloned(),
                description: meta
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                unpacked_size: meta
                    .get("dist")
                    .and_then(|d| d.get("unpackedSize"))
                    .and_then(|v| v.as_u64()),
            });
        }
    }

    if versions.is_empty() {
        return Err("registry 响应中没有版本数据".into());
    }
    versions.sort_by(|a, b| crate::semver::compare(&b.version, &a.version));

    Ok(RegistryInfo { tags, versions })
}

// ── 插件包搜索 / GitHub 仓库预览（安装对话框数据源） ─────────────

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackageSearchItem {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    /// 最近发布时间（ISO 8601）
    pub published_at: Option<String>,
    /// npm 包页面链接
    pub link: Option<String>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepoInfo {
    pub full_name: String,
    pub description: Option<String>,
    pub stars: u64,
    pub pushed_at: Option<String>,
    pub html_url: String,
    pub license: Option<String>,
    /// 实际用于探测/安装的 ref（缺省 = 仓库默认分支）
    pub git_ref: Option<String>,
    /// 仓库默认分支
    pub default_branch: Option<String>,
    /// 仓库是否是 monorepo（声明了 pnpm-workspace / workspaces）
    pub is_monorepo: bool,
    /// workspace 成员 glob（如 ["plugins/*"]）
    pub workspace_globs: Vec<String>,
    /// 探测到的插件候选包（含仓库根；monorepo 下逐个 workspace 子包）
    pub candidates: Vec<PluginCandidate>,
    /// 探测方式：tree（Git Trees API 全量扫描）| contents（降级为单目录校验）
    pub probe: String,
    /// 有候选的 package.json 因超时未读取（名称/版本/描述缺失，其余信息仍有效）
    pub facts_pending: bool,
    /// 仓库元数据（stars/license/描述）因 GitHub API 额度不可用而缺失
    pub meta_degraded: bool,
    /// 插件在仓库内的路径（首个候选，兼容旧字段）
    pub plugin_path: Option<String>,
    /// lib/ 目录校验：Some(true)=已确认存在 Some(false)=确认缺失 None=未校验（打包产物直装）
    pub lib_ok: Option<bool>,
    /// 最终交给 dsh plugin add 的安装规格（github:owner/repo#ref&path:xx 或打包产物 URL）
    pub install_spec: String,
}

/// 探测到的一个可安装插件包（仓库根 / monorepo 子包 / 本地目录子包共用）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginCandidate {
    /// 相对插件根目录的路径；"" = 根目录
    pub path: String,
    /// package.json 的 name（读不到时为 None）
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// lib/ 目录存在且含构建产物（.js/.cjs/.mjs）
    pub lib_ok: bool,
    /// package.json 声明了 dsh.bundle.patch —— dsh 会把它并入 profile 层
    pub has_bundle: bool,
    /// 插件根目录内有 cordis.patch.yml
    pub has_patch: bool,
    /// 命中 pnpm-workspace / workspaces 成员 glob
    pub workspace_member: bool,
    /// 可以直接作为插件安装（声明了 dsh.bundle）
    pub ready: bool,
    /// 交给 dsh plugin add 的安装规格
    pub install_spec: String,
}

impl PluginCandidate {
    fn root() -> Self {
        PluginCandidate {
            path: String::new(),
            name: None,
            version: None,
            description: None,
            lib_ok: false,
            has_bundle: false,
            has_patch: false,
            workspace_member: false,
            ready: false,
            install_spec: String::new(),
        }
    }
}

/// 解析出的 GitHub 插件来源
#[derive(Clone, Debug, Default)]
pub struct GitHubPluginSpec {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
    pub plugin_path: Option<String>,
    /// 非空 = 打包产物直装模式
    pub tarball_url: Option<String>,
}

/// package.json 中与插件探测相关的字段
#[derive(Clone, Debug, Default)]
pub struct PkgFacts {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// dsh.bundle.patch 声明的补丁文件（相对路径）
    pub bundle_patch: Option<String>,
    /// package.json 里声明的 workspace 成员 glob（monorepo 根）
    pub workspaces: Vec<String>,
}

/// 从 package.json 文本提取探测相关字段
pub fn parse_pkg_facts(text: &str) -> PkgFacts {
    let Ok(pkg) = serde_json::from_str::<serde_json::Value>(text) else {
        return PkgFacts::default();
    };
    let str_at = |v: &serde_json::Value, k: &str| {
        v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
    };
    PkgFacts {
        name: str_at(&pkg, "name"),
        version: str_at(&pkg, "version"),
        description: str_at(&pkg, "description"),
        bundle_patch: pkg
            .get("dsh")
            .and_then(|d| d.get("bundle"))
            .and_then(|b| b.get("patch"))
            .and_then(|p| p.as_str())
            .map(String::from),
        workspaces: parse_workspaces_field(&pkg),
    }
}

/// npm/yarn workspaces 字段：`["packages/*"]` 或 `{ "packages": [...] }`
fn parse_workspaces_field(pkg: &serde_json::Value) -> Vec<String> {
    let ws = pkg.get("workspaces");
    let arr = match ws {
        Some(serde_json::Value::Array(a)) => Some(a),
        Some(serde_json::Value::Object(o)) => o.get("packages").and_then(|p| p.as_array()),
        _ => None,
    };
    arr.map(|a| {
        a.iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

/// 解析 pnpm-workspace.yaml 的 `packages:` 列表（忽略 ! 取反项以外的其他字段）
pub fn parse_pnpm_workspace(text: &str) -> Vec<String> {
    let Ok(v) = serde_yaml::from_str::<serde_yaml::Value>(text) else {
        return Vec::new();
    };
    v.get("packages")
        .and_then(|p| p.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|s| s.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

// ── glob 匹配（pnpm workspace 成员判定） ────────────────────

/// 单段通配：`*` 任意字符、`?` 单字符
fn match_segment(pat: &str, seg: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();
    // 经典双指针回溯
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = si;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// workspace glob 匹配：支持 `*` / `?` / `**`（跨目录）与结尾 `/`
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim().trim_end_matches('/');
    let path = path.trim().trim_matches('/');
    if pat.is_empty() {
        return false;
    }
    let p: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let s: Vec<&str> = if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').filter(|x| !x.is_empty()).collect()
    };
    fn rec(p: &[&str], s: &[&str]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(&"**") => {
                // `**` 吃掉 0..n 段
                (0..=s.len()).any(|k| rec(&p[1..], &s[k..]))
            }
            Some(&seg) => match s.first() {
                Some(&first) if match_segment(seg, first) => rec(&p[1..], &s[1..]),
                _ => false,
            },
        }
    }
    rec(&p, &s)
}

/// 相对路径是否命中任一 workspace glob（支持 `!` 取反）
pub fn workspace_hit(globs: &[String], rel: &str) -> bool {
    let mut hit = false;
    for g in globs {
        let g = g.trim();
        if let Some(neg) = g.strip_prefix('!') {
            if glob_match(neg, rel) {
                return false;
            }
        } else if glob_match(g, rel) {
            hit = true;
        }
    }
    hit
}

#[cfg(test)]
mod glob_tests {
    use super::*;

    #[test]
    fn glob_matches_workspace_shapes() {
        assert!(glob_match("plugins/*", "plugins/mcwiki-search"));
        assert!(!glob_match("plugins/*", "plugins/a/b"));
        assert!(glob_match("plugins/**", "plugins/a/b"));
        assert!(glob_match("packages/*/plugins/*", "packages/a/plugins/b"));
        assert!(glob_match("apps/**/plugins/**", "apps/a/x/plugins/b/c"));
        // 目录本身（无子段）不构成 workspace 成员
        assert!(!glob_match("plugins/*", "plugins/"));
        assert!(!glob_match("plugins/*", "plugins"));
        assert!(!glob_match("plugins/*", "other/x"));
        assert!(glob_match("*", "root-pkg"));
    }

    #[test]
    fn workspace_hit_honours_negation() {
        let globs = vec!["plugins/*".to_string(), "!plugins/skip".to_string()];
        assert!(workspace_hit(&globs, "plugins/keep"));
        assert!(!workspace_hit(&globs, "plugins/skip"));
        assert!(!workspace_hit(&globs, "elsewhere/x"));
    }

    #[test]
    fn parses_pkg_and_workspace_files() {
        let f = parse_pkg_facts(
            r#"{"name":"@dshp/x","version":"1.0.0","description":"d","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        );
        assert_eq!(f.name.as_deref(), Some("@dshp/x"));
        assert_eq!(f.bundle_patch.as_deref(), Some("./cordis.patch.yml"));
        let f2 = parse_pkg_facts(r#"{"workspaces":["packages/*"]}"#);
        assert_eq!(f2.workspaces, vec!["packages/*".to_string()]);
        let f3 = parse_pkg_facts(r#"{"workspaces":{"packages":["a","b"]}}"#);
        assert_eq!(f3.workspaces.len(), 2);
        assert_eq!(
            parse_pnpm_workspace("packages:\n  - 'plugins/*'\n  - shared\n"),
            vec!["plugins/*".to_string(), "shared".to_string()]
        );
    }
}

fn http_client() -> Result<reqwest::Client, String> {
    // 30s：只给"值得等"的调用（registry packument、元数据）用
    client_with(Duration::from_secs(30)).map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))
}

fn client_with(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))
}

/// github.com 的 git 智能 HTTP：**必须短超时**。
///
/// 本机实测该域名间歇性挂起（同一分钟内一次 0.5s、一次 40s 无响应），
/// 而这条通道只是"免额度优先"的一档，失败要立刻让位给 API/CDN，
/// 不能把 30s 的超时压在下游每一次提交比对、每一次探测前面。
fn refs_client() -> reqwest::Client {
    client_with(Duration::from_secs(6)).unwrap_or_default()
}

/// 免额度 CDN / 探测用：12s，比内容抓取宽松一点（树可能较大）
fn probe_client() -> reqwest::Client {
    client_with(Duration::from_secs(12)).unwrap_or_default()
}

// ── 通道健康度 / 熔断 ───────────────────────────────────────
//
// 这台机器到 github.com 是**间歇性不可达**（实测同一分钟内一次 0.5s、一次 40s 超时）。
// 每次操作都去撞一遍会很难受，所以给每条免额度通道记连续失败次数：
// 连续失败到阈值就**短暂停用**该通道，让调用方直接走下一档，避免每次都白等一个超时。
// 计时到点自动恢复（半开），任何一个成功都会把计数清零。
//
// 注意这是**本地进程内的启发式**，不是把某个域名永久拉黑：窗口只有几分钟。

#[derive(Clone, Copy)]
struct ChannelState {
    failures: u32,
    disable_until: Option<Instant>,
    /// 最近一次成功的往返毫秒（EWMA），用于"按实测选更快的通道"。
    /// 本机实测 api 0.6s、jsDelivr 5~9s、github.com/raw 直接超时——
    /// 固定顺序在这种网络上一定是错的。
    latency_ms: Option<u64>,
}

impl Default for ChannelState {
    fn default() -> Self {
        ChannelState {
            failures: 0,
            disable_until: None,
            latency_ms: None,
        }
    }
}

/// 连续失败到 2 次就停用；窗口 5 分钟后自动半开重试
const CHANNEL_FAIL_THRESHOLD: u32 = 2;
const CHANNEL_DISABLE_SECS: u64 = 300;

static CHANNELS: Mutex<Option<std::collections::HashMap<&'static str, ChannelState>>> =
    Mutex::new(None);

/// 通道名（同时用于自检展示）
pub const CH_REF: &str = "github-refs"; // github.com 的 git 智能 HTTP（免额度）
pub const CH_JSDELIVR: &str = "jsdelivr"; // data.jsdelivr.com + cdn.jsdelivr.net（免额度）
pub const CH_RAW: &str = "raw"; // raw.githubusercontent.com（免额度，兜底）
pub const CH_API: &str = "github-api"; // api.github.com（额度受限）

fn with_channels<R>(f: impl FnOnce(&mut std::collections::HashMap<&'static str, ChannelState>) -> R) -> R {
    let mut guard = CHANNELS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(Default::default);
    f(map)
}

/// 该通道当前是否可用（熔断窗口内的直接判为不可用）
pub fn channel_available(name: &'static str) -> bool {
    with_channels(|m| {
        let st = m.entry(name).or_default();
        match st.disable_until {
            Some(until) if Instant::now() < until => false,
            Some(_) => {
                // 窗口到期：半开，清零后允许再试
                st.disable_until = None;
                st.failures = 0;
                true
            }
            None => true,
        }
    })
}

/// 记一次通道结果
pub fn note_channel(name: &'static str, ok: bool) {
    note_channel_ms(name, ok, None);
}

/// 记一次通道结果（带耗时，供"按实测延迟挑通道"）
pub fn note_channel_ms(name: &'static str, ok: bool, ms: Option<u64>) {
    with_channels(|m| {
        let st = m.entry(name).or_default();
        if ok {
            st.failures = 0;
            st.disable_until = None;
            if let Some(v) = ms {
                // EWMA：新值权重 0.4，避免单次抖动就把顺序翻过来
                st.latency_ms = Some(match st.latency_ms {
                    Some(old) => (old * 3 + v * 2) / 5,
                    None => v,
                });
            }
        } else {
            st.failures += 1;
            if st.failures >= CHANNEL_FAIL_THRESHOLD {
                st.disable_until = Some(Instant::now() + Duration::from_secs(CHANNEL_DISABLE_SECS));
            }
        }
    });
}

/// 该通道最近的成功往返毫秒（没有记录返回 None）
pub fn channel_latency(name: &'static str) -> Option<u64> {
    with_channels(|m| m.get(name).and_then(|st| st.latency_ms))
}

/// 文件树该先问哪条通道（**纯函数**，便于离线测试，也避免测试之间互相污染）。
/// 规则：jsDelivr 不可用 → 只能 api；api 额度用尽 → 只能 jsDelivr；
/// 两条都有实测延迟且 api 快一倍以上 → api 先；否则**免额度优先**（jsDelivr 先）。
fn choose_tree_order(
    jsd_available: bool,
    api_ready: bool,
    jsd_ms: Option<u64>,
    api_ms: Option<u64>,
) -> Vec<&'static str> {
    if !jsd_available {
        return vec![CH_API];
    }
    if !api_ready {
        return vec![CH_JSDELIVR];
    }
    match (jsd_ms, api_ms) {
        (Some(j), Some(a)) if a * 2 < j => vec![CH_API, CH_JSDELIVR],
        _ => vec![CH_JSDELIVR, CH_API],
    }
}

/// 读当前通道状态后套用上面的规则
fn tree_channel_order(api_ready: bool) -> Vec<&'static str> {
    choose_tree_order(
        channel_available(CH_JSDELIVR),
        api_ready,
        channel_latency(CH_JSDELIVR),
        channel_latency(CH_API),
    )
}

/// 手动清空所有通道的熔断状态（自检按钮会调用：用户想知道"现在还行不行"）
pub fn reset_channels() {
    with_channels(|m| m.clear());
}

// ── 单次通道探测（自检 + 熔断共用） ─────────────────────────

/// 一次通道探测结果
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChannelProbe {
    /// github-refs | jsdelivr | raw | github-api
    pub name: String,
    pub ok: bool,
    /// 往返毫秒（失败时是耗时到超时/报错的毫秒数）
    pub ms: u64,
    pub detail: Option<String>,
}

fn elapsed_ms(t: Instant) -> u64 {
    t.elapsed().as_millis() as u64
}

/// 探测 github.com 的 git 智能 HTTP（免额度通道：refs）
pub async fn probe_channel_refs(owner: &str, repo: &str) -> ChannelProbe {
    let t = Instant::now();
    let client = refs_client();
    let url = format!("https://github.com/{owner}/{repo}.git/info/refs?service=git-upload-pack");
    match client
        .get(&url)
        .header("Accept", "application/x-git-upload-pack-advertisement")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            let body = r.text().await.unwrap_or_default();
            let refs = parse_upload_pack_refs(&body);
            let ok = !refs.is_empty();
            note_channel(CH_REF, ok);
            ChannelProbe {
                name: CH_REF.into(),
                ok,
                ms: elapsed_ms(t),
                detail: Some(format!("{} 个 ref", refs.len())),
            }
        }
        Ok(r) => {
            note_channel(CH_REF, false);
            ChannelProbe {
                name: CH_REF.into(),
                ok: false,
                ms: elapsed_ms(t),
                detail: Some(format!("HTTP {}", r.status().as_u16())),
            }
        }
        Err(e) => {
            note_channel(CH_REF, false);
            ChannelProbe {
                name: CH_REF.into(),
                ok: false,
                ms: elapsed_ms(t),
                detail: Some(short_err(&e.to_string())),
            }
        }
    }
}

/// 探测 jsDelivr（data API 取树 + CDN 取文件）
pub async fn probe_channel_jsdelivr(owner: &str, repo: &str, rev: &str, file: &str) -> ChannelProbe {
    let t = Instant::now();
    let client = probe_client();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = false;
    let data = format!("https://data.jsdelivr.com/v1/packages/gh/{owner}/{repo}@{rev}?structure=flat");
    match client.get(&data).send().await {
        Ok(r) if r.status().is_success() => {
            let n = r
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("files").and_then(|f| f.as_array()).map(|a| a.len()))
                .unwrap_or(0);
            ok = n > 0;
            detail.push(format!("树 {n} 条"));
        }
        Ok(r) => detail.push(format!("树 HTTP {}", r.status().as_u16())),
        Err(e) => detail.push(format!("树 {}", short_err(&e.to_string()))),
    }
    let cdn = format!("https://cdn.jsdelivr.net/gh/{owner}/{repo}@{rev}/{file}");
    match client.get(&cdn).send().await {
        Ok(r) if r.status().is_success() => {
            ok = true;
            detail.push("CDN 200".into());
        }
        Ok(r) => detail.push(format!("CDN HTTP {}", r.status().as_u16())),
        Err(e) => detail.push(format!("CDN {}", short_err(&e.to_string()))),
    }
    note_channel(CH_JSDELIVR, ok);
    ChannelProbe {
        name: CH_JSDELIVR.into(),
        ok,
        ms: elapsed_ms(t),
        detail: Some(detail.join(" · ")),
    }
}

/// 探测 raw.githubusercontent.com（内容兜底通道）
pub async fn probe_channel_raw(owner: &str, repo: &str, rev: &str, file: &str) -> ChannelProbe {
    let t = Instant::now();
    let client = probe_client();
    let url = format!("https://raw.githubusercontent.com/{owner}/{repo}/{rev}/{file}");
    match client.get(&url).send().await {
        Ok(r) => {
            let ok = r.status().is_success();
            note_channel(CH_RAW, ok);
            ChannelProbe {
                name: CH_RAW.into(),
                ok,
                ms: elapsed_ms(t),
                detail: Some(format!("HTTP {}", r.status().as_u16())),
            }
        }
        Err(e) => {
            note_channel(CH_RAW, false);
            ChannelProbe {
                name: CH_RAW.into(),
                ok: false,
                ms: elapsed_ms(t),
                detail: Some(short_err(&e.to_string())),
            }
        }
    }
}

/// 探测 api.github.com（只用于元数据；顺带回传额度与是否带 token）
pub async fn probe_channel_api(owner: &str, repo: &str, token: Option<&str>) -> ChannelProbe {
    let t = Instant::now();
    match api_get_json(&format!("https://api.github.com/repos/{owner}/{repo}"), token).await {
        Ok((status, _)) => {
            let ok = (200..300).contains(&status);
            note_channel(CH_API, ok);
            let rl = rate_limit();
            ChannelProbe {
                name: CH_API.into(),
                ok,
                ms: elapsed_ms(t),
                detail: Some(format!(
                    "HTTP {status} · 额度 {}/{}",
                    rl.remaining.unwrap_or(0),
                    rl.limit.unwrap_or(60)
                )),
            }
        }
        Err(e) => {
            note_channel(CH_API, false);
            ChannelProbe {
                name: CH_API.into(),
                ok: false,
                ms: elapsed_ms(t),
                detail: Some(short_err(&e.lines().next().unwrap_or(""))),
            }
        }
    }
}

/// 一次完整的通道自检：四条通道**并发**探测，返回各自的可达性与延迟
pub async fn check_channels(token: Option<&str>) -> Vec<ChannelProbe> {
    reset_channels();
    // 靶子用 GitHub 官方的测试仓库：体积极小、永远公开、默认分支是 master
    // （顺带验证默认分支不等于 main 的场景）。不要用本项目的仓库——名字写错会
    // 让四条通道一起 404，把"网络不通"和"仓库不存在"混在一起。
    let (owner, repo) = ("octocat", "Hello-World");
    let rev = "master";
    // 先从树里挑一个**真实存在**的文件当靶子，避免 404 被误读成通道不通
    let file = match jsdelivr_tree(owner, repo, rev).await {
        Some((t, _)) => t.any_blob().unwrap_or_else(|| "package.json".into()),
        None => "package.json".into(),
    };
    // 四条通道并发探测（spawn 后按序 await：全部已同时开跑，总耗时 ≈ 最慢那条）
    let (o1, r1) = (owner.to_string(), repo.to_string());
    let (o2, r2, v2) = (owner.to_string(), repo.to_string(), rev.to_string());
    let (o3, r3, v3) = (owner.to_string(), repo.to_string(), rev.to_string());
    let (o4, r4) = (owner.to_string(), repo.to_string());
    let tok = token.map(str::to_string);
    let h_refs = tauri::async_runtime::spawn(async move { probe_channel_refs(&o1, &r1).await });
    let f2 = file.clone();
    let h_jsd =
        tauri::async_runtime::spawn(async move { probe_channel_jsdelivr(&o2, &r2, &v2, &f2).await });
    let f3 = file.clone();
    let h_raw =
        tauri::async_runtime::spawn(async move { probe_channel_raw(&o3, &r3, &v3, &f3).await });
    let h_api = tauri::async_runtime::spawn(async move {
        probe_channel_api(&o4, &r4, tok.as_deref()).await
    });
    let mut out = Vec::new();
    for h in [h_refs, h_jsd, h_raw, h_api] {
        match h.await {
            Ok(p) => out.push(p),
            Err(e) => eprintln!("通道探测任务失败: {e}"),
        }
    }
    out
}

fn short_err(s: &str) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() > 90 {
        format!("{}…", s.chars().take(90).collect::<String>())
    } else {
        s
    }
}

// ── GitHub REST 额度守卫 + 进程内缓存 ────────────────────────
//
// 高频动作（探测仓库、比提交）全部走**免额度通道**：
//   * jsDelivr data API  → 整棵文件树
//   * jsDelivr CDN / raw.githubusercontent → 文件内容
//   * git ls-remote      → HEAD 提交、私有仓库判定
// api.github.com 只用于元数据增强（stars / license / 描述），匿名额度 60/小时。
// 每次响应都记录 x-ratelimit-*；额度用尽就**直接失败并给出重置时间**，
// 不再白发请求（也就不会再看到一串 403）。

/// 最近的 GitHub API 额度状态（设置页展示用）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRateLimit {
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    /// 额度重置的 unix 秒（前端自己按本地时区格式化）
    pub reset: Option<u64>,
    /// 是否带了 token（带 token 时额度 5000/小时）
    pub authenticated: bool,
    pub exhausted: bool,
}

#[derive(Clone, Copy, Default)]
struct RlState {
    limit: Option<u32>,
    remaining: Option<u32>,
    reset: Option<u64>,
    authenticated: bool,
}

static RL: Mutex<RlState> = Mutex::new(RlState {
    limit: None,
    remaining: None,
    reset: None,
    authenticated: false,
});
/// URL → (写入时刻, 值) 的简易内存缓存（进程级，重启即失效）
static CACHE: Mutex<Option<std::collections::HashMap<String, (Instant, String)>>> = Mutex::new(None);

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_get(key: &str, ttl: Duration) -> Option<String> {
    let guard = CACHE.lock().ok()?;
    let map = guard.as_ref()?;
    let (at, val) = map.get(key)?;
    if at.elapsed() > ttl {
        return None;
    }
    Some(val.clone())
}

fn cache_put(key: &str, value: &str) {
    let Ok(mut guard) = CACHE.lock() else { return };
    let map = guard.get_or_insert_with(Default::default);
    // 简易容量控制：超过 512 条就整体清掉（探测场景重复率低，不值得做 LRU）
    if map.len() > 512 {
        map.clear();
    }
    map.insert(key.to_string(), (Instant::now(), value.to_string()));
}

/// 从设置里的 token 或环境变量取 GitHub token（提高额度用，可选）
pub fn github_token(explicit: Option<&str>) -> Option<String> {
    if let Some(t) = explicit.map(str::trim).filter(|t| !t.is_empty()) {
        return Some(t.to_string());
    }
    for key in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn note_rl(resp: &reqwest::Response, authenticated: bool) {
    let num = |k: &str| -> Option<u32> {
        resp.headers()
            .get(k)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
    };
    let Ok(mut rl) = RL.lock() else { return };
    if let Some(l) = num("x-ratelimit-limit") {
        rl.limit = Some(l);
    }
    if let Some(r) = num("x-ratelimit-remaining") {
        rl.remaining = Some(r);
    }
    if let Some(t) = num("x-ratelimit-reset") {
        rl.reset = Some(t as u64);
    }
    rl.authenticated = authenticated;
}

/// 当前额度快照
pub fn rate_limit() -> GitHubRateLimit {
    let rl = RL.lock().map(|g| *g).unwrap_or_default();
    let exhausted = rl.remaining == Some(0) && rl.reset.map(|t| t > now_unix()).unwrap_or(false);
    GitHubRateLimit {
        limit: rl.limit,
        remaining: rl.remaining,
        reset: rl.reset,
        authenticated: rl.authenticated,
        exhausted,
    }
}

/// 额度用尽时的统一话术（含剩余等待时间与提额办法）
pub fn rate_limit_message() -> String {
    let rl = rate_limit();
    let wait = rl
        .reset
        .map(|t| t.saturating_sub(now_unix()))
        .map(|s| if s >= 60 { format!("约 {} 分钟后", s / 60) } else { "不到 1 分钟".into() })
        .unwrap_or_else(|| "稍后".into());
    let scope = if rl.authenticated { "GitHub API（token）" } else { "GitHub 匿名 API" };
    format!(
        "{scope} 额度已用尽（{}/{}），{wait}重置。\
         探测与更新检测已走免额度通道（jsDelivr / git），不受影响；\
         想恢复元数据（stars / license）可在设置里填一个 GitHub Token（5000/小时）。",
        rl.remaining.unwrap_or(0),
        rl.limit.unwrap_or(60)
    )
}

/// 额度守卫：已知用尽就直接失败，不发请求
fn api_budget_guard() -> Result<(), String> {
    let rl = rate_limit();
    if rl.exhausted {
        return Err(rate_limit_message());
    }
    Ok(())
}

/// 带额度守卫 + token 的 GitHub API GET，返回 (状态码, JSON)
async fn api_get_json(url: &str, token: Option<&str>) -> Result<(u16, serde_json::Value), String> {
    api_budget_guard()?;
    let t0 = Instant::now();
    let client = http_client()?;
    let etag_key = format!("etag:{url}");
    let body_key = format!("body:{url}");
    let mut req = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    // 条件请求：带 If-None-Match，命中 304 时 GitHub **不计额度**（官方支持的省额度手段）
    if let Some(etag) = cache_get(&etag_key, Duration::from_secs(7 * 24 * 3600)) {
        req = req.header("If-None-Match", etag);
    }
    let resp = req.send().await.map_err(|e| format!("请求 GitHub API 失败: {e}"))?;
    let status = resp.status().as_u16();
    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    note_rl(&resp, token.is_some());
    if status == 304 {
        if let Some(body) = cache_get(&body_key, Duration::from_secs(7 * 24 * 3600)) {
            let json = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
            return Ok((200, json));
        }
        return Ok((304, serde_json::Value::Null));
    }
    if status == 403 || status == 429 {
        return Err(rate_limit_message());
    }
    let text = resp.text().await.unwrap_or_default();
    note_channel_ms(CH_API, (200..300).contains(&status), Some(t0.elapsed().as_millis() as u64));
    if (200..300).contains(&status) {
        if let Some(et) = &etag {
            cache_put(&etag_key, et);
        }
        cache_put(&body_key, &text);
    }
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    Ok((status, json))
}

// ── 免额度通道：jsDelivr 文件树 / CDN 内容 ──────────────────

/// 一个仓库的文件树（文件集合 + 由文件名推导出的目录集合）
#[derive(Default, Clone, Debug)]
pub struct FileTree {
    blobs: std::collections::HashSet<String>,
    dirs: std::collections::HashSet<String>,
}

impl FileTree {
    /// 由「文件路径列表」构建（jsDelivr 的 flat 列表只给文件）
    pub fn from_paths<I: IntoIterator<Item = String>>(paths: I) -> Self {
        let mut t = FileTree::default();
        for p in paths {
            let p = p.trim_start_matches('/').to_string();
            if p.is_empty() {
                continue;
            }
            // 每一级祖先目录都登记
            let segs: Vec<&str> = p.split('/').collect();
            for i in 1..segs.len() {
                t.dirs.insert(segs[..i].join("/"));
            }
            t.blobs.insert(p);
        }
        t
    }

    pub fn has_blob(&self, path: &str) -> bool {
        self.blobs.contains(path.trim_start_matches('/'))
    }

    pub fn has_dir(&self, path: &str) -> bool {
        self.dirs.contains(path.trim_start_matches('/'))
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// 任取一个文件路径（自检时用它当靶子：拿真实存在的文件探测 CDN/raw，
    /// 否则 404 会被误读成"通道不通"）
    pub fn any_blob(&self) -> Option<String> {
        self.blobs.iter().next().cloned()
    }
}

/// jsDelivr data API：整棵文件树（免 GitHub 额度；@sha 时结果不可变、@分支时按 CDN 缓存）
async fn jsdelivr_tree(
    owner: &str,
    repo: &str,
    rev: &str,
) -> Option<(FileTree, Option<String>)> {
    if !channel_available(CH_JSDELIVR) {
        return None;
    }
    let t0 = Instant::now();
    let key = format!("jsd-tree:{owner}/{repo}@{rev}");
    if let Some(hit) = cache_get(&key, Duration::from_secs(600)) {
        // 缓存值 = default_branch 一行 + 路径行
        let mut lines = hit.lines();
        let default = lines.next().filter(|s| *s != "-").map(|s| s.to_string());
        let paths: Vec<String> = lines.map(|s| s.to_string()).collect();
        return Some((FileTree::from_paths(paths), default));
    }
    let client = probe_client();
    let url =
        format!("https://data.jsdelivr.com/v1/packages/gh/{owner}/{repo}@{rev}?structure=flat");
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        note_channel(CH_JSDELIVR, false);
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let default = json
        .get("default")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut paths: Vec<String> = json
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|f| f.get("name").and_then(|n| n.as_str()))
        .map(|s| s.trim_start_matches('/').to_string())
        .collect();
    if paths.is_empty() {
        return None;
    }
    paths.sort();
    let mut cached = String::new();
    cached.push_str(default.as_deref().unwrap_or("-"));
    for p in &paths {
        cached.push('\n');
        cached.push_str(p);
    }
    note_channel_ms(CH_JSDELIVR, true, Some(t0.elapsed().as_millis() as u64));
    cache_put(&key, &cached);
    Some((FileTree::from_paths(paths), default))
}

/// 读取仓库内文件内容：jsDelivr CDN → raw.githubusercontent，两者都**不吃 API 额度**
async fn fetch_file(client: &reqwest::Client, owner: &str, repo: &str, rev: &str, path: &str) -> Option<String> {
    let key = format!("file:{owner}/{repo}@{rev}/{path}");
    if let Some(hit) = cache_get(&key, Duration::from_secs(600)) {
        return Some(hit);
    }
    let path = path.trim_start_matches('/');
    // 熔断期间直接跳过不可用通道，别让每个文件都白等一个超时
    if channel_available(CH_JSDELIVR) {
        let cdn = format!("https://cdn.jsdelivr.net/gh/{owner}/{repo}@{rev}/{path}");
        // jsDelivr 会对并发突发限流（返回 429/空体），退避后重试一次再换源
        for attempt in 0..2 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500));
            }
            if let Some(t) = get_text(client, &cdn, None, None).await {
                note_channel(CH_JSDELIVR, true);
                cache_put(&key, &t);
                return Some(t);
            }
        }
        note_channel(CH_JSDELIVR, false);
    }
    if !channel_available(CH_RAW) {
        return None;
    }
    let raw = format!("https://raw.githubusercontent.com/{owner}/{repo}/{rev}/{path}");
    let t = get_text(client, &raw, None, None).await;
    note_channel(CH_RAW, t.is_some());
    if let Some(t) = &t {
        cache_put(&key, t);
    }
    t
}

/// 并发抓多个文件（jsDelivr 单文件约 1~2s，串行抓 20 个太慢）
async fn fetch_files_concurrent(
    owner: &str,
    repo: &str,
    rev: &str,
    paths: Vec<String>,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for chunk in paths.chunks(4) {
        let mut handles = Vec::new();
        for p in chunk {
            let (o, r, v, path) = (owner.to_string(), repo.to_string(), rev.to_string(), p.clone());
            handles.push(tauri::async_runtime::spawn(async move {
                let client = content_client();
                let text = fetch_file(&client, &o, &r, &v, &path).await;
                (path, text)
            }));
        }
        for h in handles {
            if let Ok((p, Some(t))) = h.await {
                out.insert(p, t);
            }
        }
    }
    out
}

/// 解析 npm registry 搜索响应（/-/v1/search）。独立纯函数便于测试。
pub fn parse_search_response(pack: &serde_json::Value) -> Vec<PackageSearchItem> {
    let mut out = Vec::new();
    if let Some(objs) = pack.get("objects").and_then(|v| v.as_array()) {
        for o in objs {
            let Some(p) = o.get("package") else { continue };
            let (Some(name), Some(version)) = (
                p.get("name").and_then(|v| v.as_str()),
                p.get("version").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            out.push(PackageSearchItem {
                name: name.into(),
                version: version.into(),
                description: p
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                published_at: p.get("date").and_then(|v| v.as_str()).map(String::from),
                link: p
                    .get("links")
                    .and_then(|l| {
                        l.get("npm")
                            .or_else(|| l.get("repository"))
                            .or_else(|| l.get("homepage"))
                    })
                    .and_then(|v| v.as_str())
                    .map(String::from),
            });
        }
    }
    out
}

/// npm registry 关键字搜索（先搜索 → 看描述 → 再安装）
pub async fn search_packages(
    registry_base: &str,
    query: &str,
) -> Result<Vec<PackageSearchItem>, String> {
    let base = registry_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("registry 地址为空".into());
    }
    let query = query.trim();
    if query.is_empty() {
        return Err("搜索词为空".into());
    }

    let resp = http_client()?
        .get(format!("{base}/-/v1/search"))
        .query(&[("text", query), ("size", "10")])
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("搜索请求失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("registry 搜索返回 {status}（{base}）"));
    }
    let pack: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析搜索响应失败: {e}"))?;
    Ok(parse_search_response(&pack))
}

/// 从用户输入解析 GitHub 插件来源：owner/repo、github:owner/repo、
/// https://github.com/owner/repo(.git)(#ref)、tree/blob 链接（含 ref 与插件路径）、
/// `#ref&path:插件子路径` 组合（与 pnpm parseGitParams 一致：& 分隔、顺序无关）、
/// 或打包产物直装链接（.tgz/.tar.gz/.tar 或 releases/download 资产）。
pub fn parse_github_spec(input: &str) -> Option<GitHubPluginSpec> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    // fragment（#ref / #path:xx / #ref&path:xx）先剥离
    let (main, fragment) = match s.split_once('#') {
        Some((m, f)) => (m, Some(f.to_string())),
        None => (s, None),
    };
    let main = main.trim_end_matches('/');
    if (main.starts_with("http://") || main.starts_with("https://"))
        && (main.ends_with(".tgz")
            || main.ends_with(".tar.gz")
            || main.ends_with(".tar")
            || main.contains("/releases/download/"))
    {
        return Some(GitHubPluginSpec {
            tarball_url: Some(main.to_string()),
            ..Default::default()
        });
    }
    let mut rest = main;
    rest = rest.strip_prefix("github:").unwrap_or(rest);
    for prefix in [
        "git+https://github.com/",
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
    ] {
        if let Some(stripped) = rest.strip_prefix(prefix) {
            rest = stripped;
            break;
        }
    }
    let mut seg = rest.split('/');
    let owner = seg.next()?.trim();
    let repo_raw = seg.next()?.trim();
    let repo = repo_raw.strip_suffix(".git").unwrap_or(repo_raw);
    let valid =
        |x: &str| !x.is_empty() && x.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c));
    if !valid(owner) || !valid(repo) {
        return None;
    }

    let mut git_ref = None;
    let mut plugin_path = None;
    let tail: Vec<&str> = seg.filter(|x| !x.is_empty()).collect();
    if tail.len() >= 2 && (tail[0] == "tree" || tail[0] == "blob") {
        // github.com/owner/repo/tree/<ref>/<插件路径…>
        git_ref = Some(tail[1].to_string());
        if tail.len() > 2 {
            plugin_path = Some(tail[2..].join("/"));
        }
    } else if tail.len() == 1 {
        // github.com/owner/repo/<ref> 短链按 ref 处理
        git_ref = Some(tail[0].to_string());
    }
    if let Some(f) = fragment {
        for part in f.split('&') {
            let part = part.trim();
            if let Some(p) = part.strip_prefix("path:") {
                if !p.trim().is_empty() {
                    plugin_path = Some(p.trim().to_string());
                }
            } else if !part.is_empty() {
                git_ref = Some(part.to_string());
            }
        }
    }
    if let Some(p) = &plugin_path {
        let clean = p.trim().trim_matches('/');
        let ok = !clean.is_empty()
            && clean.split('/').all(|seg| seg != ".." && seg != "." && !seg.is_empty());
        if !ok {
            return None;
        }
        plugin_path = Some(clean.to_string());
    }
    Some(GitHubPluginSpec {
        owner: owner.into(),
        repo: repo.into(),
        git_ref,
        plugin_path,
        tarball_url: None,
    })
}

/// 由原始 git 规格构造「升级规格」：保留分支/标签（非 sha）与 `path:` 插件路径，
/// 丢弃钉死的提交 sha——重新安装时解析到该 ref 的最新提交。
pub fn github_upgrade_spec(spec: &str) -> Option<String> {
    let s = parse_github_spec(spec)?;
    if s.tarball_url.is_some() {
        return None;
    }
    let mut fragment: Vec<String> = Vec::new();
    if let Some(r) = &s.git_ref {
        let is_sha = (7..=40).contains(&r.len()) && r.chars().all(|c| c.is_ascii_hexdigit());
        if !is_sha {
            fragment.push(r.clone());
        }
    }
    if let Some(p) = &s.plugin_path {
        fragment.push(format!("path:{p}"));
    }
    Some(match fragment.is_empty() {
        true => format!("github:{}/{}", s.owner, s.repo),
        false => format!("github:{}/{}#{}", s.owner, s.repo, fragment.join("&")),
    })
}

/// 查询 npm 包的 `latest` dist-tag 版本（包不存在返回 None）
pub async fn npm_latest_version(registry_base: &str, name: &str) -> Result<Option<String>, String> {
    let base = registry_base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("registry 地址为空".into());
    }
    let name = name.trim();
    if name.is_empty() {
        return Err("包名为空".into());
    }
    // scope 包名的 / 转义（与 fetch_registry 的 PKG_SCOPE_ENCODED 同一约定）
    let encoded = name.replace('/', "%2F");
    let resp = http_client()?
        .get(format!("{base}/{encoded}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("查询 {name} 版本失败: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("registry 返回 {status}（查询 {name}）"));
    }
    let pack: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 {name} packument 失败: {e}"))?;
    Ok(pack
        .get("dist-tags")
        .and_then(|d| d.get("latest"))
        .and_then(|v| v.as_str())
        .map(String::from))
}

// ── 远端 ref 发现：自己实现 git 智能 HTTP，不依赖 git 二进制 ──
//
// 为什么要自己实现：本机实测 `github.com/<repo>.git/info/refs` 用普通 HTTPS GET
// 只要 ~0.5s，而 `git ls-remote` 会卡 40s（git 的凭据/地址族协商）。探测与更新检测
// 只需要「读出 refs 列表」这一件事，因此直接发这个请求并解析 pkt-line：
// 快、免 GitHub API 额度、且对私有仓库会明确返回 401（不再有交互式登录）。

/// 远端 refs 发现的结果
#[derive(Debug, Clone)]
pub enum RemoteRefs {
    /// 成功拿到 refs：(sha, refname)，另外 HEAD 会以 ("<sha>", "HEAD") 形式出现
    Refs(Vec<(String, String)>),
    /// 需要凭据（私有仓库）或仓库不存在——GitHub 对两者都可能返回 401
    NotPublic,
    /// 网络不可达 / 其它异常，无法判定
    Unknown(String),
}

/// 解析 git 智能 HTTP `info/refs` 的 pkt-line 响应
pub fn parse_upload_pack_refs(body: &str) -> Vec<(String, String)> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        let Ok(len) = usize::from_str_radix(&body[i..i + 4], 16) else {
            break;
        };
        if len == 0 {
            i += 4; // flush-pkt
            continue;
        }
        if len < 5 {
            break;
        }
        let end = (i + len).min(bytes.len());
        let line = body[i + 4..end].trim_end_matches('\n');
        if let Some((sha, name)) = line.split_once(' ') {
            let name = name.split('\0').next().unwrap_or(name).trim();
            if sha.len() == 40
                && sha.chars().all(|c| c.is_ascii_hexdigit())
                && !name.is_empty()
            {
                out.push((sha.to_string(), name.to_string()));
            }
        }
        i += len;
    }
    out
}

fn git_base(owner: &str, repo: &str) -> String {
    format!("https://github.com/{owner}/{repo}.git")
}

/// 读取远端 refs（等价 `git ls-remote`，0.5s 级、免额度、无交互登录）
pub async fn discover_refs(owner: &str, repo: &str, token: Option<&str>) -> RemoteRefs {
    let key = format!("refs:{owner}/{repo}");
    if let Some(hit) = cache_get(&key, Duration::from_secs(60)) {
        if hit == "401" {
            return RemoteRefs::NotPublic;
        }
        if !hit.is_empty() {
            let refs = parse_upload_pack_refs(&hit);
            if !refs.is_empty() {
                return RemoteRefs::Refs(refs);
            }
        }
    }
    if !channel_available(CH_REF) {
        return RemoteRefs::Unknown("github.com 近期不可达（已临时跳过该通道）".into());
    }
    let t0 = Instant::now();
    let client = refs_client();
    let url = format!("{}/info/refs?service=git-upload-pack", git_base(owner, repo));
    let mut req = client
        .get(&url)
        .header("Accept", "application/x-git-upload-pack-advertisement");
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            note_channel(CH_REF, false);
            return RemoteRefs::Unknown(format!("访问 github.com 失败: {e}"));
        }
    };
    let status = resp.status().as_u16();
    if status == 401 || status == 403 || status == 404 {
        // 能明确答复（要凭据/不存在）说明通道是通的
        note_channel_ms(CH_REF, true, Some(t0.elapsed().as_millis() as u64));
        cache_put(&key, "401");
        return RemoteRefs::NotPublic;
    }
    if !(200..300).contains(&status) {
        note_channel(CH_REF, false);
        return RemoteRefs::Unknown(format!("github.com 返回 {status}"));
    }
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            note_channel(CH_REF, false);
            return RemoteRefs::Unknown(format!("读取 refs 失败: {e}"));
        }
    };
    let refs = parse_upload_pack_refs(&body);
    if refs.is_empty() {
        note_channel(CH_REF, false);
        return RemoteRefs::Unknown("refs 响应为空".into());
    }
    note_channel_ms(CH_REF, true, Some(t0.elapsed().as_millis() as u64));
    cache_put(&key, &body);
    RemoteRefs::Refs(refs)
}

/// 在 refs 里解析某个 ref 指向的提交（缺省 = HEAD）
pub fn ref_sha(refs: &[(String, String)], git_ref: Option<&str>) -> Option<String> {
    let r = git_ref.map(str::trim).filter(|r| !r.is_empty());
    match r {
        None => refs
            .iter()
            .find(|(_, n)| n == "HEAD")
            .or_else(|| refs.iter().find(|(_, n)| n.starts_with("refs/heads/")))
            .map(|(s, _)| s.clone()),
        Some(name) if name.len() == 40 && name.chars().all(|c| c.is_ascii_hexdigit()) => {
            Some(name.to_string())
        }
        Some(name) => {
            let candidates = [
                name.to_string(),
                format!("refs/heads/{name}"),
                format!("refs/tags/{name}"),
                format!("refs/tags/{name}^{{}}"),
            ];
            candidates
                .iter()
                .find_map(|c| refs.iter().find(|(_, n)| n == c).map(|(s, _)| s.clone()))
        }
    }
}

/// 查询 GitHub 仓库某 ref（缺省默认分支）的最新提交 SHA（不可匿名访问返回 None）。
///
/// 通道顺序是**自适应**的，因为这台机器到 github.com 间歇性挂起：
/// - 带 token（额度 5000/小时）或额度仍充裕 → 先走 API（0.5s 级）；
/// - 否则先走免额度的 git 智能 HTTP refs 发现（同样 0.5s 级，但不吃额度）；
/// - 任一条被熔断（连续失败 2 次，5 分钟窗口）就跳过它，最后才是 6s 上限的 git 二进制兜底。
pub async fn github_head_commit(
    owner: &str,
    repo: &str,
    git_ref: Option<&str>,
    token: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(r) = git_ref {
        if r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(Some(r.to_string()));
        }
    }
    let key = format!("head:{owner}/{repo}#{}", git_ref.unwrap_or(""));
    if let Some(hit) = cache_get(&key, Duration::from_secs(60)) {
        return Ok(if hit == "-" { None } else { Some(hit) });
    }

    let api_ready = !rate_limit().exhausted;
    let refs_ready = channel_available(CH_REF);
    let mut order: Vec<u8> = Vec::new();
    if token.is_some() && api_ready {
        order.push(2);
    }
    if refs_ready {
        order.push(1);
    }
    if api_ready && !order.contains(&2) {
        order.push(2);
    }
    if order.is_empty() {
        // 全被熔断：仍各试一次（熔断窗口到点会半开）
        order = vec![1, 2];
    }

    let mut last_err: Option<String> = None;
    for step in order {
        if step == 1 {
            match discover_refs(owner, repo, token).await {
                RemoteRefs::Refs(refs) => {
                    return match ref_sha(&refs, git_ref) {
                        Some(s) => {
                            cache_put(&key, &s);
                            Ok(Some(s))
                        }
                        // refs 拿到了但没有匹配的 ref（例如传了不存在的分支）
                        None => Ok(None),
                    };
                }
                RemoteRefs::NotPublic => {
                    cache_put(&key, "-");
                    return Ok(None);
                }
                RemoteRefs::Unknown(e) => last_err = Some(e),
            }
        } else {
            let api = match git_ref {
                Some(r) => format!("https://api.github.com/repos/{owner}/{repo}/commits/{r}"),
                None => format!("https://api.github.com/repos/{owner}/{repo}/commits?per_page=1"),
            };
            match api_get_json(&api, token).await {
                Ok((404, _)) => {
                    cache_put(&key, "-");
                    return Ok(None);
                }
                Ok((status, json)) if (200..300).contains(&status) => {
                    let sha = match &json {
                        serde_json::Value::Array(arr) => arr
                            .first()
                            .and_then(|c| c.get("sha"))
                            .and_then(|v| v.as_str()),
                        obj => obj.get("sha").and_then(|v| v.as_str()),
                    };
                    if let Some(s) = sha {
                        cache_put(&key, s);
                    }
                    return Ok(sha.map(String::from));
                }
                Ok((status, _)) => last_err = Some(format!("GitHub API 返回 {status}")),
                Err(e) => last_err = Some(e),
            }
        }
    }

    // 兜底：git 二进制（6s 上限，见 git_remote_head 的注释）
    let url = format!("https://github.com/{owner}/{repo}.git");
    match crate::plugin::git_remote_head(&url, git_ref) {
        Ok(sha) => {
            cache_put(&key, &sha);
            Ok(Some(sha))
        }
        Err(e) => {
            if crate::plugin::looks_like_auth_error(&e) || crate::plugin::looks_like_missing_repo(&e)
            {
                Ok(None)
            } else {
                Err(last_err.unwrap_or_else(|| {
                    format!("无法读取远端提交：{}", e.lines().next().unwrap_or(""))
                }))
            }
        }
    }
}

/// 仓库可访问性判定（三态：可用 / 不可匿名访问 / 无法判定）
#[derive(Debug, Clone)]
pub enum RepoAccess {
    Public,
    /// 私有仓库或不存在（GitHub 对二者都要求凭据）
    NotPublic,
    /// 网络/额度等原因无法判定——**不应该阻止用户安装**
    Unknown(String),
}

/// 判定仓库是否可匿名访问。三条通道依次尝试，任何一条给出确定答案即返回；
/// 都判不出来时返回 Unknown，让上层「先装了再说」而不是报错卡住。
pub async fn repo_access(owner: &str, repo: &str, token: Option<&str>) -> RepoAccess {
    let key = format!("access:{owner}/{repo}");
    if let Some(hit) = cache_get(&key, Duration::from_secs(600)) {
        return match hit.as_str() {
            "public" => RepoAccess::Public,
            "private" => RepoAccess::NotPublic,
            other => RepoAccess::Unknown(other.to_string()),
        };
    }
    // ① ref 发现（~0.5s，免额度）
    #[allow(unused_assignments)]
    let mut first_reason: Option<String> = None;
    match discover_refs(owner, repo, token).await {
        RemoteRefs::Refs(_) => {
            cache_put(&key, "public");
            return RepoAccess::Public;
        }
        RemoteRefs::NotPublic => {
            cache_put(&key, "private");
            return RepoAccess::NotPublic;
        }
        RemoteRefs::Unknown(reason) => first_reason = Some(reason),
    }
    // ② REST API（额度守卫）
    match api_get_json(&format!("https://api.github.com/repos/{owner}/{repo}"), token).await {
        Ok((200..=299, _)) => {
            cache_put(&key, "public");
            RepoAccess::Public
        }
        Ok((404, _)) => {
            cache_put(&key, "private");
            RepoAccess::NotPublic
        }
        Ok((status, _)) => {
            RepoAccess::Unknown(format!("GitHub API 返回 {status}（ref 发现：{}）", first_reason.unwrap_or_else(|| "未知".into())))
        }
        Err(e) => RepoAccess::Unknown(if first_reason.is_some() {
            format!("{e}（ref 发现：{}）", first_reason.unwrap_or_default())
        } else {
            e
        }),
    }
}

/// 预检一个 git 远端地址（GitHub 走自实现 ref 发现 / 其它主机走 git ls-remote）
pub async fn probe_remote_access(url: &str, token: Option<&str>) -> RepoAccess {
    if let Some(spec) = parse_github_spec(url) {
        if spec.tarball_url.is_none() && !spec.owner.is_empty() && !spec.repo.is_empty() {
            return repo_access(&spec.owner, &spec.repo, token).await;
        }
    }
    let owned = url.to_string();
    match tauri::async_runtime::spawn_blocking(move || {
        crate::plugin::git_remote_head(&owned, None)
    })
    .await
    {
        Ok(Ok(_)) => RepoAccess::Public,
        Ok(Err(e)) => {
            if crate::plugin::looks_like_auth_error(&e) || crate::plugin::looks_like_missing_repo(&e)
            {
                RepoAccess::NotPublic
            } else {
                RepoAccess::Unknown(crate::plugin::probe_error_hint(&e))
            }
        }
        Err(e) => RepoAccess::Unknown(format!("预检失败: {e}")),
    }
}

/// 私有仓库/不存在的仓库的统一拒绝话术（GitHub 对二者都返回 404）
pub fn private_repo_reject(owner: &str, repo: &str) -> String {
    format!(
        "仓库 {owner}/{repo} 不存在或为私有仓库。\
         暂不支持私有仓库：请手动 git clone 到本地后，用「链接 / 本地」标签页的 link 路径安装"
    )
}

/// 拉取 GitHub 仓库公开信息并**全量探测**其中的插件包（安装前预览）。
///
/// 通道顺序（前两步完全不消耗 GitHub API 额度，60/小时 的限额因此基本不会被磨光）：
/// 1. `git ls-remote` 解析用户指定的 ref 或默认分支的 HEAD sha（git 协议，免额度、最新）；
/// 2. `data.jsdelivr.com` 按该 sha 取**整棵文件树** + `cdn.jsdelivr.net` 取各 package.json
///    （CDN，免额度；按 sha 取还保证预览与安装到的是同一份提交）；
/// 3. 仅在 jsDelivr 不可用时回落到 `api.github.com/git/trees`（额度守卫）；
/// 4. 再不行才降级为单目录 contents 探测。
///
/// 仓库元数据（stars / license / 描述）本来就只有 REST API 有：额度用完时
/// `meta_degraded = true`，候选列表照常给出。
pub async fn fetch_github_repo(
    repo_input: &str,
    token: Option<&str>,
) -> Result<GitHubRepoInfo, String> {
    let spec = parse_github_spec(repo_input)
        .ok_or_else(|| format!("无法识别 GitHub 来源：「{repo_input}」（示例 owner/repo 或打包产物链接）"))?;
    if let Some(url) = &spec.tarball_url {
        return Ok(GitHubRepoInfo {
            full_name: url.clone(),
            html_url: url.clone(),
            install_spec: url.clone(),
            lib_ok: None,
            plugin_path: None,
            git_ref: None,
            default_branch: None,
            is_monorepo: false,
            workspace_globs: Vec::new(),
            candidates: Vec::new(),
            probe: "tarball".into(),
            meta_degraded: false,
            facts_pending: false,
            description: None,
            stars: 0,
            pushed_at: None,
            license: None,
        });
    }
    let (owner, repo) = (spec.owner.clone(), spec.repo.clone());
    let content = content_client();

    // ── ① 元数据（最先做：快、可缓存，还直接给出默认分支名） ──
    //
    // 顺序刻意如此：这台机器到 github.com 间歇性挂起，而 api.github.com 稳定在 0.5s 级。
    // 把"解析 HEAD sha"放到关键路径最前面，会让一个探测白等一个 6s 超时才能开始取树；
    // 先拿元数据既便宜（额度守卫 + ETag + 60s 缓存），又顺手拿到 default_branch，
    // 树就能直接按分支名取（jsDelivr 的稳定路径）。
    let mut meta_degraded = false;
    let mut meta = serde_json::Value::Null;
    let mut default_branch: Option<String> = None;
    match api_get_json(&format!("https://api.github.com/repos/{owner}/{repo}"), token).await {
        Ok((status, json)) if (200..300).contains(&status) => {
            meta = json;
            default_branch = meta
                .get("default_branch")
                .and_then(|v| v.as_str())
                .map(String::from);
        }
        _ => meta_degraded = true,
    }
    // 拿不到默认分支（额度用尽/不可用）时，退回免额度的 refs 发现
    let mut head_sha: Option<String> = None;
    let git_ref_for_tree = spec.git_ref.clone();
    if default_branch.is_none() {
        match github_head_commit(&owner, &repo, None, None).await {
            Ok(Some(sha)) => {
                head_sha = Some(sha);
            }
            _ => {}
        }
    }
    let mut default_branch = default_branch.unwrap_or_else(|| "main".into());

    // ── ② 文件树：jsDelivr(用户 ref) → jsDelivr(默认分支) → sha → main/master → API → contents ──
    let mut probe = String::from("jsdelivr");
    let mut tree: Option<FileTree> = None;
    let mut rev_candidates: Vec<String> = Vec::new();
    if let Some(r) = &git_ref_for_tree {
        rev_candidates.push(r.clone());
    }
    rev_candidates.push(default_branch.clone());
    for c in ["main", "master"] {
        if !rev_candidates.iter().any(|x| x == c) {
            rev_candidates.push(c.to_string());
        }
    }
    if let Some(sha) = &head_sha {
        rev_candidates.push(sha.clone());
    }
    let api_ready = !rate_limit().exhausted;
    for channel in tree_channel_order(api_ready) {
        if tree.is_some() {
            break;
        }
        match channel {
            CH_JSDELIVR => {
                for cand in &rev_candidates {
                    if let Some((t, d)) = jsdelivr_tree(&owner, &repo, cand).await {
                        default_branch = d.filter(|d| !d.is_empty()).unwrap_or(default_branch);
                        tree = Some(t);
                        break;
                    }
                }
            }
            CH_API => {
                let t1 = Instant::now();
                let ref_for_api = git_ref_for_tree.as_deref().or(Some(default_branch.as_str()));
                if let Some((t, truncated)) = fetch_tree_api(&owner, &repo, ref_for_api, token).await {
                    note_channel_ms(CH_API, true, Some(t1.elapsed().as_millis() as u64));
                    probe = if truncated { "api-truncated".into() } else { "api".into() };
                    tree = Some(t);
                }
            }
            _ => {}
        }
    }
    if tree.is_none() {
        let ref_for_api = git_ref_for_tree.as_deref().or(Some(default_branch.as_str()));
        if let Some((t, truncated)) = fetch_tree_api(&owner, &repo, ref_for_api, token).await {
            probe = if truncated { "api-truncated".into() } else { "api".into() };
            tree = Some(t);
        }
    }

    let git_ref = spec.git_ref.clone().or_else(|| Some(default_branch.clone()));
    // 内容读取用的 rev：优先分支名/标签（jsDelivr 的稳定路径）
    let rev = git_ref.clone().unwrap_or_else(|| default_branch.clone());

    if tree.is_none() {
        // ── ④ 彻底降级：单目录 contents 探测 ──
        probe = "contents".into();
        let target = spec.plugin_path.clone().unwrap_or_default();
        let lib_ok = check_lib_dir(
            &content,
            &owner,
            &repo,
            if target.is_empty() { None } else { Some(target.as_str()) },
            git_ref.as_deref(),
            token,
        )
        .await;
        let mut cand = PluginCandidate::root();
        cand.path = target.clone();
        cand.lib_ok = lib_ok.unwrap_or(false);
        cand.install_spec = github_install_spec(&owner, &repo, git_ref.as_deref(), &target);
        let spec_text = github_install_spec(&owner, &repo, git_ref.as_deref(), &target);
        return Ok(GitHubRepoInfo {
            full_name: format!("{owner}/{repo}"),
            description: None,
            stars: 0,
            pushed_at: None,
            html_url: format!("https://github.com/{owner}/{repo}"),
            license: None,
            plugin_path: Some(target),
            lib_ok: lib_ok,
            git_ref,
            default_branch: Some(default_branch.clone()),
            is_monorepo: false,
            workspace_globs: Vec::new(),
            candidates: vec![cand],
            probe,
            meta_degraded,
            facts_pending: true,
            install_spec: spec_text,
        });
    }
    let tree = tree.expect("tree 已在上面的分支里保证存在");
    if probe == "jsdelivr" && tree.is_empty() {
        probe = "api".into();
    }

    // ── ⑤ monorepo：workspace 成员 glob ──
    let mut workspace_globs: Vec<String> = Vec::new();
    let mut root_facts = PkgFacts::default();
    if tree.has_blob("package.json") {
        if let Some(t) = fetch_file(&content, &owner, &repo, &rev, "package.json").await {
            root_facts = parse_pkg_facts(&t);
        }
    }
    for ws_file in ["pnpm-workspace.yaml", "pnpm-workspace.yml"] {
        if tree.has_blob(ws_file) {
            if let Some(text) = fetch_file(&content, &owner, &repo, &rev, ws_file).await {
                workspace_globs = parse_pnpm_workspace(&text);
                if !workspace_globs.is_empty() {
                    break;
                }
            }
        }
    }
    if workspace_globs.is_empty() {
        workspace_globs = root_facts.workspaces.clone();
    }

    // ── ⑥ 候选包：workspace 成员优先，抓 package.json 判定 dsh.bundle ──
    let mut ordered = dir_list_ordered(&tree, &workspace_globs);
    if let Some(p) = &spec.plugin_path {
        let p = p.trim().trim_matches('/').to_string();
        ordered.retain(|(d, _)| *d == p);
        if ordered.is_empty() {
            ordered.push((p, true));
        }
    }
    let cap = if spec.plugin_path.is_some() { 1 } else { 20 };
    let take: Vec<(String, bool)> = ordered.into_iter().take(cap).collect();
    let pkg_paths: Vec<String> = take
        .iter()
        .map(|(d, _)| {
            if d.is_empty() {
                "package.json".to_string()
            } else {
                format!("{d}/package.json")
            }
        })
        .collect();
    let fetched = fetch_files_concurrent(&owner, &repo, &rev, pkg_paths).await;

    // API 树被截断时，候选列表可能不全 —— 如实标记，别让用户以为"仓库里就这些"
    let mut facts_pending = probe == "api-truncated";
    let mut candidates: Vec<PluginCandidate> = Vec::new();
    for (dir, ws) in take {
        let pkg_path = if dir.is_empty() {
            "package.json".to_string()
        } else {
            format!("{dir}/package.json")
        };
        let facts = match fetched.get(&pkg_path) {
            Some(t) => parse_pkg_facts(t),
            None => {
                facts_pending = true;
                PkgFacts::default()
            }
        };
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let lib_ok = lib_dir_in_tree(&tree, &prefix);
        let has_patch = tree.has_blob(&format!("{prefix}cordis.patch.yml"))
            || tree.has_blob(&format!("{prefix}cordis.patch.yaml"));
        let has_bundle = facts.bundle_patch.is_some();
        candidates.push(PluginCandidate {
            path: dir.clone(),
            name: facts.name.clone(),
            version: facts.version.clone(),
            description: facts.description.clone(),
            lib_ok,
            has_bundle,
            has_patch,
            workspace_member: ws,
            ready: has_bundle,
            install_spec: github_install_spec(&owner, &repo, git_ref.as_deref(), &dir),
        });
    }
    candidates.sort_by(|a, b| {
        b.ready
            .cmp(&a.ready)
            .then(b.lib_ok.cmp(&a.lib_ok))
            .then(b.workspace_member.cmp(&a.workspace_member))
            .then(depth_of(&a.path).cmp(&depth_of(&b.path)))
            .then(a.path.cmp(&b.path))
    });
    candidates.dedup_by(|a, b| a.path == b.path);

    let is_monorepo = !workspace_globs.is_empty();
    let first = candidates.first().cloned().unwrap_or_else(PluginCandidate::root);
    let install_spec = if first.install_spec.is_empty() {
        github_install_spec(&owner, &repo, git_ref.as_deref(), "")
    } else {
        first.install_spec.clone()
    };

    Ok(GitHubRepoInfo {
        full_name: meta
            .get("full_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&format!("{owner}/{repo}"))
            .to_string(),
        description: meta
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| root_facts.description.clone()),
        stars: meta.get("stargazers_count").and_then(|v| v.as_u64()).unwrap_or(0),
        pushed_at: meta.get("pushed_at").and_then(|v| v.as_str()).map(String::from),
        html_url: meta
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("https://github.com/{owner}/{repo}")),
        license: meta
            .get("license")
            .and_then(|l| l.get("spdx_id"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "NOASSERTION")
            .map(String::from),
        plugin_path: Some(first.path.clone()),
        lib_ok: if candidates.is_empty() { None } else { Some(first.lib_ok) },
        git_ref,
        default_branch: Some(default_branch.clone()),
        is_monorepo,
        workspace_globs,
        candidates,
        probe,
        meta_degraded,
        facts_pending,
        install_spec,
    })
}

/// 由 (owner, repo, ref, 仓库内路径) 构造 pnpm 安装规格
pub fn github_install_spec(owner: &str, repo: &str, git_ref: Option<&str>, path: &str) -> String {
    let mut fragment: Vec<String> = Vec::new();
    if let Some(r) = git_ref.filter(|r| !r.is_empty()) {
        fragment.push(r.to_string());
    }
    let p = path.trim().trim_matches('/');
    if !p.is_empty() {
        fragment.push(format!("path:{p}"));
    }
    match fragment.is_empty() {
        true => format!("github:{owner}/{repo}"),
        false => format!("github:{owner}/{repo}#{}", fragment.join("&")),
    }
}

fn depth_of(path: &str) -> usize {
    if path.is_empty() {
        0
    } else {
        path.split('/').filter(|s| !s.is_empty()).count()
    }
}

/// 探测时忽略的目录片段（依赖 / 构建产物 / 版本库）
fn is_ignored_path(path: &str) -> bool {
    const BAD: &[&str] = &[
        "node_modules",
        ".git",
        ".github",
        "dist",
        "build",
        "coverage",
        ".next",
        ".cache",
        ".turbo",
        "tmp",
        "__fixtures__",
        "fixtures",
    ];
    path.split('/').any(|seg| BAD.contains(&seg))
}

/// 列出树中所有含 package.json 的目录（workspace 成员优先、浅层优先）
fn dir_list_ordered(tree: &FileTree, globs: &[String]) -> Vec<(String, bool)> {
    let mut dirs: Vec<String> = Vec::new();
    for b in tree.blobs.iter() {
        if !b.ends_with("/package.json") && b.as_str() != "package.json" {
            continue;
        }
        let dir = match b.strip_suffix("/package.json") {
            Some(d) => d.to_string(),
            None => String::new(),
        };
        if is_ignored_path(&dir) {
            continue;
        }
        dirs.push(dir);
    }
    dirs.sort();
    dirs.dedup();
    let mut out: Vec<(String, bool)> = dirs
        .into_iter()
        .map(|d| {
            let ws = workspace_hit(globs, &d);
            (d, ws)
        })
        .collect();
    out.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(depth_of(&a.0).cmp(&depth_of(&b.0)))
            .then(a.0.cmp(&b.0))
    });
    out
}

/// 树里 `<prefix>lib/` 是否存在且有构建产物（.js/.cjs/.mjs）
fn lib_dir_in_tree(tree: &FileTree, prefix: &str) -> bool {
    let lib = format!("{prefix}lib");
    if !tree.has_dir(&lib) && !tree.blobs.iter().any(|b| b.starts_with(&format!("{lib}/"))) {
        return false;
    }
    tree.blobs
        .iter()
        .any(|b| b.starts_with(&format!("{lib}/")) && is_js_file(b))
}

fn is_js_file(p: &str) -> bool {
    p.ends_with(".js") || p.ends_with(".cjs") || p.ends_with(".mjs")
}

/// 兜底通道：GitHub REST 的文件树（额度守卫；jsDelivr 不可用时才走）。
///
/// 注意 `truncated`：大仓库的文件树会被 API 截断（本仓库实测只剩 5 个 package.json，
/// 而 jsDelivr 给 10 个）。返回 (树, 是否被截断)，调用方要把截断如实透给用户，
/// 否则"候选变少"会被误当成"仓库里就这么几个插件"。
async fn fetch_tree_api(
    owner: &str,
    repo: &str,
    git_ref: Option<&str>,
    token: Option<&str>,
) -> Option<(FileTree, bool)> {
    let reference = git_ref.unwrap_or("HEAD");
    let url = format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{reference}?recursive=1");
    let (status, json) = api_get_json(&url, token).await.ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    let arr = json.get("tree")?.as_array()?;
    let files: Vec<String> = arr
        .iter()
        .filter(|e| e.get("type").and_then(|v| v.as_str()) == Some("blob"))
        .filter_map(|e| e.get("path").and_then(|v| v.as_str()))
        .map(String::from)
        .collect();
    if files.is_empty() {
        return None;
    }
    let truncated = json
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Some((FileTree::from_paths(files), truncated))
}

/// 带可选 query / Accept 的文本 GET（非 2xx 返回 None）
async fn get_text(
    client: &reqwest::Client,
    url: &str,
    query: Option<(&str, &str)>,
    accept: Option<&str>,
) -> Option<String> {
    let mut req = client.get(url);
    if let Some((k, v)) = query {
        req = req.query(&[(k, v)]);
    }
    if let Some(a) = accept {
        req = req.header("Accept", a);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// 内容/树抓取专用客户端：单请求 6s 上限，慢源快速失败切换到下一个源
fn content_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

// ── 本地目录探测（本地 link 安装 / 克隆仓库清单共用） ──────

/// 探测本地目录里的插件包：目录本身 + monorepo（pnpm-workspace / workspaces）子包。
/// 与 GitHub 侧使用同一套判定规则（dsh.bundle / lib/ / cordis.patch.yml）。
pub fn probe_local_plugins(root: &Path) -> Result<Vec<PluginCandidate>, String> {
    if !root.is_dir() {
        return Err(format!("目录不存在：{}", root.display()));
    }
    let root_pkg_path = root.join("package.json");
    let root_facts = std::fs::read_to_string(&root_pkg_path)
        .ok()
        .map(|t| parse_pkg_facts(&t))
        .unwrap_or_default();

    // workspace glob：pnpm-workspace.yaml 优先，其次 package.json workspaces
    let mut globs: Vec<String> = Vec::new();
    for f in ["pnpm-workspace.yaml", "pnpm-workspace.yml"] {
        if let Ok(text) = std::fs::read_to_string(root.join(f)) {
            globs = parse_pnpm_workspace(&text);
            if !globs.is_empty() {
                break;
            }
        }
    }
    if globs.is_empty() {
        globs = root_facts.workspaces.clone();
    }

    // 候选目录：根 + 深度 ≤ 4 内含 package.json 的目录（跳过依赖/产物）
    let mut dirs: Vec<PathBuf> = vec![root.to_path_buf()];
    collect_pkg_dirs(root, 0, 4, &mut dirs);
    dirs.sort();
    dirs.dedup();

    let mut out = Vec::new();
    for dir in dirs {
        let rel = dir
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let rel_trim = rel.trim_matches('/').to_string();
        if !rel_trim.is_empty() && is_ignored_path(&rel_trim) {
            continue;
        }
        let facts = std::fs::read_to_string(dir.join("package.json"))
            .ok()
            .map(|t| parse_pkg_facts(&t))
            .unwrap_or_default();
        let lib_ok = lib_dir_ok(&dir);
        let has_patch =
            dir.join("cordis.patch.yml").is_file() || dir.join("cordis.patch.yaml").is_file();
        let has_bundle = facts.bundle_patch.is_some();
        let ws = (rel_trim.is_empty() && globs.is_empty()) || workspace_hit(&globs, &rel_trim);
        // 根目录若是普通包（无 bundle、无 lib、无 name）且不是 workspace 成员，不算候选
        if rel_trim.is_empty() && !has_bundle && !lib_ok && facts.name.is_none() {
            continue;
        }
        out.push(PluginCandidate {
            path: rel_trim.clone(),
            name: facts.name.clone(),
            version: facts.version.clone(),
            description: facts.description.clone(),
            lib_ok,
            has_bundle,
            has_patch,
            workspace_member: ws,
            ready: has_bundle,
            install_spec: format!("link:{}", dir.to_string_lossy()),
        });
    }

    out.sort_by(|a, b| {
        b.ready
            .cmp(&a.ready)
            .then(b.lib_ok.cmp(&a.lib_ok))
            .then(b.workspace_member.cmp(&a.workspace_member))
            .then(depth_of(&a.path).cmp(&depth_of(&b.path)))
            .then(a.path.cmp(&b.path))
    });
    Ok(out)
}

/// 递归收集含 package.json 的目录（跳过依赖/构建产物，限制深度与数量）
fn collect_pkg_dirs(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth || out.len() > 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || is_ignored_path(&name) {
            continue;
        }
        if p.join("package.json").is_file() {
            out.push(p.clone());
        }
        collect_pkg_dirs(&p, depth + 1, max_depth, out);
    }
}

/// 本地 lib/ 校验：目录存在且有 .js/.cjs/.mjs（导出给更新检测判断是否需要重新构建）
pub fn local_lib_ok(dir: &Path) -> bool {
    lib_dir_ok(dir)
}

/// 本地 lib/ 校验：目录存在且有 .js/.cjs/.mjs
fn lib_dir_ok(dir: &Path) -> bool {
    let lib = dir.join("lib");
    if !lib.is_dir() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(&lib) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.is_file()
            && p.extension()
                .and_then(|x| x.to_str())
                .map(|x| matches!(x, "js" | "cjs" | "mjs"))
                .unwrap_or(false)
    })
}

/// 查询插件根目录内容，确认 lib/ 目录存在。API 异常（限流等）返回 None 不阻断安装。
async fn check_lib_dir(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    plugin_path: Option<&str>,
    git_ref: Option<&str>,
    _token: Option<&str>,
) -> Option<bool> {
    let mut url = match plugin_path {
        Some(p) => format!("https://api.github.com/repos/{owner}/{repo}/contents/{}", 
            p.split('/').map(|seg| seg.trim()).collect::<Vec<_>>().join("/")),
        None => format!("https://api.github.com/repos/{owner}/{repo}/contents"),
    };
    if let Some(r) = git_ref {
        url.push_str(&format!("?ref={r}"));
    }
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        // 路径不存在 / ref 不对：确定没有可安装的插件根
        let code = resp.status().as_u16();
        if code == 404 {
            return Some(false);
        }
        return None;
    }
    let entries: serde_json::Value = resp.json().await.ok()?;
    let arr = entries.as_array()?;
    Some(arr
        .iter()
        .any(|e| e.get("name").and_then(|v| v.as_str()) == Some("lib")
            && e.get("type").and_then(|v| v.as_str()) == Some("dir")))
}
#[cfg(test)]
mod tests {
    use super::*;


    /// 通道熔断：连续失败到阈值就临时跳过，成功即清零，窗口到期半开
    #[test]
    fn channel_breaker_skips_flaky_channel_then_recovers() {
        let _guard = crate::util::NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_channels();
        assert!(channel_available(CH_REF));
        // 单次失败还不熔断（避免偶发抖动就禁用）
        note_channel(CH_REF, false);
        assert!(channel_available(CH_REF));
        // 第二次连续失败 → 熔断
        note_channel(CH_REF, false);
        assert!(!channel_available(CH_REF), "连续两次失败后应临时跳过");
        // 其它通道不受影响
        assert!(channel_available(CH_JSDELIVR));
        // 任意一次成功清零
        note_channel(CH_REF, true);
        assert!(channel_available(CH_REF));
        // 手动重置（自检按钮会调用）
        note_channel(CH_API, false);
        note_channel(CH_API, false);
        assert!(!channel_available(CH_API));
        reset_channels();
        assert!(channel_available(CH_API));
    }

    /// 按实测延迟挑树通道（纯函数，不碰全局状态，因此不受其它用例影响）
    #[test]
    fn tree_channel_prefers_faster_channel_by_measured_latency() {
        // 还没实测过 → 免额度优先
        assert_eq!(choose_tree_order(true, true, None, None), vec![CH_JSDELIVR, CH_API]);
        // api 明显更快（0.6s vs 8s，就是本机现状）→ 先问 api
        assert_eq!(
            choose_tree_order(true, true, Some(8000), Some(600)),
            vec![CH_API, CH_JSDELIVR]
        );
        // 只差一倍以内 → 仍免额度优先，避免来回横跳
        assert_eq!(
            choose_tree_order(true, true, Some(900), Some(600)),
            vec![CH_JSDELIVR, CH_API]
        );
        // api 额度用尽 → 只剩免额度通道
        assert_eq!(choose_tree_order(true, false, Some(8000), None), vec![CH_JSDELIVR]);
        // jsDelivr 被熔断 → 改用 api
        assert_eq!(choose_tree_order(false, true, None, None), vec![CH_API]);
    }

    /// 自检返回四条通道且结构完整（外网不可达时只断言形状，不要求 ok）
    #[test]
    fn check_channels_reports_all_four() {
        let _guard = crate::util::NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let probes = tauri::async_runtime::block_on(check_channels(None));
        let names: Vec<&str> = probes.iter().map(|p| p.name.as_str()).collect();
        for want in [CH_REF, CH_JSDELIVR, CH_RAW, CH_API] {
            assert!(names.contains(&want), "缺少通道 {want}: {names:?}");
        }
        assert!(probes.iter().all(|p| p.ms < 60_000), "耗时字段异常: {probes:?}");
        // 自检会把熔断状态清空重建，因此至少有一条免额度通道给出确定结论
        println!("通道自检: {probes:?}");
    }

    #[test]
    fn private_repo_message_guides_manual_clone() {
        let m = private_repo_reject("Yinxe", "dsh-qqbot");
        assert!(m.contains("不存在或为私有仓库"));
        assert!(m.contains("暂不支持私有仓库"));
        assert!(m.contains("git clone"));
        assert!(m.contains("link"));
    }

    /// 关键回归：GitHub API 额度耗尽时，探测必须仍然可用（走 jsDelivr + git 免额度通道）。
    ///
    /// 需要外网（jsDelivr）；网络不可达时跳过，不视为失败。
    #[test]
    fn probe_works_without_api_quota() {
        let _guard = crate::util::NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let rl = rate_limit();
        println!("额度状态: {rl:?}");
        let info = match tauri::async_runtime::block_on(fetch_github_repo(
            "Yinxe/deepseek-harness-plugins",
            None,
        )) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("跳过：探测失败（网络不可达？）{e}");
                return;
            }
        };
        if info.candidates.is_empty() {
            eprintln!("跳过：CDN 未返回候选（网络不可达？）probe={}", info.probe);
            return;
        }
        println!(
            "probe={} meta_degraded={} mono={} globs={:?} 候选={}",
            info.probe,
            info.meta_degraded,
            info.is_monorepo,
            info.workspace_globs,
            info.candidates.len()
        );
        for c in info.candidates.iter().take(12) {
            println!("  {:?} {:?} v={:?} ready={} lib={}", c.path, c.name, c.version, c.ready, c.lib_ok);
        }
        assert!(!info.candidates.is_empty(), "额度耗尽也应探到候选包");
        assert!(info.candidates.iter().any(|c| c.path == "plugins/token-meter"), "应含 token-meter");
        // 树走哪条通道是**按实测延迟自适应**的（api 快一倍以上就用 api），
        // 因此只要求"用了一条已知通道"，重点在候选与降级标记
        assert!(
            matches!(info.probe.as_str(), "jsdelivr" | "api" | "api-truncated"),
            "未知探测通道: {}",
            info.probe
        );
        // 元数据只依赖 GitHub API：额度已知耗尽时必须标记降级且照常出候选
        if rl.exhausted {
            assert!(info.meta_degraded, "额度耗尽时元数据应标记为降级");
        }
        // 首次探测里那次「元数据增强」撞上 403 后，额度状态即被记为耗尽；
        // 之后的探测不会再发任何 API 请求（api_budget_guard 直接短路）
        let after = rate_limit();
        println!("探测后额度: {after:?}");
        if after.exhausted {
            let info2 = tauri::async_runtime::block_on(fetch_github_repo(
                "Yinxe/deepseek-harness-plugins",
                None,
            ))
            .expect("额度耗尽后第二次探测仍应成功");
            assert!(matches!(info2.probe.as_str(), "jsdelivr" | "api" | "api-truncated"));
            assert!(!info2.candidates.is_empty());
            assert_eq!(
                rate_limit().remaining,
                after.remaining,
                "已知耗尽后不该再发 API 请求"
            );
        }
    }

    #[test]
    fn parse_search_response_extracts_packages() {
        let json: serde_json::Value = serde_json::json!({
            "objects": [
                { "package": {
                    "name": "@dshp/mcwiki-search", "version": "1.2.3",
                    "description": "Minecraft wiki search",
                    "date": "2026-01-02T03:04:05Z",
                    "links": { "npm": "https://www.npmjs.com/package/@dshp/mcwiki-search" }
                }},
                { "package": { "name": "no-desc-pkg", "version": "0.1.0" } },
                { "no_package_here": true }
            ]
        });
        let items = parse_search_response(&json);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "@dshp/mcwiki-search");
        assert_eq!(items[0].version, "1.2.3");
        assert_eq!(items[0].description.as_deref(), Some("Minecraft wiki search"));
        assert_eq!(items[0].published_at.as_deref(), Some("2026-01-02T03:04:05Z"));
        assert!(items[0].link.is_some());
        assert!(items[1].description.is_none() && items[1].link.is_none());
    }

    #[test]
    fn github_upgrade_spec_keeps_ref_path_drops_sha() {
        assert_eq!(
            github_upgrade_spec("github:o/r#dev&path:plugins/foo").as_deref(),
            Some("github:o/r#dev&path:plugins/foo")
        );
        // 钉死的 sha 被丢弃，重新解析到 HEAD
        assert_eq!(
            github_upgrade_spec("github:o/r#1234567890abcdef1234567890abcdef12345678&path:plugins/foo").as_deref(),
            Some("github:o/r#path:plugins/foo")
        );
        assert_eq!(
            github_upgrade_spec("github:o/r").as_deref(),
            Some("github:o/r")
        );
        // 打包产物没有「升级到 HEAD」语义
        assert_eq!(github_upgrade_spec("https://example.com/pkg.tgz"), None);
        assert_eq!(github_upgrade_spec("not a spec"), None);
    }

    #[test]
    fn parse_github_spec_accepts_common_forms() {
        let case = |input: &str, owner: &str, repo: &str, git_ref: Option<&str>, path: Option<&str>, tarball: bool| {
            let got = parse_github_spec(input).unwrap();
            assert_eq!(got.owner, owner, "input={input}");
            assert_eq!(got.repo, repo, "input={input}");
            assert_eq!(got.git_ref.as_deref(), git_ref, "input={input}");
            assert_eq!(got.plugin_path.as_deref(), path, "input={input}");
            assert_eq!(got.tarball_url.is_some(), tarball, "input={input}");
        };
        case("owner/repo", "owner", "repo", None, None, false);
        case(" owner/repo ", "owner", "repo", None, None, false);
        case("github:owner/repo", "owner", "repo", None, None, false);
        case("https://github.com/owner/repo", "owner", "repo", None, None, false);
        case("https://github.com/owner/repo.git", "owner", "repo", None, None, false);
        case("https://github.com/owner/repo#dev", "owner", "repo", Some("dev"), None, false);
        case("git@github.com:owner/repo.git", "owner", "repo", None, None, false);
        // 插件子路径：tree 链接 / #path: 参数 / 组合（顺序无关，同 pnpm）
        case(
            "https://github.com/owner/repo/tree/main/plugins/mcwiki-search",
            "owner", "repo", Some("main"), Some("plugins/mcwiki-search"), false,
        );
        case("github:owner/repo#path:plugins/foo", "owner", "repo", None, Some("plugins/foo"), false);
        case("github:owner/repo#dev&path:plugins/foo", "owner", "repo", Some("dev"), Some("plugins/foo"), false);
        case("github:owner/repo#path:plugins/foo&dev", "owner", "repo", Some("dev"), Some("plugins/foo"), false);
        // 打包产物直装：整条 URL（剥离 fragment 后）作为 tarball 规格，不解析 owner/repo
        for (tb, expect) in [
            ("https://github.com/o/r/releases/download/v1/pkg-1.0.tgz", "https://github.com/o/r/releases/download/v1/pkg-1.0.tgz"),
            ("https://example.com/dist/pkg.tar.gz", "https://example.com/dist/pkg.tar.gz"),
            ("https://example.com/dist/pkg.tar", "https://example.com/dist/pkg.tar"),
            ("https://example.com/x.tgz#path:nope", "https://example.com/x.tgz"),
        ] {
            let got = parse_github_spec(tb).unwrap();
            assert_eq!(got.tarball_url.as_deref(), Some(expect), "input={tb}");
            assert_eq!(got.owner, "", "tarball 模式不解析 owner");
        }
        // 路径清洗：首尾斜杠去掉（裸 fragment 无 path: 前缀按 ref 处理，同 pnpm）
        case("github:owner/repo#path:/plugins/foo/", "owner", "repo", None, Some("plugins/foo"), false);
        // 非法：缺 repo、路径穿越、注入字符
        for bad in [
            "owner", "", "https://github.com/owner", "owner/re po",
            "github:owner/repo#path:../..", "github:owner/repo#path:a//b",
        ] {
            assert!(parse_github_spec(bad).is_none(), "bad={bad}");
        }
    }

    /// 本地探测：monorepo 的 pnpm-workspace 子包插件应被逐个发现并按「已就绪」排序
    #[test]
    fn probe_local_plugins_finds_monorepo_subpackages() {
        let tmp = std::env::temp_dir().join(format!("dsh-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("package.json"),
            r#"{"name":"monorepo","private":true}"#,
        )
        .unwrap();
        std::fs::write(tmp.join("pnpm-workspace.yaml"), "packages:\n  - 'plugins/*'\n").unwrap();

        // 子包 A：完整插件（dsh.bundle + lib 产物 + patch）
        let a = tmp.join("plugins/alpha");
        std::fs::create_dir_all(a.join("lib")).unwrap();
        std::fs::write(
            a.join("package.json"),
            r#"{"name":"@dshp/alpha","version":"1.2.3","description":"A","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .unwrap();
        std::fs::write(a.join("lib/host.js"), "export default {}").unwrap();
        std::fs::write(a.join("cordis.patch.yml"), "- id: alpha\n").unwrap();

        // 子包 B：只提交了源码，没有 lib（提示需要 clone+build）
        let b = tmp.join("plugins/beta");
        std::fs::create_dir_all(b.join("src")).unwrap();
        std::fs::write(
            b.join("package.json"),
            r#"{"name":"@dshp/beta","version":"0.0.1","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .unwrap();

        // node_modules 里的包必须被忽略
        let nm = tmp.join("node_modules/@x/y");
        std::fs::create_dir_all(nm.join("lib")).unwrap();
        std::fs::write(nm.join("package.json"), r#"{"name":"@x/y"}"#).unwrap();
        std::fs::write(nm.join("lib/index.js"), "").unwrap();

        let found = probe_local_plugins(&tmp).unwrap();
        let paths: Vec<String> = found.iter().map(|c| c.path.clone()).collect();
        assert!(paths.contains(&"plugins/alpha".to_string()), "paths={paths:?}");
        assert!(paths.contains(&"plugins/beta".to_string()), "paths={paths:?}");
        assert!(!paths.iter().any(|p| p.contains("node_modules")), "paths={paths:?}");

        let alpha = found.iter().find(|c| c.path == "plugins/alpha").unwrap();
        assert_eq!(alpha.name.as_deref(), Some("@dshp/alpha"));
        assert_eq!(alpha.version.as_deref(), Some("1.2.3"));
        assert!(alpha.has_bundle && alpha.lib_ok && alpha.ready && alpha.workspace_member);
        assert!(alpha.install_spec.starts_with("link:"));

        let beta = found.iter().find(|c| c.path == "plugins/beta").unwrap();
        assert!(beta.has_bundle && !beta.lib_ok && beta.ready, "beta 应可安装但提示无 lib");
        // 已就绪的排在前面
        assert_eq!(found[0].path, "plugins/alpha");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
