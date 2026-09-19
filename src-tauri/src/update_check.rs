use serde::Serialize;
use std::time::Duration;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateStatus {
    pub available: bool,
    pub current: String,
    pub latest: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
    /// manifest（自建清单）| unconfigured | error
    pub mode: String,
    pub message: Option<String>,
}

/// 拉取自建更新清单 {version, notes?, url?} 并与当前版本比较
pub async fn check_manifest(url: &str, current: &str) -> LauncherUpdateStatus {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return LauncherUpdateStatus {
                available: false,
                current: current.into(),
                latest: None,
                notes: None,
                url: None,
                mode: "error".into(),
                message: Some(format!("初始化 HTTP 客户端失败: {e}")),
            }
        }
    };

    let resp = match client.get(url.trim()).send().await {
        Ok(r) => r,
        Err(e) => {
            return LauncherUpdateStatus {
                available: false,
                current: current.into(),
                latest: None,
                notes: None,
                url: None,
                mode: "error".into(),
                message: Some(format!("请求更新清单失败: {e}")),
            }
        }
    };

    if !resp.status().is_success() {
        return LauncherUpdateStatus {
            available: false,
            current: current.into(),
            latest: None,
            notes: None,
            url: None,
            mode: "error".into(),
            message: Some(format!("更新清单返回 {}", resp.status())),
        };
    }

    let j: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            return LauncherUpdateStatus {
                available: false,
                current: current.into(),
                latest: None,
                notes: None,
                url: None,
                mode: "error".into(),
                message: Some(format!("解析更新清单失败: {e}")),
            }
        }
    };

    let latest = j.get("version").and_then(|v| v.as_str()).map(String::from);
    let notes = j.get("notes").and_then(|v| v.as_str()).map(String::from);
    let url_out = j.get("url").and_then(|v| v.as_str()).map(String::from);

    match latest.as_deref() {
        Some(l) => {
            let available = crate::semver::compare(l, current) == std::cmp::Ordering::Greater;
            LauncherUpdateStatus {
                available,
                current: current.into(),
                latest: Some(l.to_string()),
                notes,
                url: url_out,
                mode: "manifest".into(),
                message: if available {
                    Some(format!("发现新版本 {l}"))
                } else {
                    Some("已是最新版本".into())
                },
            }
        }
        None => LauncherUpdateStatus {
            available: false,
            current: current.into(),
            latest: None,
            notes: None,
            url: None,
            mode: "error".into(),
            message: Some("更新清单缺少 version 字段".into()),
        },
    }
}
