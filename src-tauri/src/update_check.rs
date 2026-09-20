use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::ghaccel;
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

/// 从编译进二进制的 tauri.conf.json 读 updater endpoints。
/// 插件把 config 放在私有 state 里没暴露读取接口，所以这里直接解析同一份文件
/// （include_str! 会在文件变化时触发重编译，不会和配置脱节）。
fn configured_endpoints() -> Vec<String> {
    let Ok(j) = serde_json::from_str::<serde_json::Value>(include_str!("../tauri.conf.json")) else {
        return Vec::new();
    };
    j.pointer("/plugins/updater/endpoints")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// 本次更新要用的加速前缀（None = 直连）。
/// 复用全局的 GitHub 加速开关与测速结果，和 clone / 插件下载同一套。
fn accel_prefix(settings: &Settings) -> Option<String> {
    if !settings.github_accel {
        return None;
    }
    let accel = match ghaccel::current().or_else(|| ghaccel::ensure_cached(ghaccel::CACHE_TTL_SECS)) {
        Some(a) => a,
        None => {
            // 还没测过速：后台补一次，这次先直连（下次检查就带加速了）
            ghaccel::spawn_refresh(settings.github_proxy_extra.clone(), false);
            return None;
        }
    };
    let preferred = if settings.github_proxy.trim().is_empty() {
        None
    } else {
        Some(settings.github_proxy.as_str())
    };
    // 更新只走 https 下载，不需要 git 能力
    ghaccel::pick_prefix(&accel, preferred, false)
}

/// 给更新相关地址套加速前缀。
/// tauri-action 生成的 latest.json 里下载地址是 `api.github.com/repos/.../releases/assets/<id>`，
/// 它不在 ghaccel 的 github 域名白名单里，这里单独补上（gh-proxy 支持这种形式；
/// ghfast 之类只认 github.com 的代理会 403，由调用处的「失败回退直连」兜住）。
fn accel_url(url: &str, prefix: &str) -> String {
    let u = url.trim();
    for host in [
        "https://api.github.com/",
        "https://release-assets.githubusercontent.com/",
    ] {
        if let Some(rest) = u.strip_prefix(host) {
            let Some(p) = ghaccel::normalize_prefix(prefix) else {
                return u.to_string();
            };
            if u.starts_with(&p) {
                return u.to_string();
            }
            return format!("{p}{host}{rest}");
        }
    }
    ghaccel::rewrite(u, prefix)
}

/// 按设置构造 updater：清单地址可选地套上加速前缀
fn build_updater(
    app: &AppHandle,
    settings: &Settings,
) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    if let Some(prefix) = accel_prefix(settings) {
        let urls: Vec<tauri::Url> = configured_endpoints()
            .iter()
            .map(|u| accel_url(u, &prefix))
            .filter_map(|u| tauri::Url::parse(&u).ok())
            .collect();
        if !urls.is_empty() {
            builder = builder.endpoints(urls).map_err(|e| e.to_string())?;
        }
    }
    builder.build().map_err(|e| e.to_string())
}

/// 用内置 Tauri updater 检查（endpoints / pubkey 取自 tauri.conf.json 的 plugins.updater）。
/// 只有这个模式能「下载并安装 + 重启」，自建清单只能跳转下载页。
pub async fn check_builtin(app: &AppHandle, settings: &Settings) -> LauncherUpdateStatus {
    let current = app.package_info().version.to_string();

    let updater = match build_updater(app, settings) {
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
    check_builtin(app, settings).await
}

/// 下载安装包字节。`Update::download` 内部会用公钥验签，所以经第三方代理加速
/// 也无法投毒：代理改一个字节就会验签失败。
async fn download_pkg(
    update: &tauri_plugin_updater::Update,
    app: &AppHandle,
) -> Result<Vec<u8>, String> {
    let app2 = app.clone();
    let mut received: u64 = 0;
    update
        .download(
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
        .map_err(|e| e.to_string())
}

/// 下载并安装内置 updater 提供的新版本（由用户点击触发，不静默）。
/// - AppImage / macOS：直接替换，无需授权
/// - deb / rpm：由 updater 调 pkexec / sudo 走 dpkg / rpm，会弹系统密码框
/// - Windows：启动 NSIS / MSI 安装器接管，本进程随后被结束
/// - Linux / macOS 安装完成后必须自己重启才生效
///
/// 开了 GitHub 加速时，清单与安装包都走加速前缀；代理不支持该地址（403）或断流时
/// 自动回退直连，不会因为代理不可用而升级失败。
pub async fn install_builtin(app: &AppHandle, settings: &Settings) -> Result<String, String> {
    if !installable() {
        return Err("当前是开发或未知安装形态，无法应用内更新，请手动下载安装包".into());
    }

    let updater = build_updater(app, settings).map_err(|e| format!("更新器不可用：{e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败：{e}"))?
        .ok_or_else(|| "已是最新版本，无需更新".to_string())?;

    let proxied = accel_prefix(settings).and_then(|prefix| {
        let url = accel_url(update.download_url.as_str(), &prefix);
        tauri::Url::parse(&url).ok().filter(|u| *u != update.download_url)
    });

    let bytes = match proxied {
        Some(url) => {
            let mut fast = update.clone();
            fast.download_url = url;
            match download_pkg(&fast, app).await {
                Ok(b) => b,
                Err(e) => {
                    // 代理不支持这个地址 / 中途断流：把进度回零后直连重试
                    let _ = app.emit(
                        "launcher-update-progress",
                        LauncherUpdateProgress { received: 0, total: 0 },
                    );
                    eprintln!("[updater] 加速下载失败，回退直连：{e}");
                    download_pkg(&update, app)
                        .await
                        .map_err(|e| format!("下载或校验安装包失败：{e}"))?
                }
            }
        }
        None => download_pkg(&update, app)
            .await
            .map_err(|e| format!("下载或校验安装包失败：{e}"))?,
    };

    update.install(bytes).map_err(|e| format!("安装失败：{e}"))?;

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
