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
        let node = util::find_node(&settings);
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
pub fn save_settings(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    settings: Settings,
) -> Result<(), String> {
    // 有实例运行时禁止切换当前版本：所有 profile 实例都基于 active 版本启动
    {
        let current = state.settings.lock().unwrap();
        if !current.active_version.is_empty() && current.active_version != settings.active_version {
            ensure_no_running_instance(&procs)?;
        }
    }
    settings::save_settings(&settings)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

/// 存在任何 dsh 实例（内嵌或终端/外部启动）时拒绝切换版本
fn ensure_no_running_instance(procs: &crate::procs::ProcState) -> Result<(), String> {
    let embedded = crate::procs::list(procs);
    if !embedded.is_empty() {
        let detail = embedded
            .iter()
            .map(|p| format!("「{}」(PID {})", p.profile, p.id))
            .collect::<Vec<_>>()
            .join("、");
        return Err(format!(
            "有 Profile 实例正在运行：{detail}。请先手动停止所有实例，再切换 DSH 版本"
        ));
    }
    let external = crate::procs::external_running_profile_pids(&[]);
    if !external.is_empty() {
        let detail = external
            .iter()
            .map(|(pid, prof)| {
                if prof.is_empty() {
                    format!("PID {pid}")
                } else {
                    format!("「{prof}」(PID {pid})")
                }
            })
            .collect::<Vec<_>>()
            .join("、");
        return Err(format!(
            "系统中有外部启动的 dsh 进程：{detail}。请先手动停止所有实例，再切换 DSH 版本"
        ));
    }
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
    procs: State<'_, crate::procs::ProcState>,
    version: String,
    force: Option<bool>,
) -> Result<bool, String> {
    if !util::is_safe_version(&version) {
        return Err("非法版本号".into());
    }
    // 安装会清空该版本的目录，正在运行时一律拒绝；force 只是前端区分"重装"的标记
    for p in crate::procs::list(&procs) {
        if p.version == version {
            return Err(format!(
                "dsh {version} 正在运行（PID {}），请先停止实例后再安装/重装",
                p.id
            ));
        }
    }
    if let Some(pid) = external_pid_on_version(&version) {
        return Err(format!(
            "dsh {version} 正在被外部实例使用（PID {pid}），请先停止后再安装/重装"
        ));
    }
    let _ = force;
    let settings = state.settings.lock().unwrap().clone();
    installer_start(&app, &install_state, &settings, &version)?;
    Ok(true)
}

fn installer_start(
    app: &AppHandle,
    install_state: &InstallState,
    settings: &Settings,
    version: &str,
) -> Result<(), String> {
    crate::installer::start_install(app.clone(), install_state, settings, version)
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
pub fn uninstall_version(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: String,
) -> Result<(), String> {
    // 该版本正在内嵌运行时不允许删除其安装目录
    for p in crate::procs::list(&procs) {
        if p.version == version {
            return Err(format!(
                "dsh {version} 正在运行（PID {}），请先停止再卸载",
                p.id
            ));
        }
    }
    // 终端/外部启动的实例跑在该版本目录上时同样禁止卸载
    if let Some(pid) = external_pid_on_version(&version) {
        return Err(format!(
            "dsh {version} 正在被外部实例使用（PID {pid}），请先停止后再卸载"
        ));
    }
    // 当前使用中的版本不允许卸载，避免所有 profile 失去运行基础
    if state.settings.lock().unwrap().active_version == version {
        return Err(format!(
            "{version} 是当前使用版本，请先在侧栏切换到其他版本后再卸载"
        ));
    }
    crate::installer::uninstall_managed(&version)
}

/// 外部（终端）启动的 dsh 实例正运行在该版本目录上时，返回其 PID
fn external_pid_on_version(version: &str) -> Option<u32> {
    let dir = crate::settings::versions_dir().join(version);
    crate::procs::external_pids_under_dir(&dir).first().copied()
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
        let target = resolve_target(&installed, version.as_deref(), &settings.active_version)?;
        let launch_args = args
            .or_else(|| Some(settings.default_args.clone()))
            .unwrap_or_default();
        let profile = profile.unwrap_or_else(|| settings.default_profile.clone());
        // 全局唯一性守卫：同一 profile（无论内嵌还是外部启动）只能有一个实例
        if let Err(msg) = ensure_profile_free(&proc_state, &profile) {
            return Err(msg.into());
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

/// 解析启动目标版本：显式指定 > 当前版本(active) > 最新已安装
fn resolve_target(
    installed: &[crate::installed::InstalledVersion],
    version: Option<&str>,
    active: &str,
) -> Result<crate::installed::InstalledVersion, String> {
    match version {
        Some(v) if v != "unknown" => installed
            .iter()
            .find(|i| i.version == v)
            .cloned()
            .ok_or_else(|| format!("未找到已安装的 {v}，请先安装")),
        _ => {
            let act = active.trim();
            let found = if act.is_empty() {
                None
            } else {
                installed.iter().find(|i| i.version == act)
            };
            match found {
                Some(i) => Ok(i.clone()),
                None => crate::installed::pick_latest(installed)
                    .ok_or_else(|| "还没有已安装的 dsh 版本，请先在版本列表中安装".to_string()),
            }
        }
    }
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

    // 1) 解析目标版本 + profile 唯一性守卫（线程池执行）
    let prep = tauri::async_runtime::spawn_blocking({
        let settings = settings.clone();
        let proc_state = proc_state.clone();
        move || {
            let installed = crate::installed::collect_installed(&settings);
            let target = resolve_target(&installed, version.as_deref(), &settings.active_version)?;
            let launch_args = args
                .or_else(|| Some(settings.default_args.clone()))
                .unwrap_or_default();
            let profile = profile.unwrap_or_else(|| settings.default_profile.clone());
            // 全局唯一性守卫：同一 profile（无论内嵌还是外部启动）只能有一个实例
            if let Err(msg) = ensure_profile_free(&proc_state, &profile) {
                return Err(msg);
            }
            Ok((target, profile, launch_args))
        }
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))??;

    // 2) keeper 线程 spawn dsh 并守候退出（线程存活期 == 子进程存活期，
    //    PDEATHSIG 绑定的是该线程，不能放进约 10s 就回收的 tokio 线程池）
    let rx = crate::procs::spawn_keeper(app, proc_state, settings, prep.0, prep.1, prep.2);

    // 3) 等待 spawn 结果
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|e| format!("dsh keeper 线程异常: {e}"))?
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))?
}

/// 校验 profile 未被任何实例（内嵌/外部）占用
fn ensure_profile_free(
    proc_state: &crate::procs::ProcState,
    profile: &str,
) -> Result<(), String> {
    let prof = profile.trim();
    if prof.is_empty() {
        return Ok(());
    }
    for p in crate::procs::list(proc_state) {
        if p.profile == prof {
            return Err(format!(
                "profile 「{prof}」已在运行（PID {}），每个 profile 同时只能启动一个实例",
                p.id
            ));
        }
    }
    for (pid, ext_prof) in crate::procs::external_running_profile_pids(&[]) {
        if ext_prof == prof {
            return Err(format!(
                "系统中已有 profile 「{prof}」的 dsh 进程（PID {pid}，可能由终端或外部启动），每个 profile 同时只能启动一个实例"
            ));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn stop_process(
    app: AppHandle,
    procs: State<'_, crate::procs::ProcState>,
    id: u32,
) -> Result<bool, String> {
    Ok(crate::procs::stop(&app, &procs, id))
}

#[tauri::command]
pub fn list_processes(
    procs: State<'_, crate::procs::ProcState>,
) -> Result<Vec<crate::procs::ProcInfo>, String> {
    Ok(crate::procs::list(&procs))
}

/// 各 profile 实例状态（含终端/外部启动的）
#[tauri::command]
pub fn list_profile_instances(
    procs: State<'_, crate::procs::ProcState>,
) -> Result<Vec<crate::procs::ProfileInstance>, String> {
    Ok(crate::procs::profile_instances(&procs))
}

/// 停止某个 profile 的实例（内嵌或外部）
#[tauri::command]
pub fn stop_profile_instance(
    app: AppHandle,
    procs: State<'_, crate::procs::ProcState>,
    profile: String,
) -> Result<bool, String> {
    crate::procs::stop_profile(&app, &procs, &profile)
}

/// 把运行日志导出到 ~/.dsh-launcher/logs/
#[tauri::command]
pub fn export_proc_log(
    profile: String,
    pid: u32,
    content: String,
) -> Result<String, String> {
    let dir = settings::launcher_home().join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let safe_profile = profile.trim().replace(['/', '\\', ' '], "_");
    let path = dir.join(format!("dsh-{}-{}-{}.log", safe_profile, pid, ts));
    std::fs::write(&path, content).map_err(|e| format!("写日志失败: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

// ── 插件管理与配置文件 ──────────────────────

#[tauri::command]
pub fn get_profile_detail(
    state: State<'_, AppState>,
    profile: String,
) -> Result<crate::profile_cfg::ProfileDetail, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::profile_cfg::read_detail(&settings, &profile)
}

#[tauri::command]
pub fn get_patch_reload(profile: String) -> Result<String, String> {
    Ok(crate::profile_cfg::patch_reload_mode(&profile))
}

#[tauri::command]
pub fn set_bundle_enabled(
    state: State<'_, AppState>,
    profile: String,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::profile_cfg::set_bundle_enabled(&settings, &profile, &name, enabled)
}

#[tauri::command]
pub fn uninstall_bundle(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: String,
    name: String,
) -> Result<bool, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::profile_cfg::uninstall_bundle(app, &settings, &profile, &name)?;
    Ok(true)
}

#[tauri::command]
pub fn install_bundle(
    app: AppHandle,
    state: State<'_, AppState>,
    profile: String,
    name: String,
) -> Result<bool, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::profile_cfg::install_bundle(app, &settings, &profile, &name)?;
    Ok(true)
}

#[tauri::command]
pub fn read_profile_file(profile: String, file: String) -> Result<String, String> {
    crate::profile_cfg::read_profile_file(&profile, &file)
}

#[tauri::command]
pub fn write_profile_file(
    profile: String,
    file: String,
    content: String,
) -> Result<(), String> {
    crate::profile_cfg::write_profile_file(&profile, &file, &content)
}

#[tauri::command]
pub fn read_global_config() -> Result<String, String> {
    crate::profile_cfg::read_global_config()
}

#[tauri::command]
pub fn write_global_config(content: String) -> Result<(), String> {
    crate::profile_cfg::write_global_config(&content)
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
