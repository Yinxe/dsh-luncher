use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;

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
    pub git_ref: Option<String>,
    /// 插件在仓库内的路径（插件根目录，规范要求其下必须有 lib/ 目录）
    pub plugin_path: Option<String>,
    /// lib/ 目录校验：Some(true)=已确认存在 Some(false)=确认缺失 None=未校验（打包产物直装）
    pub lib_ok: Option<bool>,
    /// 最终交给 dsh plugin add 的安装规格（github:owner/repo#ref&path:xx 或打包产物 URL）
    pub install_spec: String,
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

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))
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

/// 查询 GitHub 仓库某 ref（缺省默认分支）的最新提交 SHA（仓库不存在返回 None）
pub async fn github_head_commit(
    owner: &str,
    repo: &str,
    git_ref: Option<&str>,
) -> Result<Option<String>, String> {
    let url = match git_ref {
        Some(r) => format!("https://api.github.com/repos/{owner}/{repo}/commits/{r}"),
        None => format!("https://api.github.com/repos/{owner}/{repo}/commits?per_page=1"),
    };
    let resp = http_client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("请求 GitHub 提交信息失败: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("GitHub API 返回 {status}（可能有速率限制，稍后再试）"));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 GitHub 提交响应失败: {e}"))?;
    // /commits/{ref} 返回单对象；/commits?per_page=1 返回数组
    let sha = match &json {
        serde_json::Value::Array(arr) => arr
            .first()
            .and_then(|c| c.get("sha"))
            .and_then(|v| v.as_str()),
        obj => obj.get("sha").and_then(|v| v.as_str()),
    };
    Ok(sha.map(String::from))
}

/// 拉取 GitHub 仓库公开信息并校验插件根目录的 lib/（未认证 API，供安装前预览）。
/// 打包产物链接跳过 API 直接返回直装规格。
pub async fn fetch_github_repo(repo_input: &str) -> Result<GitHubRepoInfo, String> {
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
            description: None,
            stars: 0,
            pushed_at: None,
            license: None,
        });
    }
    let (owner, repo) = (&spec.owner, &spec.repo);
    let client = http_client()?;
    let resp = client
        .get(format!("https://api.github.com/repos/{owner}/{repo}"))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("请求 GitHub API 失败: {e}"))?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Err(format!("仓库 {owner}/{repo} 不存在或为私有仓库"));
    }
    if !status.is_success() {
        return Err(format!("GitHub API 返回 {status}（可能有速率限制，稍后再试）"));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 GitHub 响应失败: {e}"))?;
    let license = json
        .get("license")
        .and_then(|l| l.get("spdx_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && *s != "NOASSERTION")
        .map(String::from);
    let full_name = json
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or(&format!("{owner}/{repo}"))
        .to_string();
    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("https://github.com/{owner}/{repo}"));

    // 规范校验：插件根目录（子路径或仓库根）下必须有 lib/ 目录，否则 dsh 无法正常安装
    let lib_ok = check_lib_dir(&client, owner, repo, spec.plugin_path.as_deref(), spec.git_ref.as_deref()).await;

    // 安装规格：github:owner/repo[#ref][&path:xx]（与 pnpm parseGitParams 对齐）
    let mut fragment: Vec<String> = Vec::new();
    if let Some(r) = &spec.git_ref {
        fragment.push(r.clone());
    }
    if let Some(p) = &spec.plugin_path {
        fragment.push(format!("path:{p}"));
    }
    let install_spec = match fragment.is_empty() {
        true => format!("github:{owner}/{repo}"),
        false => format!("github:{owner}/{repo}#{}", fragment.join("&")),
    };

    Ok(GitHubRepoInfo {
        full_name,
        description: json.get("description").and_then(|v| v.as_str()).map(String::from),
        stars: json.get("stargazers_count").and_then(|v| v.as_u64()).unwrap_or(0),
        pushed_at: json.get("pushed_at").and_then(|v| v.as_str()).map(String::from),
        html_url,
        license,
        git_ref: spec.git_ref,
        plugin_path: spec.plugin_path,
        lib_ok,
        install_spec,
    })
}

/// 查询插件根目录内容，确认 lib/ 目录存在。API 异常（限流等）返回 None 不阻断安装。
async fn check_lib_dir(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    plugin_path: Option<&str>,
    git_ref: Option<&str>,
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
}
