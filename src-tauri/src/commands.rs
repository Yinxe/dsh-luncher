use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::installer::InstallState;
use crate::launcher::{self, LaunchResult};
use crate::registry::{self, RegistryInfo};
use crate::settings::{self, AppState, Settings};
use crate::update_check::{self, LauncherUpdateStatus};
use crate::util;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub node: Option<String>,
    pub node_path: Option<String>,
    pub npm: Option<String>,
    pub npm_path: Option<String>,
    pub dsh_home: String,
    pub versions_dir: String,
    pub registry: String,
    /// dsh 自身数据目录（$DSH_HOME 或 ~/.dsh）
    pub dsh_native_home: String,
    /// dsh 的 profile 目录
    pub profiles_dir: String,
    /// 启动器内置 Node 运行时是否已安装
    pub runtime_installed: bool,
    pub runtime_dir: String,
}

#[tauri::command]
pub async fn get_environment(state: State<'_, AppState>) -> Result<EnvironmentInfo, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let node = util::find_node(&settings.node_path);
        let npm = util::find_npm(&settings);
        let node_version = node.as_ref().and_then(|n| {
            util::run_captured(
                n,
                &["--version".to_string()],
                std::time::Duration::from_secs(5),
            )
        });
        let npm_version = match &npm {
            Some(inv) => {
                let mut args = inv.args.clone();
                args.push("--version".into());
                util::run_captured(&inv.program, &args, std::time::Duration::from_secs(10))
            }
            None => None,
        };
        Ok(EnvironmentInfo {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            node: node_version.map(|v| v.trim_start_matches('v').to_string()),
            node_path: node.map(|p| p.to_string_lossy().into_owned()),
            npm: npm_version,
            npm_path: npm.map(|i| i.program.to_string_lossy().into_owned()),
            dsh_home: settings::launcher_home().to_string_lossy().into_owned(),
            versions_dir: settings::versions_dir().to_string_lossy().into_owned(),
            registry: settings.registry,
            dsh_native_home: crate::profiles::dsh_native_home().to_string_lossy().into_owned(),
            profiles_dir: crate::profiles::profiles_dir().to_string_lossy().into_owned(),
            runtime_installed: crate::runtime::runtime_installed(),
            runtime_dir: crate::runtime::runtime_dir().to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| format!("环境探测失败: {e}"))?
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    settings::save_settings(&settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub async fn list_remote_versions(state: State<'_, AppState>) -> Result<RegistryInfo, String> {
    let registry_base = state.settings.lock().unwrap().registry.clone();
    registry::fetch_registry(&registry_base).await
}

#[tauri::command]
pub async fn list_installed(
    state: State<'_, AppState>,
) -> Result<Vec<crate::installed::InstalledVersion>, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || Ok(crate::installed::collect_installed(&settings)))
        .await
        .map_err(|e| format!("扫描已安装版本失败: {e}"))?
}

#[tauri::command]
pub fn install_version(
    app: AppHandle,
    state: State<'_, AppState>,
    install_state: State<'_, InstallState>,
    version: String,
    force: Option<bool>,
) -> Result<bool, String> {
    if !util::is_safe_version(&version) {
        return Err("非法版本号".into());
    }
    let settings = state.settings.lock().unwrap().clone();
    installer_start(
        &app,
        &install_state,
        &settings,
        &version,
        force.unwrap_or(false),
    )?;
    Ok(true)
}

fn installer_start(
    app: &AppHandle,
    install_state: &InstallState,
    settings: &Settings,
    version: &str,
    force: bool,
) -> Result<(), String> {
    crate::installer::start_install(app.clone(), install_state, settings, version, force)
}

#[tauri::command]
pub fn cancel_install(install_state: State<'_, InstallState>) {
    crate::installer::cancel(&install_state);
}

#[tauri::command]
pub fn get_install_status(install_state: State<'_, InstallState>) -> Option<String> {
    install_state.running_version()
}

#[tauri::command]
pub fn uninstall_version(version: String) -> Result<(), String> {
    crate::installer::uninstall_managed(&version)
}

#[tauri::command]
pub async fn launch_version(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: Option<String>,
    args: Option<String>,
    // Some("") = 用户显式选择“默认 profile”；None 时回退到设置里的 default_profile
    profile: Option<String>,
) -> Result<LaunchResult, String> {
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let installed = crate::installed::collect_installed(&settings);
        let target = match version.as_deref() {
            Some(v) if v != "unknown" => installed
                .iter()
                .find(|i| i.version == v)
                .cloned()
                .ok_or_else(|| format!("未找到已安装的 {v}，请先安装"))?,
            _ => crate::installed::pick_latest(&installed)
                .ok_or_else(|| "还没有已安装的 dsh 版本，请先在列表中选择安装".to_string())?,
        };
        let launch_args = args
            .or(Some(settings.default_args.clone()))
            .unwrap_or_default();
        let profile = profile.unwrap_or_else(|| settings.default_profile.clone());
        // 每个 profile 同时只能有一个实例（内嵌与终端启动共用该约束）
        let prof = profile.trim();
        if !prof.is_empty() {
            for p in crate::procs::list(&proc_state) {
                if p.profile == prof {
                    return Err(format!(
                        "profile 「{prof}」已在运行（PID {}），每个 profile 同时只能启动一个实例",
                        p.id
                    ));
                }
            }
        }
        Ok(launcher::launch(
            &settings,
            &target,
            &launch_args,
            &profile,
        ))
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))?
}

#[tauri::command]
pub async fn list_profiles() -> Result<Vec<crate::profiles::ProfileInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(crate::profiles::scan_profiles()))
        .await
        .map_err(|e| format!("扫描 profile 失败: {e}"))?
}

/// 内嵌启动：dsh 作为启动器子进程运行，日志回传界面，启动器退出即全部结束
#[tauri::command]
pub async fn start_embedded(
    app: AppHandle,
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: Option<String>,
    profile: Option<String>,
    args: Option<String>,
) -> Result<crate::procs::ProcInfo, String> {
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let installed = crate::installed::collect_installed(&settings);
        let target = match version.as_deref() {
            Some(v) if v != "unknown" => installed
                .iter()
                .find(|i| i.version == v)
                .cloned()
                .ok_or_else(|| format!("未找到已安装的 {v}，请先安装"))?,
            _ => crate::installed::pick_latest(&installed)
                .ok_or_else(|| "还没有已安装的 dsh 版本，请先在列表中选择安装".to_string())?,
        };
        let launch_args = args
            .or(Some(settings.default_args.clone()))
            .unwrap_or_default();
        let profile = profile.unwrap_or_else(|| settings.default_profile.clone());
        // 每个 profile 同时只能有一个实例
        let prof = profile.trim();
        if !prof.is_empty() {
            for p in crate::procs::list(&proc_state) {
                if p.profile == prof {
                    return Err(format!(
                        "profile 「{prof}」已在运行（PID {}），每个 profile 同时只能启动一个实例",
                        p.id
                    ));
                }
            }
        }
        crate::procs::spawn_embedded(
            app,
            &proc_state,
            &settings,
            &target,
            &profile,
            &launch_args,
        )
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))?
}

#[tauri::command]
pub fn stop_process(
    procs: State<'_, crate::procs::ProcState>,
    id: u32,
) -> Result<bool, String> {
    Ok(crate::procs::stop(&procs, id))
}

#[tauri::command]
pub fn list_processes(
    procs: State<'_, crate::procs::ProcState>,
) -> Result<Vec<crate::procs::ProcInfo>, String> {
    Ok(crate::procs::list(&procs))
}

#[tauri::command]
pub async fn check_launcher_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LauncherUpdateStatus, String> {
    let current = app.package_info().version.to_string();
    let url = state.settings.lock().unwrap().update_manifest_url.clone();
    if url.trim().is_empty() {
        return Ok(LauncherUpdateStatus {
            available: false,
            current,
            latest: None,
            notes: None,
            url: None,
            mode: "unconfigured".into(),
            message: Some(
                "未配置启动器更新源。可在设置中填写自建更新清单地址；正式发布时可启用 Tauri updater（签名更新）".into(),
            ),
        });
    }
    Ok(update_check::check_manifest(url.trim(), &current).await)
}

#[tauri::command]
pub fn reveal_folder(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| format!("打开目录失败: {e}"))
}

#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| format!("打开链接失败: {e}"))
}

/// 一键安装内置 Node 运行时（下载默认走镜像站）
#[tauri::command]
pub async fn install_runtime(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::runtime::install(app, &settings).await
}

/// 启动器启动时推送一次自动检查结果给前端
pub fn emit_startup_checks(app: &AppHandle) {
    let settings = settings::load_settings();
    if settings.auto_check_update {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let current = app2.package_info().version.to_string();
            let status = if settings.update_manifest_url.trim().is_empty() {
                None
            } else {
                Some(
                    update_check::check_manifest(settings.update_manifest_url.trim(), &current)
                        .await,
                )
            };
            if let Some(s) = status {
                if s.available {
                    let _ = app2.emit("launcher-update", s);
                }
            }
        });
    }
}
