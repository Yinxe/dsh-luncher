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
