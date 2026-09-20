use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::settings::Settings;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateStatus {
    pub available: bool,
    pub current: String,
    pub latest: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
    /// manifest（自建清单，只能跳转下载）| builtin（Tauri updater，可应用内安装）
    /// | unconfigured | unsupported（deb/rpm 等无法自更新）| error
    pub mode: String,
    pub message: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherUpdateProgress {
    pub received: u64,
    pub total: u64,
}

fn status(current: &str, mode: &str, message: Option<String>) -> LauncherUpdateStatus {
    LauncherUpdateStatus {
        available: false,
        current: current.into(),
        latest: None,
        notes: None,
        url: None,
        mode: mode.into(),
        message,
    }
}

/// 当前安装形态能否自我替换。
/// Linux 上只有 AppImage 是「自包含、可整包替换」的；deb / rpm 由系统包管理器负责升级，
/// 应用内安装必然失败，所以直接判为不支持而不是等到下载报错。
pub fn self_update_supported() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// 用内置 Tauri updater 检查（endpoints / pubkey 取自 tauri.conf.json 的 plugins.updater）。
/// 只有这个模式能「下载并安装 + 重启」，自建清单只能跳转下载页。
pub async fn check_builtin(app: &AppHandle) -> LauncherUpdateStatus {
    let current = app.package_info().version.to_string();

    if !self_update_supported() {
        return status(
            &current,
            "unsupported",
            Some("当前安装形态不支持应用内更新（Linux 仅 AppImage 支持），请用系统包管理器升级".into()),
        );
    }

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return status(
                &current,
                "unconfigured",
                Some(format!("未配置更新源（tauri.conf.json 的 plugins.updater）：{e}")),
            )
        }
    };

    match updater.check().await {
        Ok(Some(u)) => LauncherUpdateStatus {
            available: true,
            current,
            latest: Some(u.version.clone()),
            notes: u.body.clone(),
            url: None,
            mode: "builtin".into(),
            message: Some(format!("发现新版本 {}", u.version)),
        },
        Ok(None) => status(&current, "builtin", Some("已是最新版本".into())),
        Err(e) => status(&current, "error", Some(format!("检查更新失败：{e}"))),
    }
}

/// 统一更新检查入口：自建清单优先，未配置清单时回落到内置 updater
pub async fn check(app: &AppHandle, settings: &Settings) -> LauncherUpdateStatus {
    let manifest = settings.update_manifest_url.trim();
    if !manifest.is_empty() {
        let current = app.package_info().version.to_string();
        return check_manifest(manifest, &current).await;
    }
    check_builtin(app).await
}

/// 下载并安装内置 updater 提供的新版本。
/// Windows 由 NSIS / MSI 安装器接管（成功启动安装器后本进程会被结束）；
/// Linux / macOS 装完必须自己重启才生效。
pub async fn install_builtin(app: &AppHandle) -> Result<String, String> {
    if !self_update_supported() {
        return Err("当前安装形态不支持应用内更新（Linux 仅 AppImage 支持）".into());
    }

    let updater = app.updater().map_err(|e| format!("更新器不可用：{e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败：{e}"))?
        .ok_or_else(|| "已是最新版本，无需更新".to_string())?;

    let app2 = app.clone();
    let mut received: u64 = 0;

    update
        .download_and_install(
            move |len, total| {
                received += len as u64;
                let _ = app2.emit(
                    "launcher-update-progress",
                    LauncherUpdateProgress {
                        received,
                        total: total.unwrap_or(0),
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|e| format!("下载或校验安装包失败：{e}"))?;

    #[cfg(windows)]
    {
        // 安装器已在运行，本进程随后被结束，这里只是为了给前端一个收尾消息
        Ok(format!("已启动安装程序，正在升级到 {}", update.version))
    }
    #[cfg(not(windows))]
    {
        app.restart();
    }
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
