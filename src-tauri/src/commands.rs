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

/// 启动 dsh：detached=false 子进程（日志回传界面，启动器退出即结束）；
/// detached=true 独立进程（自成进程组、日志写文件，启动器退出后继续运行，重启后由扫描识别）。
/// 缺省跟随设置里的 launchMode。
#[tauri::command]
pub async fn start_embedded(
    app: AppHandle,
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: Option<String>,
    profile: Option<String>,
    args: Option<String>,
    detached: Option<bool>,
) -> Result<crate::procs::ProcInfo, String> {
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    let detached = detached.unwrap_or(settings.launch_mode == "detached");

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

    // 2) 独立进程：直接 spawn（无 keeper、无日志管道），实例由 /proc 扫描感知
    if detached {
        return tauri::async_runtime::spawn_blocking(move || {
            crate::procs::spawn_detached(&settings, &prep.0, &prep.1, &prep.2)
        })
        .await
        .map_err(|e| format!("启动失败: {e}"))?;
    }

    // 3) keeper 线程 spawn dsh 并守候退出（线程存活期 == 子进程存活期，
    //    PDEATHSIG 绑定的是该线程，不能放进约 10s 就回收的 tokio 线程池）
    let rx = crate::procs::spawn_keeper(app, proc_state, settings, prep.0, prep.1, prep.2);

    // 4) 等待 spawn 结果
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
    // 独立进程注册表（macOS/Windows 没有 /proc，唯一性守卫跨平台依赖它）
    for r in crate::procs::validate_detached_registry() {
        if r.profile == prof {
            return Err(format!(
                "独立进程 profile 「{prof}」正在运行（PID {}），每个 profile 同时只能启动一个实例",
                r.pid
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
    // 端口冲突预检：profile 的 web 端口已经有人在监听时直接给出可操作的错误。
    // 否则要等 dsh 拉起、bind 失败、秒退，日志里只剩一行难懂的报错。
    // 同时这也是「cmdline 认不出、但端口占着」的外部实例的兜底守卫。
    if let Some((host, port)) = crate::profile_cfg::web_addr(prof) {
        use crate::procs::PortOwner;
        let who = match crate::procs::port_owner(&host, port, prof) {
            None => None,
            Some(PortOwner::ThisProfile(pid)) => Some(format!(
                "profile「{prof}」已有的实例（PID {pid}）"
            )),
            Some(PortOwner::OtherProfile { pid, profile: other }) => {
                Some(format!("profile「{other}」的 dsh 实例（PID {pid}）"))
            }
            Some(PortOwner::OtherProcess(pid)) => Some(format!("其它进程（PID {pid}）")),
            Some(PortOwner::Unknown) => Some("另一个进程".to_string()),
        };
        if let Some(who) = who {
            return Err(format!(
                "profile「{prof}」的 web 端口 {port} 已被{who}占用，无法启动；请先停止它，或到「快捷配置」改用其它端口"
            ));
        }
    }
    Ok(())
}

/// 停止一个实例。先按内嵌子进程找；找不到就按 PID 找独立进程/终端外部启动的 dsh。
/// 这样界面上任何被发现的实例都有统一的「停止」入口（内嵌实例带日志管道，
/// 独立/外部实例没有，只能按 PID 结束）。
#[tauri::command]
pub fn stop_process(
    app: AppHandle,
    procs: State<'_, crate::procs::ProcState>,
    id: u32,
) -> Result<bool, String> {
    if crate::procs::stop(&app, &procs, id) {
        return Ok(true);
    }
    crate::procs::stop_external_pid(id)
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

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceLog {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

/// 读取独立进程实例的日志尾部（内嵌实例走日志管道，外部实例没有文件日志 → None）。
/// 只按 PID 从注册表解析路径，避免把「读任意文件」暴露给前端。
#[tauri::command]
pub fn read_instance_log(
    pid: u32,
    max_bytes: Option<u32>,
) -> Result<Option<InstanceLog>, String> {
    let max = max_bytes.unwrap_or(64 * 1024) as usize;
    Ok(crate::procs::read_instance_log_tail(pid, max)?.map(|(path, content, truncated)| {
        InstanceLog {
            path,
            content,
            truncated,
        }
    }))
}

/// profile 还有实例在运行时不允许改名/删除：会让运行中的实例指向错目录
fn ensure_profile_idle(procs: &crate::procs::ProcState, name: &str) -> Result<(), String> {
    if let Some(i) = crate::procs::profile_instances(procs)
        .into_iter()
        .find(|i| i.profile == name)
    {
        let pid = i
            .pid
            .map(|p| format!("PID {p}"))
            .unwrap_or_else(|| "PID 未知".into());
        return Err(format!(
            "profile「{name}」还有实例在运行（{pid}），请先停止再操作"
        ));
    }
    Ok(())
}

/// 重命名 profile。dsh 内置保留 profile（headless/web/desktop）由后端拒绝。
/// 默认 profile 若指向它，一并跟随改名，避免启动器指向不存在的名字。
#[tauri::command]
pub fn rename_profile(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    name: String,
    new_name: String,
) -> Result<String, String> {
    let old = name.trim().to_string();
    ensure_profile_idle(&procs, &old)?;
    crate::profile_cfg::rename_profile(&old, &new_name)?;
    let new = new_name.trim().to_string();
    // 独立进程登记表里也记着 profile 名，不同步会在实例列表里留下旧名的幽灵条目
    crate::procs::rename_detached_profile(&old, &new);
    let mut s = state.settings.lock().unwrap();
    if s.default_profile == old {
        s.default_profile = new.clone();
        let _ = settings::save_settings(&s);
    }
    Ok(new)
}

/// 删除 profile：移入 ~/.dsh-launcher/deleted-profiles/ 可找回；
/// dsh 内置保留 profile 由后端拒绝。
#[tauri::command]
pub fn delete_profile(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    name: String,
) -> Result<String, String> {
    let name = name.trim().to_string();
    ensure_profile_idle(&procs, &name)?;
    let trash = crate::profile_cfg::delete_profile(&name)?;
    crate::procs::drop_detached_profile(&name);
    let mut s = state.settings.lock().unwrap();
    if s.default_profile == name {
        s.default_profile = String::new();
        let _ = settings::save_settings(&s);
    }
    Ok(trash)
}

/// 回收站列表（删除的 profile 只是被移入这里，可还原或彻底删除）
#[tauri::command]
pub fn list_deleted_profiles() -> Vec<crate::profile_cfg::DeletedProfile> {
    crate::profile_cfg::list_deleted_profiles()
}

/// 把回收站条目还原回 profiles 目录
#[tauri::command]
pub fn restore_deleted_profile(dir_name: String) -> Result<String, String> {
    crate::profile_cfg::restore_deleted_profile(&dir_name)
}

/// 从回收站彻底删除（不可恢复）
#[tauri::command]
pub fn purge_deleted_profile(dir_name: String) -> Result<(), String> {
    crate::profile_cfg::purge_deleted_profile(&dir_name)
}

/// 生成「恢复模式」profile（用 dsh 的 `--profile web-Recovery --from-default-profile web`
/// 从随附模板新建，再把端口等快捷配置写进它自己的 cordis.patch.yml）。
/// 只在用户显式点击并确认后调用，启动器不会自动创建。
#[tauri::command]
pub async fn create_recovery_profile(
    state: State<'_, AppState>,
) -> Result<crate::profile_cfg::RecoveryCreated, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || crate::profile_cfg::create_recovery_profile(&settings))
        .await
        .map_err(|e| format!("创建恢复模式失败: {e}"))?
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
pub async fn get_profile_detail(
    state: State<'_, AppState>,
    profile: String,
) -> Result<crate::profile_cfg::ProfileDetail, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || crate::profile_cfg::read_detail(&settings, &profile))
        .await
        .map_err(|e| format!("读取配置详情失败: {e}"))?
}

#[tauri::command]
pub fn get_patch_reload(profile: String) -> Result<String, String> {
    Ok(crate::profile_cfg::patch_reload_mode(&profile))
}

#[tauri::command]
pub async fn set_bundle_enabled(
    state: State<'_, AppState>,
    profile: String,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::profile_cfg::set_bundle_enabled(&settings, &profile, &name, enabled)
    })
    .await
    .map_err(|e| format!("切换插件包状态失败: {e}"))?
}

// ── 插件管理（全部走官方 dsh plugin 命令，输出实时进内置终端） ──────

/// 安装 / 升级插件：`dsh plugin --profile <p> add <spec>`（可一次给多个规格，
/// 在同一个任务里按顺序执行，输出合并进同一段终端记录）
/// mode: install | upgrade（只影响终端里的任务标签，命令完全一致）
#[tauri::command]
pub fn plugin_install(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    profile: String,
    specs: Vec<String>,
    mode: Option<String>,
) -> Result<u64, String> {
    let raw_specs: Vec<String> = specs
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if raw_specs.is_empty() {
        return Err("安装规格为空".into());
    }
    // github:owner/repo（含去 sha 的升级规格）在任务里先做匿名可读预检：
    // 私有仓库直接拒绝；判不出来就放行（不让网络问题挡住安装）
    let is_github = |s: &str| {
        crate::registry::parse_github_spec(s)
            .map(|sp| sp.tarball_url.is_none() && !sp.owner.is_empty() && !sp.repo.is_empty())
            .unwrap_or(false)
    };
    let settings = state.settings.lock().unwrap().clone();
    let upgrade = mode.as_deref() == Some("upgrade");
    // GitHub 加速：目标若是 github 的 http 链接（releases 资产 / raw 等），直接换成代理链接；
    // github:owner/repo 之类交给 pnpm 解析的规格改不了链接，但 git 部分仍吃 insteadOf 加速
    let accel_pref = if settings.github_accel {
        let p = settings.github_proxy.trim();
        let pref = if p.is_empty() { None } else { Some(p.to_string()) };
        let _ = crate::ghaccel::ensure_cached(crate::ghaccel::CACHE_TTL_SECS);
        crate::ghaccel::current().and_then(|a| crate::ghaccel::pick_prefix(&a, pref.as_deref(), false))
    } else {
        None
    };
    let specs: Vec<String> = raw_specs
        .into_iter()
        .map(|s| match &accel_pref {
            Some(p) if crate::ghaccel::is_github_url(&s) => crate::ghaccel::rewrite(&s, p),
            _ => s,
        })
        .collect();
    let mut steps = vec![crate::plugin::Step::Note {
        text: format!(
            "{}：{}",
            if upgrade { "升级（重新走官方安装命令）" } else { "安装" },
            specs.join(" + ")
        ),
    }];
    if let Some(p) = &accel_pref {
        if specs.iter().any(|s| s.starts_with(p.as_str())) {
            steps.push(crate::plugin::Step::Note {
                text: format!("⚡ GitHub 加速：链接已改写为 {p}<原链接>"),
            });
        }
    }

    for spec in &specs {
        if is_github(spec) {
            // 只做「远端可匿名访问」的轻量预检（git ls-remote 级别，秒级、结果确定）。
            // 不再扫文件树：jsDelivr 的索引是缓存快照，会给不全的候选与错误的 lib/ 判定，
            // 反而把能装的包拦下来；能不能加载由装后校验负责。
            steps.push(crate::plugin::Step::ProbeRemote { url: spec.clone() });
        }
        steps.push(crate::plugin::Step::Dsh {
            args: vec!["add".into(), spec.clone()],
        });
    }
    let label = if specs.len() == 1 {
        format!("{} {}", if upgrade { "升级" } else { "安装" }, specs[0])
    } else {
        format!("{} {} 个包", if upgrade { "升级" } else { "安装" }, specs.len())
    };
    crate::plugin::start_job(
        app,
        &jobs,
        settings,
        crate::plugin::JobRequest {
            profile,
            kind: if upgrade { "upgrade".into() } else { "install".into() },
            label,
            steps,
        },
    )
}

/// 卸载插件：`dsh plugin --profile <p> remove <name>`（可选顺带删除本地克隆目录）
#[tauri::command]
pub fn plugin_uninstall(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    profile: String,
    name: String,
    purge_clone_dir: Option<String>,
) -> Result<u64, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("插件名为空".into());
    }
    // 宿主自带的插件包不允许卸载：卸掉 base / web-app 会让 profile 起不来
    if crate::profile_cfg::is_inbox_bundle(&name) {
        return Err(crate::profile_cfg::inbox_bundle_reject(&name, "卸载"));
    }
    let settings = state.settings.lock().unwrap().clone();
    let mut steps = vec![
        crate::plugin::Step::Note {
            text: format!(
                "卸载 {name}：dsh plugin remove（同时从 dsh.profile.bundles 与依赖声明移除）"
            ),
        },
        crate::plugin::Step::Dsh {
            args: vec!["remove".into(), name.clone()],
        },
    ];
    let mut label = format!("卸载 {name}");
    if let Some(dir) = purge_clone_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let path = crate::plugin::clone_dir_path(dir)?;
        label = format!("卸载 {name} + 删除克隆");
        steps.push(crate::plugin::Step::RmDir { path });
    }
    crate::plugin::start_job(
        app,
        &jobs,
        settings,
        crate::plugin::JobRequest {
            profile,
            kind: "uninstall".into(),
            label,
            steps,
        },
    )
}

/// 升级本地克隆插件：git pull --ff-only →（可选）重新构建 → dsh plugin add link:<dir>
#[tauri::command]
pub fn plugin_pull_update(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    profile: String,
    name: String,
    clone_root: String,
    sub_path: Option<String>,
    build: Option<bool>,
) -> Result<u64, String> {
    let settings = state.settings.lock().unwrap().clone();
    let root = std::path::PathBuf::from(clone_root.trim());
    if clone_root.trim().is_empty() {
        return Err("缺少本地仓库路径".into());
    }
    let (steps, _root) =
        crate::plugin::steps_for_pull_update_root(&root, sub_path.as_deref(), build.unwrap_or(false))?;
    crate::plugin::start_job(
        app,
        &jobs,
        settings,
        crate::plugin::JobRequest {
            profile,
            kind: "pull".into(),
            label: format!("git pull 升级 {name}"),
            steps,
        },
    )
}

/// clone 仓库 + 本地 link 安装：git clone →（可选）构建 → dsh plugin add link:<dir>
///
/// 起任务前先做**匿名可读预检**：私有仓库 / 不存在的仓库直接拒绝（不在终端里弹登录、
/// 也不产生注定失败的任务），指引用户手动 clone 后走 link 安装。
///
/// `accel`：本次任务是否走 GitHub 加速（缺省跟随设置）。
#[tauri::command]
pub fn plugin_clone_install(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    profile: String,
    input: crate::plugin::CloneInstallInput,
    accel: Option<bool>,
) -> Result<u64, String> {
    let mut settings = state.settings.lock().unwrap().clone();
    if let Some(a) = accel {
        settings.github_accel = a;
    }
    let (mut steps, root) = crate::plugin::steps_for_clone_install(&input)?;
    // 预检放任务第一步：点击后立刻出现任务与输出，不再让界面干等网络
    steps.insert(
        0,
        crate::plugin::Step::ProbeRemote {
            url: input.url.clone(),
        },
    );
    let label = format!(
        "clone 安装 {}",
        root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    );
    crate::plugin::start_job(
        app,
        &jobs,
        settings,
        crate::plugin::JobRequest {
            profile,
            kind: "clone".into(),
            label,
            steps,
        },
    )
}

/// GitHub 加速状态（候选前缀 → 下载/git 两段测速 → 缓存）。
///
/// force=true 重新测速（设置页「重新测速」）；否则**有缓存就直接返回**（哪怕过期），
/// 只有完全没有缓存时才现测——避免打开对话框就触发一轮真实浅克隆测速。
#[tauri::command]
pub async fn get_github_accel(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<crate::ghaccel::GhAccel, String> {
    let force = force.unwrap_or(false);
    let extra = state.settings.lock().unwrap().github_proxy_extra.clone();
    if !force {
        if let Some(cached) = crate::ghaccel::ensure_cached(crate::ghaccel::CACHE_TTL_SECS) {
            return Ok(cached);
        }
    }
    crate::ghaccel::refresh(&extra, force).await
}

/// 探测一个 git 仓库里的插件包：**先真克隆（或同步）到 git-plugins，再本地扫描**。
///
/// 这是启动器唯一保留的「探测」——探测结果来自真实工作树（本地按 package.json /
/// dsh.bundle / lib 判定），不依赖 jsDelivr 索引或 GitHub API 的文件树，因此不会
/// 出现"候选少几个""明明有 lib 却说缺失"这类偏差。克隆结果同时就是安装要用的目录。
#[tauri::command]
pub async fn probe_clone_repo(
    state: State<'_, AppState>,
    url: String,
    git_ref: Option<String>,
    accel: Option<bool>,
) -> Result<crate::plugin::CloneProbe, String> {
    let settings = state.settings.lock().unwrap().clone();
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err("git 远端地址为空".into());
    }
    if !(url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("git@")
        || url.starts_with("ssh://")
        || url.starts_with("git://"))
    {
        return Err(format!("不支持的 git 远端地址：{url}"));
    }
    let want_accel = accel.unwrap_or(settings.github_accel);
    let pref: Option<String> = {
        let p = settings.github_proxy.trim();
        if p.is_empty() { None } else { Some(p.to_string()) }
    };
    // 首次使用先测速（缓存 6 小时）；失败不拦克隆，只是没有加速
    let mut accel_summary: Option<String> = None;
    if want_accel {
        let _ = crate::ghaccel::ensure_cached(crate::ghaccel::CACHE_TTL_SECS);
        if let Err(e) = crate::ghaccel::refresh(&settings.github_proxy_extra, false).await {
            eprintln!("[ghaccel] 加速不可用：{e}");
        }
        if let Some(a) = crate::ghaccel::current() {
            if !a.is_empty() {
                accel_summary = Some(crate::ghaccel::summary(&a, pref.as_deref(), true));
            }
        }
    }

    let dir_name = crate::plugin::clone_dir_for_url(&url);
    let root = crate::plugin::git_plugins_dir().join(&dir_name);
    let clone_url = url.clone();
    let ref_clone = git_ref.clone().filter(|r| !r.trim().is_empty());
    // git_ref 会直接进 `git checkout <ref>` / `git clone --branch <ref>`，必须先校验，
    // 否则以 - 开头的值会被当成 git 选项（参数注入），含 / .. 的可越界。
    if let Some(r) = &ref_clone {
        if !crate::registry::is_safe_git_ref(r) {
            return Err(format!("非法的分支 / 标签名：{r}"));
        }
    }
    let ref_for_scan = ref_clone.clone();
    let root_clone = root.clone();
    let accelerated = want_accel;

    let output = tauri::async_runtime::spawn_blocking(move || -> Result<(bool, String), String> {
        let git = std::path::PathBuf::from("git");
        let mut envs = crate::plugin::git_no_prompt_env();
        if accelerated {
            if let Some(a) = crate::ghaccel::current() {
                envs.extend(crate::ghaccel::git_env(&a, pref.as_deref()));
            }
        }
        let existed = root_clone.join(".git").is_dir();
        let timeout = std::time::Duration::from_secs(if existed { 180 } else { 300 });
        if existed {
            let dir = root_clone.to_string_lossy().into_owned();
            let (ok, text) = util::run_captured_in(
                &git,
                &["-C".to_string(), dir.clone(), "fetch".to_string(), "--all".to_string(), "--prune".to_string()],
                None,
                &envs,
                timeout,
            )
            .ok_or_else(|| "启动 git 失败（确认 git 在 PATH 中）".to_string())?;
            if !ok {
                return Ok((false, text));
            }
            match &ref_clone {
                Some(r) => {
                    let (ok2, text2) = util::run_captured_in(
                        &git,
                        &["-C".to_string(), dir, "checkout".to_string(), r.clone()],
                        None,
                        &envs,
                        timeout,
                    )
                    .ok_or_else(|| "启动 git 失败".to_string())?;
                    return Ok((ok2, text2));
                }
                None => {
                    let (ok2, text2) = util::run_captured_in(
                        &git,
                        &["-C".to_string(), dir, "pull".to_string(), "--ff-only".to_string()],
                        None,
                        &envs,
                        timeout,
                    )
                    .ok_or_else(|| "启动 git 失败".to_string())?;
                    return Ok((ok2, text2));
                }
            }
        }
        let mut args: Vec<String> = vec!["clone".into(), "--depth".into(), "1".into()];
        if let Some(r) = &ref_clone {
            args.push("--branch".into());
            args.push(r.clone());
        }
        args.push(clone_url.clone());
        args.push(root_clone.to_string_lossy().into_owned());
        let (ok, text) = util::run_captured_in(&git, &args, None, &envs, timeout)
            .ok_or_else(|| "启动 git 失败（确认 git 在 PATH 中）".to_string())?;
        Ok((ok, text))
    })
    .await
    .map_err(|e| format!("克隆任务失败: {e}"))??;

    let (ok, text) = output;
    if !ok {
        // 私有仓库 / 网络不通：把 git 的原话带回去，别让用户猜
        if let Some(spec) = registry::parse_github_spec(&url) {
            if crate::plugin::looks_like_auth_error(&text) {
                return Err(registry::private_repo_reject(&spec.owner, &spec.repo));
            }
        }
        return Err(format!("克隆失败：{}", text.lines().rev().take(4).collect::<Vec<_>>().join(" / ")));
    }

    let candidates = crate::registry::probe_local_plugins(&root)?;
    Ok(crate::plugin::CloneProbe {
        url: url.clone(),
        root: root.to_string_lossy().into_owned(),
        dir_name,
        git_ref: ref_for_scan,
        accel: accel_summary,
        candidates,
    })
}

#[tauri::command]
pub fn list_plugin_jobs(jobs: State<'_, crate::plugin::PluginJobState>) -> Vec<crate::plugin::PluginJob> {
    jobs.snapshot()
}

#[tauri::command]
pub fn cancel_plugin_job(jobs: State<'_, crate::plugin::PluginJobState>, job_id: u64) -> bool {
    jobs.cancel(job_id)
}

/// 重试一个失败的任务：**原样重放它的全部步骤**（新任务，输出同样进内置终端）。
///
/// 为什么重放整个任务而不是只重跑最后一条命令：clone 安装是多步的
/// （预检 → git clone/pull → 可选构建 → dsh plugin add link:…），中途失败时
/// 只重跑最后一步是错的。为此 `git clone` 目标已存在、`rm -rf` 目标已消失
/// 这两种重试必然遇到的情况都做成了幂等。
#[tauri::command]
pub fn plugin_retry_job(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    job_id: u64,
) -> Result<u64, String> {
    let (req, running) = jobs
        .job_request(job_id)
        .ok_or_else(|| format!("任务 {job_id} 不存在或已被清理（无法重试）"))?;
    if running {
        return Err("任务还在运行中：请先取消或等它结束".into());
    }
    if req.steps.is_empty() {
        return Err("这个任务没有可重放的步骤".into());
    }
    let settings = state.settings.lock().unwrap().clone();
    let retry = crate::plugin::JobRequest {
        profile: req.profile.clone(),
        kind: req.kind.clone(),
        label: format!("重试：{}", req.label),
        steps: req.steps.clone(),
    };
    crate::plugin::start_job(app, &jobs, settings, retry)
}

/// 放行被 pnpm 拦下的构建脚本，并把原命令原样重跑一次。
///
/// 构建脚本会执行第三方代码，属于**用户的决定**：只有用户点「允许构建脚本并重试」
/// 才会走到这里（启动器不默认放行）。写入 profile 的 pnpm-workspace.yaml →
/// allowBuilds（合并已有条目、保留注释与行尾、写前备份），重跑依旧走 `dsh plugin`。
#[tauri::command]
pub fn plugin_approve_builds(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, crate::plugin::PluginJobState>,
    job_id: u64,
) -> Result<u64, String> {
    let (profile, argv, pending) = jobs
        .job_retry_info(job_id)
        .ok_or_else(|| format!("任务 {job_id} 不存在（可能已被清理）"))?;
    if pending.is_empty() {
        return Err("这个任务没有被拦下的构建脚本".into());
    }
    if argv.is_empty() {
        return Err("这个任务没有可重跑的参数".into());
    }
    let dir = crate::profiles::profiles_dir().join(&profile);
    let allowed = crate::verify::set_allow_builds(&dir, &pending)?;
    let settings = state.settings.lock().unwrap().clone();
    let label = format!("重试（已放行构建脚本：{}）", pending.join("、"));
    let id = crate::plugin::start_job(
        app,
        &jobs,
        settings,
        crate::plugin::JobRequest {
            profile,
            kind: "install".into(),
            label,
            steps: vec![
                crate::plugin::Step::Note {
                    text: format!(
                        "已在 {} 的 allowBuilds 里放行：{}（当前共 {} 条）",
                        dir.join("pnpm-workspace.yaml").display(),
                        pending.join("、"),
                        allowed.len()
                    ),
                },
                crate::plugin::Step::Dsh { args: argv },
            ],
        },
    )?;
    Ok(id)
}

#[tauri::command]
pub fn clear_plugin_jobs(jobs: State<'_, crate::plugin::PluginJobState>) -> usize {
    jobs.clear_finished()
}

/// 把某个插件任务的完整日志导出到 ~/.dsh-launcher/logs/，返回文件路径
#[tauri::command]
pub fn export_plugin_job_log(
    jobs: State<'_, crate::plugin::PluginJobState>,
    job_id: u64,
) -> Result<String, String> {
    let text = jobs
        .job_log_text(job_id)
        .ok_or_else(|| format!("任务 {job_id} 不存在（可能已被清理）"))?;
    let dir = settings::launcher_home().join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("dsh-plugin-job-{job_id}-{ts}.log"));
    std::fs::write(&path, text).map_err(|e| format!("写日志失败: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// 列出 ~/.dsh-launcher/git-plugins 下的克隆仓库（git 状态 + 插件候选）
#[tauri::command]
pub async fn list_cloned_plugins() -> Result<Vec<crate::plugin::ClonedPlugin>, String> {
    tauri::async_runtime::spawn_blocking(crate::plugin::list_cloned)
        .await
        .map_err(|e| format!("读取克隆仓库失败: {e}"))
}

/// 探测本地目录里的插件包（monorepo 子包一并列出）
#[tauri::command]
pub async fn probe_local_plugins(
    path: String,
) -> Result<Vec<crate::registry::PluginCandidate>, String> {
    let p = path.trim().to_string();
    if p.is_empty() {
        return Err("目录为空".into());
    }
    tauri::async_runtime::spawn_blocking(move || crate::registry::probe_local_plugins(std::path::Path::new(&p)))
        .await
        .map_err(|e| format!("探测失败: {e}"))?
}

#[tauri::command]
pub fn delete_cloned_plugin(dir_name: String) -> Result<(), String> {
    crate::plugin::remove_clone_dir(&dir_name)
}

/// 返回 git-plugins 目录路径（不存在则创建），供前端在文件管理器里打开
#[tauri::command]
pub fn reveal_git_plugins_dir() -> Result<String, String> {
    let dir = crate::plugin::git_plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
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
pub fn get_web_quick_config(
    profile: String,
) -> Result<crate::profile_cfg::WebQuickConfig, String> {
    crate::profile_cfg::get_web_quick_config(&profile)
}

#[tauri::command]
pub fn set_web_quick_config(
    profile: String,
    config: crate::profile_cfg::WebQuickConfigInput,
) -> Result<(), String> {
    crate::profile_cfg::set_web_quick_config(&profile, &config)
}

/// 复制 profile。复制后**自动错开 web 端口**（复用「邻近空闲端口」那套逻辑），
/// 否则两个实例配置同一个端口、无法并行启动。返回新端口（null = 该 profile 没有 webserver 配置）。
#[tauri::command]
pub fn copy_profile(source: String, new_name: String) -> Result<Option<u16>, String> {
    crate::profile_cfg::copy_profile(&source, &new_name)?;
    crate::profile_cfg::assign_free_web_port(new_name.trim())
}

#[tauri::command]
pub async fn search_registry_packages(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<crate::registry::PackageSearchItem>, String> {
    let settings = state.settings.lock().unwrap().clone();
    crate::registry::search_packages(&settings.registry, &query).await
}

// 注：仓库文件树探测（fetch_github_repo）已随「GitHub 仓库」入口下线——
// jsDelivr 的文件索引是缓存快照，会给不全的候选与错误的 lib/ 判定。
// 现在唯一保留的探测是 probe_clone_repo（先克隆再本地扫描）。
// registry::fetch_github_repo 仍在（自检与测试用），但没有命令入口。

/// 通道自检：并发探测 github.com refs / jsDelivr / raw / api.github.com 四条通道的
/// 可达性与延迟（这台机器到 github.com 是间歇性不可达，有了这个能一眼看出当时哪条通）
#[tauri::command]
pub async fn check_channels(state: State<'_, AppState>) -> Result<Vec<crate::registry::ChannelProbe>, String> {
    let token = state.settings.lock().unwrap().github_token.clone();
    let token = crate::registry::github_token(Some(&token));
    Ok(crate::registry::check_channels(token.as_deref()).await)
}

/// 当前 GitHub API 额度（探测/更新检测已走免额度通道，这里只用于展示元数据额度）
#[tauri::command]
pub fn get_github_rate_limit() -> crate::registry::GitHubRateLimit {
    crate::registry::rate_limit()
}

#[tauri::command]
pub async fn check_plugin_updates(
    state: State<'_, AppState>,
    profile: String,
) -> Result<Vec<crate::profile_cfg::PluginUpdateInfo>, String> {
    let settings = state.settings.lock().unwrap().clone();
    let token = crate::registry::github_token(Some(&settings.github_token));
    crate::profile_cfg::check_plugin_updates(&settings.registry, &profile, token.as_deref()).await
}

#[tauri::command]
pub fn read_global_config() -> Result<String, String> {
    crate::profile_cfg::read_global_config()
}

#[tauri::command]
pub fn write_global_config(content: String) -> Result<(), String> {
    crate::profile_cfg::write_global_config(&content)
}

/// 读取模型配置（settings.yaml 的 llm-pi-ai.providers 与 agent-default-model）
#[tauri::command]
pub fn get_model_config() -> Result<crate::modelcfg::ModelConfig, String> {
    crate::modelcfg::read()
}

/// 保存模型配置：仅重写上述两节（其余内容与节外注释逐字节保留，写前自动备份）
#[tauri::command]
pub fn set_model_config(config: crate::modelcfg::ModelConfigInput) -> Result<(), String> {
    crate::modelcfg::write(&config)
}

/// 拉取服务方可用模型（GET {baseURL}/models；密钥：手动值 > 凭据 refs > 环境变量）
#[tauri::command]
pub async fn fetch_provider_models(
    base_url: String,
    api: String,
    api_key_env: String,
    api_key: Option<String>,
) -> Result<Vec<crate::modelcfg::RemoteModel>, String> {
    crate::modelcfg::fetch_provider_models(&base_url, &api, &api_key_env, api_key).await
}

#[tauri::command]
pub fn get_credentials() -> Result<crate::credentials::CredentialFile, String> {
    crate::credentials::read()
}

/// 整表保存凭据 refs（records / version 等其余顶层键原样保留，写前自动备份）
#[tauri::command]
pub fn write_credential_refs(
    refs: Vec<crate::credentials::CredentialRefInput>,
) -> Result<(), String> {
    crate::credentials::write_refs(&refs)
}

#[tauri::command]
pub async fn check_launcher_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LauncherUpdateStatus, String> {
    let settings = state.settings.lock().unwrap().clone();
    Ok(update_check::check(&app, &settings).await)
}

/// 下载并安装启动器新版本（内置 updater 模式）。
/// 非 Windows 平台上本调用不会返回：安装成功后直接重启应用。
#[tauri::command]
pub async fn install_launcher_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    update_check::install_builtin(&app, &settings).await
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

/// 启动器启动时推送一次自动检查结果给前端。
/// 清单地址为空时回落到内置 Tauri updater（能直接下载安装）。
/// 默认只提示、由用户决定是否升级；开了 autoInstallUpdate 且安装不需要提权
/// （AppImage / Windows / macOS）时才静默自动升级 —— deb/rpm 必然弹密码框，不静默。
pub fn emit_startup_checks(app: &AppHandle) {
    let settings = settings::load_settings();
    if !settings.auto_check_update {
        return;
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let status = update_check::check(&app2, &settings).await;
        if !status.available {
            return;
        }
        if settings.auto_install_update && status.mode == "builtin" && !status.needs_elevation {
            let _ = app2.emit("launcher-update", status);
            if let Err(e) = update_check::install_builtin(&app2, &settings).await {
                let _ = app2.emit("toast", format!("自动更新失败：{e}"));
            }
            return;
        }
        let _ = app2.emit("launcher-update", status);
    });
}
