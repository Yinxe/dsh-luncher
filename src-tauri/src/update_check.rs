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
    /// | unconfigured | unsupported（开发构建 / 未知安装形态）| error
    pub mode: String,
    /// 安装时需要管理员授权（deb / rpm：pkexec+dpkg / rpm -U）
    pub needs_elevation: bool,
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
        needs_elevation: false,
        message,
    }
}

/// 当前二进制的打包形态。
/// 打包时 tauri-bundler 会把二进制里的 `__TAURI_BUNDLE_TYPE_VAR_UNK` 标记替换成
/// 具体格式（_DEB / _RPM / _APP / _NSS / _MSI），所以同一个二进制能知道自己
/// 是被装成 deb 还是 AppImage；开发构建没有这个标记，返回 None。
pub fn bundle_kind() -> Option<tauri::utils::config::BundleType> {
    tauri::utils::platform::bundle_type()
}

/// 安装新版本是否需要管理员授权：deb / rpm 由 updater 调
/// `pkexec dpkg -i`（退 zenity/kdialog + sudo），必然弹一次系统密码框。
pub fn needs_elevation() -> bool {
    use tauri::utils::config::BundleType;
    matches!(bundle_kind(), Some(BundleType::Deb) | Some(BundleType::Rpm))
}

/// 能否在应用内安装：必须是已知的打包形态。
/// 开发构建 / 未知形态（bundle_type() 为 None）不放开——那种情况下 updater 会把
/// 安装包字节写到当前可执行文件上，会把开发用的二进制写坏。
pub fn installable() -> bool {
    bundle_kind().is_some()
}

/// 用内置 Tauri updater 检查（endpoints / pubkey 取自 tauri.conf.json 的 plugins.updater）。
/// 只有这个模式能「下载并安装 + 重启」，自建清单只能跳转下载页。
pub async fn check_builtin(app: &AppHandle) -> LauncherUpdateStatus {
    let current = app.package_info().version.to_string();

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

    let installable = installable();
    match updater.check().await {
        Ok(Some(u)) => LauncherUpdateStatus {
            available: true,
            current,
            latest: Some(u.version.clone()),
            notes: u.body.clone(),
            url: None,
            mode: if installable { "builtin".into() } else { "unsupported".into() },
            needs_elevation: installable && needs_elevation(),
            message: Some(if installable {
                format!("发现新版本 {}", u.version)
            } else {
                format!("发现新版本 {}，但当前是开发/未知安装形态，请手动下载", u.version)
            }),
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

/// 下载并安装内置 updater 提供的新版本（由用户点击触发，不静默）。
/// - AppImage / macOS：直接替换，无需授权
/// - deb / rpm：由 updater 调 pkexec / sudo 走 dpkg / rpm，会弹系统密码框
/// - Windows：启动 NSIS / MSI 安装器接管，本进程随后被结束
/// - Linux / macOS 安装完成后必须自己重启才生效
pub async fn install_builtin(app: &AppHandle) -> Result<String, String> {
    if !installable() {
        return Err("当前是开发或未知安装形态，无法应用内更新，请手动下载安装包".into());
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
        Err(e) => return status(current, "error", Some(format!("初始化 HTTP 客户端失败: {e}"))),
    };

    let resp = match client.get(url.trim()).send().await {
        Ok(r) => r,
        Err(e) => return status(current, "error", Some(format!("请求更新清单失败: {e}"))),
    };

    if !resp.status().is_success() {
        return status(current, "error", Some(format!("更新清单返回 {}", resp.status())));
    }

    let j: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return status(current, "error", Some(format!("解析更新清单失败: {e}"))),
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
                // 自建清单只能跳转下载页，安装由用户手动完成，不需要提权
                needs_elevation: false,
                message: if available {
                    Some(format!("发现新版本 {l}"))
                } else {
                    Some("已是最新版本".into())
                },
            }
        }
        None => status(current, "error", Some("更新清单缺少 version 字段".into())),
    }
}
