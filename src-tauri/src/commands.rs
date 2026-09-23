use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::installer::InstallState;
use crate::starter::{self, LaunchResult};
use crate::registry::{self, RegistryInfo};
use crate::settings::{self, AppState, Settings};
use crate::update_check::{self, StarterUpdateStatus};
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
    /// 日志目录（启动留痕 / 安装与解压的操作日志都在这里）
    pub logs_dir: String,
    /// 当前生效的系统日志级别（小写 debug/info/warn/error）
    pub log_level: String,
    /// 级别是否被 DSH_STARTER_LOG 环境变量锁定
    pub log_level_pinned: bool,
    /// dsh 是否已经初始化过（`$DSH_HOME/profiles` 里有没有 profile）。
    /// 全新机器上这是 false：此时 profile 列表、快捷配置、插件管理全都无从谈起，
    /// 前端据此给出「首次初始化（跑一次 dsh web）」的引导。
    pub dsh_initialized: bool,
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
                let v = util::run_captured(&inv.program, &args, std::time::Duration::from_secs(10));
                if v.is_none() {
                    // 界面上只会显示「npm 未装」，真实原因得看日志
                    crate::diag::op(
                        "app",
                        &format!(
                            "npm --version 执行失败：{} {}\n  PATH: {}",
                            inv.program.display(),
                            inv.args.join(" "),
                            std::env::var("PATH").unwrap_or_default()
                        ),
                    );
                }
                v
            }
            None => {
                crate::diag::op(
                    "app",
                    &format!(
                        "未找到 npm；node={} PATH={}",
                        node.as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "（未找到）".into()),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                );
                None
            }
        };
        Ok(EnvironmentInfo {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            node: node_version.map(|v| v.trim_start_matches('v').to_string()),
            node_path: node.map(|p| p.to_string_lossy().into_owned()),
            npm: npm_version,
            npm_path: npm.map(|i| i.program.to_string_lossy().into_owned()),
            dsh_home: settings::starter_home().to_string_lossy().into_owned(),
            versions_dir: settings::versions_dir().to_string_lossy().into_owned(),
            registry: settings.registry,
            dsh_native_home: crate::profiles::dsh_native_home().to_string_lossy().into_owned(),
            profiles_dir: crate::profiles::profiles_dir().to_string_lossy().into_owned(),
            runtime_installed: crate::runtime::runtime_installed(),
            runtime_dir: crate::runtime::runtime_dir().to_string_lossy().into_owned(),
            logs_dir: crate::diag::logs_dir().to_string_lossy().into_owned(),
            log_level: crate::diag::level_label().to_ascii_lowercase(),
            log_level_pinned: crate::diag::env_pinned(),
            dsh_initialized: !crate::profiles::scan_profiles().is_empty(),
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
pub async fn save_settings(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    settings: Settings,
) -> Result<(), String> {
    let mut settings = settings;
    // 白名单归一：脏值不写进 settings.json（否则非法值会永久躺在配置里，日志变更记录也跟着失真）
    if !matches!(settings.log_level.as_str(), "debug" | "info" | "warn" | "error") {
        settings.log_level = "info".into();
    }
    if !matches!(settings.web_open_mode.as_str(), "window" | "browser") {
        settings.web_open_mode = "window".into();
    }
    // 按 profile 的覆盖表同样收敛：脏值直接丢弃（等价于该 profile 未覆盖）
    settings
        .profile_launch_mode
        .retain(|_, v| matches!(v.as_str(), "child" | "detached"));
    settings
        .profile_web_open_mode
        .retain(|_, v| matches!(v.as_str(), "window" | "browser"));
    // 有实例运行时禁止切换当前版本：所有 profile 实例都基于 active 版本启动。
    // 这个守卫要枚举系统进程（Windows 上要起 PowerShell，1s 级），必须离开主线程 ——
    // 同步命令跑在主线程上，会把界面冻住（与 GitHub issue #1 同一类问题）。
    let current = state.settings.lock().unwrap().clone();
    if current.active_version != settings.active_version {
        crate::diag::info(
            "install",
            &format!(
                "切换当前版本：{:?} → {:?}",
                current.active_version, settings.active_version
            ),
        );
    }
    if !current.active_version.is_empty() && current.active_version != settings.active_version {
        let proc_state = procs.inner().clone();
        tauri::async_runtime::spawn_blocking(move || ensure_no_running_instance(&proc_state))
            .await
            .map_err(|e| format!("保存设置失败: {e}"))??;
    }
    settings::save_settings(&settings)?;
    // 日志级别：先落盘再立即应用（DSH_STARTER_LOG 环境变量存在时被其覆盖）
    if current.log_level != settings.log_level {
        let applied = crate::diag::set_runtime_level(&settings.log_level);
        crate::diag::info(
            "app",
            &format!(
                "日志级别 {} → {}{}",
                current.log_level,
                settings.log_level,
                if applied {
                    "（立即生效）"
                } else {
                    "（被 DSH_STARTER_LOG 环境变量覆盖，实际级别未变）"
                }
            ),
        );
    }
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
    let started = std::time::Instant::now();
    let out = registry::fetch_registry(&registry_base).await;
    match &out {
        Ok(info) => crate::diag::info(
            "network",
            &format!(
                "拉取版本列表成功：{} 个版本，耗时 {}ms，registry={}",
                info.versions.len(),
                started.elapsed().as_millis(),
                crate::registry::scrub_url(&registry_base)
            ),
        ),
        Err(e) => crate::diag::error(
            "network",
            &format!(
                "拉取版本列表失败：registry={} 耗时 {}ms\n  错误: {e}",
                crate::registry::scrub_url(&registry_base),
                started.elapsed().as_millis()
            ),
        ),
    }
    out
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
pub async fn install_version(
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
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    let install_state = install_state.inner().clone();
    // 安装会清空该版本的目录，正在运行时一律拒绝；force 只是前端区分"重装"的标记。
    // 这里的守卫要枚举系统进程（Windows 上 1s 级），放后台线程执行。
    tauri::async_runtime::spawn_blocking(move || {
        for p in crate::procs::list(&proc_state) {
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
        installer_start(&app, &install_state, &settings, &version)?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("安装启动失败: {e}"))?
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
pub async fn uninstall_version(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: String,
) -> Result<(), String> {
    let active_version = state.settings.lock().unwrap().active_version.clone();
    let proc_state = procs.inner().clone();
    // 外部实例检查要枚举系统进程（Windows 上 1s 级）：放后台线程，别冻住界面
    tauri::async_runtime::spawn_blocking(move || {
        // 该版本正在内嵌运行时不允许删除其安装目录
        for p in crate::procs::list(&proc_state) {
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
        if active_version == version {
            return Err(format!(
                "{version} 是当前使用版本，请先在侧栏切换到其他版本后再卸载"
            ));
        }
        let out = crate::installer::uninstall_managed(&version);
        match &out {
            Ok(()) => crate::diag::info("install", &format!("卸载 dsh {version} 完成")),
            Err(e) => crate::diag::error("install", &format!("卸载 dsh {version} 失败：{e}")),
        }
        out
    })
    .await
    .map_err(|e| format!("卸载失败: {e}"))?
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
        // 旧版兜底：目标 dsh < 0.1.7 时它只认全局 settings.yaml，若已被 0.1.7 导入
        // 改名则先还原一份（幂等，绝不改 .imported；诊断日志由 restore 内部记录）。
        if !crate::profile_cfg::uses_patch_config(&target.version) {
            crate::profile_cfg::restore_legacy_settings();
        }
        Ok(starter::launch(
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

/// 版本变化风险提示：启动闸门把一次启动拦下来时带回给前端，由前端弹确认框。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionChange {
    pub profile: String,
    /// 该 profile 上次成功启动用的 dsh 版本
    pub from_version: String,
    /// 本次将要使用的版本
    pub to_version: String,
    /// `upgrade` | `downgrade`（按 semver 比较，预发布版本也能判方向）
    pub direction: &'static str,
}

/// `start_embedded` 的返回：要么真的拉起了实例，要么被版本变化闸门拦下等用户确认。
/// 两个字段都可能为 null，前端按 `versionChange` 是否非空分支。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub proc: Option<crate::procs::ProcInfo>,
    pub version_change: Option<VersionChange>,
    /// 旧版（<0.1.7）启动前从 settings.yaml.imported 还原了全局配置时带回其路径，
    /// 前端 toast 明示；未发生还原为 null
    pub legacy_restored: Option<String>,
}

fn version_change(profile: &str, from: &str, to: &str) -> VersionChange {
    VersionChange {
        profile: profile.to_string(),
        from_version: from.to_string(),
        to_version: to.to_string(),
        direction: match crate::semver::compare(to, from) {
            std::cmp::Ordering::Less => "downgrade",
            _ => "upgrade",
        },
    }
}

/// 启动 dsh：detached=false 子进程（日志回传界面，启动器退出即结束）；
/// detached=true 独立进程（自成进程组、日志写文件，启动器退出后继续运行，重启后由扫描识别）。
/// 缺省跟随设置里的 launchMode。
/// `ack_version_change=true` = 用户已在确认框里认了这个版本变化，放行。
#[tauri::command]
pub async fn start_embedded(
    app: AppHandle,
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    version: Option<String>,
    profile: Option<String>,
    args: Option<String>,
    detached: Option<bool>,
    ack_version_change: Option<bool>,
) -> Result<StartResult, String> {
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    start_instance(
        app,
        settings,
        proc_state,
        version,
        profile,
        args,
        detached,
        ack_version_change.unwrap_or(false),
    )
    .await
}

/// 真正拉起一个实例（内嵌 keeper 或独立进程）。`start_embedded` 与 `init_dsh` 共用。
/// `ack_version_change=false` 时，若该 profile 上次是用另一个 dsh 版本跑起来的，
/// 不 spawn，只把风险提示带回去让用户确认。
async fn start_instance(
    app: AppHandle,
    settings: Settings,
    proc_state: crate::procs::ProcState,
    version: Option<String>,
    profile: Option<String>,
    args: Option<String>,
    detached: Option<bool>,
    ack_version_change: bool,
) -> Result<StartResult, String> {
    let detached = detached.unwrap_or(settings.launch_mode == "detached");

    enum Prep {
        /// 第 4 个字段：用户确认过版本变化时的新版本 —— spawn 成功后才写绑定表
        Go(
            crate::installed::InstalledVersion,
            String,
            String,
            Option<String>,
        ),
        NeedConfirm(VersionChange),
    }

    // 1) 解析目标版本 + profile 唯一性守卫 + 版本变化闸门（线程池执行）
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
            // 版本变化闸门：换 dsh 版本可能让这个 profile 起不来（插件不兼容、
            // 配置格式变了），降级尤其危险。首次启动没有历史版本，不打扰。
            // 用 semver 比较而非字符串相等：「1.0」与「1.0.0」功能同版，不该弹确认。
            let change = crate::profile_versions::bound(&profile)
                .filter(|prev| {
                    crate::semver::compare(prev, &target.version) != std::cmp::Ordering::Equal
                })
                .map(|prev| version_change(&profile, &prev, &target.version));
            match change {
                None => Ok(Prep::Go(target, profile, launch_args, None)),
                Some(c) if !ack_version_change => {
                    crate::diag::info(
                        "instance",
                        &format!(
                            "版本变化已拦下等确认：profile={:?} dsh {} → {}（{}）",
                            c.profile, c.from_version, c.to_version, c.direction
                        ),
                    );
                    Ok(Prep::NeedConfirm(c))
                }
                Some(c) => {
                    // 用户已经认了这个风险：等 spawn 成功再写绑定（见下方两条启动路径）。
                    // 立刻写的话，端口被占等原因起不来时绑定已被改成新版本，
                    // 下次启动不再提醒——正是这道闸门要拦的场景被静默吞掉；
                    // 而 record_running 的 15s grace 对已退出的进程也没机会纠偏。
                    crate::diag::info(
                        "instance",
                        &format!(
                            "版本变化已确认放行：profile={:?} dsh {} → {}（{}）",
                            c.profile, c.from_version, c.to_version, c.direction
                        ),
                    );
                    Ok(Prep::Go(target, profile, launch_args, Some(c.to_version)))
                }
            }
        }
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))??;

    let (target, profile, launch_args, confirmed_version) = match prep {
        Prep::NeedConfirm(change) => {
            return Ok(StartResult {
                proc: None,
                version_change: Some(change),
                legacy_restored: None,
            })
        }
        Prep::Go(target, profile, launch_args, confirmed_version) => {
            (target, profile, launch_args, confirmed_version)
        }
    };
    let prep = (target, profile, launch_args);
    let profile_name = prep.1.clone();

    // 旧版兜底（spawn 前）：本次要跑的 dsh < 0.1.7，而它只认全局 settings.yaml。
    // 若那份文件已被 0.1.7 导入改名（只剩 settings.yaml.imported），先复制还原一份，
    // 否则旧版读不到模型/凭据配置。幂等：settings.yaml 已存在则 no-op，绝不改 .imported。
    let version_for_restore = prep.0.version.clone();
    let legacy_restored = tauri::async_runtime::spawn_blocking(move || {
        if crate::profile_cfg::uses_patch_config(&version_for_restore) {
            None
        } else {
            crate::profile_cfg::restore_legacy_settings()
                .map(|p| p.to_string_lossy().into_owned())
        }
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))?;

    // 2) 独立进程：直接 spawn（无 keeper、无日志管道），实例由 /proc 扫描感知
    if detached {
        let rec = confirmed_version.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let info = crate::procs::spawn_detached(&settings, &prep.0, &prep.1, &prep.2)?;
            if let Some(v) = rec {
                crate::profile_versions::record(&prep.1, &v);
            }
            Ok(started(info, legacy_restored))
        })
        .await
        .map_err(|e| format!("启动失败: {e}"))?;
    }

    // 3) keeper 线程 spawn dsh 并守候退出（线程存活期 == 子进程存活期，
    //    PDEATHSIG 绑定的是该线程，不能放进约 10s 就回收的 tokio 线程池）
    let rx = crate::procs::spawn_keeper(app, proc_state, settings, prep.0, prep.1, prep.2);

    // 4) 等待 spawn 结果
    let info = tauri::async_runtime::spawn_blocking(move || {
        rx.recv().map_err(|e| format!("dsh keeper 线程异常: {e}"))?
    })
    .await
    .map_err(|e| format!("启动失败: {e}"))??;
    // 用户确认过版本变化且确实 spawn 成功：现在才写绑定表
    if let Some(v) = confirmed_version {
        let p = profile_name;
        tauri::async_runtime::spawn_blocking(move || crate::profile_versions::record(&p, &v));
    }
    Ok(started(info, legacy_restored))
}

fn started(info: crate::procs::ProcInfo, legacy_restored: Option<String>) -> StartResult {
    StartResult {
        proc: Some(info),
        version_change: None,
        legacy_restored,
    }
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
pub async fn stop_process(
    app: AppHandle,
    procs: State<'_, crate::procs::ProcState>,
    id: u32,
) -> Result<bool, String> {
    let procs = procs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if crate::procs::stop(&app, &procs, id) {
            return Ok(true);
        }
        crate::procs::stop_external_pid(id)
    })
    .await
    .map_err(|e| format!("停止失败: {e}"))?
}

#[tauri::command]
pub fn list_processes(
    procs: State<'_, crate::procs::ProcState>,
) -> Result<Vec<crate::procs::ProcInfo>, String> {
    Ok(crate::procs::list(&procs))
}

/// 各 profile 实例状态（含终端/外部启动的）
#[tauri::command]
pub async fn list_profile_instances(
    procs: State<'_, crate::procs::ProcState>,
) -> Result<Vec<crate::procs::ProfileInstance>, String> {
    // profile_instances 会扫盘 / 逐个校验 detached 存活（ps/PowerShell）/ 真实 TCP
    // 端口探测，Windows 上可达 1 秒；且被前端每 3 秒轮询，必须离开主线程执行。
    let procs = procs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let list = crate::procs::profile_instances(&procs);
        // 顺手把「这个版本确实把该 profile 跑起来了」写进绑定表（只在真有变化时落盘）。
        // 放在这里而不是 profile_instances 内部：那个函数保持纯枚举、不带磁盘副作用，
        // 落盘统一由命令层在 spawn_blocking 里做。
        crate::profile_versions::record_running(&list, crate::profile_versions::RUN_GRACE_MS);
        Ok(list)
    })
    .await
    .map_err(|e| format!("枚举实例失败: {e}"))?
}

/// 各 profile 的绑定启动版本（上次真正跑起来用的 dsh 版本），供界面显示「上次 dsh x.y.z」
#[tauri::command]
pub async fn list_profile_versions() -> Result<Vec<crate::profile_versions::ProfileVersionInfo>, String> {
    tauri::async_runtime::spawn_blocking(|| Ok(crate::profile_versions::list()))
        .await
        .map_err(|e| format!("读取 profile 绑定版本失败: {e}"))?
}

/// dsh 会话统计：扫描 $DSH_HOME/sessions 的会话日志，按 dsh-token-stats 口径聚合
/// （token 总量/趋势/热力图/模型分布/今日 + 在线时长三口径）。首扫全量、之后按文件指纹增量。
#[tauri::command]
pub async fn get_session_stats(
    range_days: Option<u32>,
    gap_min: Option<u32>,
) -> Result<crate::sessions::SessionStats, String> {
    let range = range_days.unwrap_or(30).clamp(1, 3650);
    let gap = gap_min.unwrap_or(crate::sessions::DEFAULT_GAP_MIN);
    tauri::async_runtime::spawn_blocking(move || crate::sessions::session_stats(range, gap))
        .await
        .map_err(|e| format!("统计扫描失败: {e}"))?
}

/// 清空统计缓存（~/.dsh-starter/session-stats-cache.json + 内存指纹表）。
/// 只影响下次扫描的快慢：清空后统计页会基于会话日志全量重算，会话数据本身不动。
#[tauri::command]
pub async fn clear_session_stats_cache() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(crate::sessions::clear_stats_cache)
        .await
        .map_err(|e| format!("清空统计缓存失败: {e}"))
}

/// 分享面板署名：git 全局身份（user.name / user.email），前端导出 PNG 用
#[tauri::command]
pub async fn get_share_identity() -> Result<crate::share::ShareIdentity, String> {
    Ok(tauri::async_runtime::spawn_blocking(crate::share::share_identity)
        .await
        .map_err(|e| format!("读取 git 身份失败: {e}"))?)
}

/// 停止某个 profile 的实例（内嵌或外部）
#[tauri::command]
pub async fn stop_profile_instance(    app: AppHandle,
    procs: State<'_, crate::procs::ProcState>,
    profile: String,
) -> Result<bool, String> {
    let procs = procs.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::procs::stop_profile(&app, &procs, &profile))
        .await
        .map_err(|e| format!("停止失败: {e}"))?
}

/// 首次初始化 dsh：跑一次 `dsh web`（等价 `--profile web`）。
///
/// 为什么需要单独一个入口：dsh 的 `$DSH_HOME`（默认 `~/.dsh`）是**第一次运行 dsh 时**
/// 才生成的（已实测：全新 DSH_HOME 下 `dsh web` 会写出 profiles/web、storages、
/// .credentials.yaml 等）。在那之前 profile 列表为空 —— 启动器里基于 profile 的一切
/// （启动实例、快捷配置、插件管理）都无从下手，用户会以为「装了但用不了」。
#[tauri::command]
pub async fn init_dsh(
    app: AppHandle,
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
) -> Result<crate::procs::ProcInfo, String> {
    let settings = state.settings.lock().unwrap().clone();
    let proc_state = procs.inner().clone();
    // 全新机器最常见的卡点是「压根没装 dsh 版本」：先拦住并给出下一步，
    // 而不是让 resolve_target 抛一句「未找到已安装的 …」
    let s2 = settings.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        crate::installed::collect_installed(&s2)
    })
    .await
    .map_err(|e| format!("初始化失败: {e}"))?;
    if installed.is_empty() {
        return Err(
            "还没有安装 dsh 版本：先到「版本」页安装一个（可点「安装最新版」），再回来做首次初始化"
                .into(),
        );
    }
    crate::diag::info(
        "app",
        &format!(
            "首次初始化：以 `dsh web` 启动内置 web profile；DSH_HOME={} profiles={}",
            crate::profiles::dsh_native_home().display(),
            crate::profiles::profiles_dir().display()
        ),
    );
    // profile 固定 "web"：这正是 `dsh web` 的等价形式（dsh 自己的 help 写明
    // `web` 是 `--profile web` 的别名），且无需 profile 目录已存在。
    // ack_version_change 传 true：首次初始化没有任何绑定记录可比，
    // 拿一句「请先确认升级风险」拦住初始化向导只会让人莫名其妙。
    let out = start_instance(
        app,
        settings,
        proc_state,
        None,
        Some("web".into()),
        None,
        None,
        true,
    )
    .await
    .map_err(|e| {
        crate::diag::error("app", &format!("首次初始化启动失败：{e}"));
        format!("初始化失败：{e}")
    })?;
    let info = out
        .proc
        .ok_or_else(|| "初始化失败：dsh 实例未拉起，请重试或到「版本」页检查已安装版本".to_string())?;
    crate::diag::info(
        "app",
        &format!(
            "首次初始化已拉起：pid={} profile={} —— dsh 正在写出 $DSH_HOME（profiles/web、storages、凭据文件）",
            info.id, info.profile
        ),
    );
    Ok(info)
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
pub async fn read_instance_log(
    pid: u32,
    max_bytes: Option<u32>,
) -> Result<Option<InstanceLog>, String> {
    // 读注册表 + 读日志文件（最多 64KB）都是阻塞 I/O，放后台线程执行
    tauri::async_runtime::spawn_blocking(move || {
        let max = max_bytes.unwrap_or(64 * 1024) as usize;
        Ok(crate::procs::read_instance_log_tail(pid, max)?.map(|(path, content, truncated)| {
            InstanceLog {
                path,
                content,
                truncated,
            }
        }))
    })
    .await
    .map_err(|e| format!("读取实例日志失败: {e}"))?
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
pub async fn rename_profile(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    name: String,
    new_name: String,
) -> Result<String, String> {
    let old = name.trim().to_string();
    let new = new_name.trim().to_string();
    let proc_state = procs.inner().clone();
    // ensure_profile_idle 会枚举实例（含系统进程），放后台线程执行
    let (old2, new2) = (old.clone(), new.clone());
    tauri::async_runtime::spawn_blocking(move || {
        ensure_profile_idle(&proc_state, &old2)?;
        crate::profile_cfg::rename_profile(&old2, &new2)?;
        // 独立进程登记表里也记着 profile 名，不同步会在实例列表里留下旧名的幽灵条目
        crate::procs::rename_detached_profile(&old2, &new2);
        // 绑定版本同理：不搬就走旧名那条记录，新名被当成首次启动（少一次风险提示）
        crate::profile_versions::rename(&old2, &new2);
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("改名失败: {e}"))??;
    crate::diag::info("profile", &format!("重命名 profile：{old:?} → {new:?}"));
    let mut s = state.settings.lock().unwrap();
    if s.default_profile == old {
        s.default_profile = new.clone();
        let _ = settings::save_settings(&s);
    }
    Ok(new)
}

/// 删除 profile：移入 ~/.dsh-starter/deleted-profiles/ 可找回；
/// dsh 内置保留 profile 由后端拒绝。
#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    procs: State<'_, crate::procs::ProcState>,
    name: String,
) -> Result<String, String> {
    let name = name.trim().to_string();
    let proc_state = procs.inner().clone();
    let name2 = name.clone();
    // 同上：实例枚举（可能起 PowerShell）必须在后台线程
    let trash = tauri::async_runtime::spawn_blocking(move || {
        ensure_profile_idle(&proc_state, &name2)?;
        let trash = crate::profile_cfg::delete_profile(&name2)?;
        crate::procs::drop_detached_profile(&name2);
        crate::profile_versions::drop(&name2);
        Ok::<String, String>(trash)
    })
    .await
    .map_err(|e| format!("删除失败: {e}"))??;
    crate::diag::info(
        "profile",
        &format!("删除 profile：{name:?} → 回收站 {trash}（可还原）"),
    );
    let mut s = state.settings.lock().unwrap();
    if s.default_profile == name {
        s.default_profile = String::new();
        let _ = settings::save_settings(&s);
    }
    Ok(trash)
}

/// 回收站列表（删除的 profile 只是被移入这里，可还原或彻底删除）
#[tauri::command]
pub async fn list_deleted_profiles() -> Vec<crate::profile_cfg::DeletedProfile> {
    match tauri::async_runtime::spawn_blocking(crate::profile_cfg::list_deleted_profiles).await {
        Ok(v) => v,
        Err(e) => {
            // 同步命令在主线程上扫回收站会卡窗口，挪进阻塞线程池后 join 失败
            // 只可能是线程池异常——留一条线索，别静默给出「回收站是空的」
            crate::diag::warn("app", &format!("读取回收站失败: {e}"));
            Vec::new()
        }
    }
}

/// 把回收站条目还原回 profiles 目录
#[tauri::command]
pub async fn restore_deleted_profile(dir_name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || crate::profile_cfg::restore_deleted_profile(&dir_name))
        .await
        .map_err(|e| format!("还原失败: {e}"))?
}

/// 从回收站彻底删除（不可恢复）
#[tauri::command]
pub async fn purge_deleted_profile(dir_name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::profile_cfg::purge_deleted_profile(&dir_name))
        .await
        .map_err(|e| format!("删除失败: {e}"))?
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

/// 把运行日志导出到 ~/.dsh-starter/logs/
#[tauri::command]
pub fn export_proc_log(
    profile: String,
    pid: u32,
    content: String,
) -> Result<String, String> {
    let dir = settings::starter_home().join("logs");
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
        // `link:~/my-plugin` 这种本地路径要展开 `~`：命令是直接交给 pnpm 的，
        // 中途没有 shell 会替用户展开（Windows 上连 cmd 都不认 `~`）
        .map(|s| match s.strip_prefix("link:") {
            Some(path) if path.trim().starts_with('~') => {
                format!("link:{}", util::expand_tilde(path).display())
            }
            _ => s,
        })
        .collect();
    if raw_specs.is_empty() {
        return Err("安装规格为空".into());
    }
    // 打包产物直链只允许 https：规格是原样交给 dsh/pnpm 的，pnpm 对明文 http
    // 的压缩包照单全收，而它在传输途中可以被中间人换成任何内容（没有完整性校验）。
    // 这是安装入口，必须在这里挡住，不能只指望上游解析不认它。
    if let Some(bad) = raw_specs.iter().find(|s| crate::registry::is_plain_http_tarball_spec(s)) {
        return Err(format!(
            "打包产物直链必须使用 https（当前为明文 http）：{bad}\n\
             明文下载的安装包可能被中途替换且无从察觉，请把链接换成 https 后重试。"
        ));
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
    // 输入里带了代理前缀就先剥掉：代理只用于本次加速，写进 origin 会在代理失效后
    // 让这个克隆永久拉不动（用户手抄带前缀的地址后正是这样卡住的）
    let url = crate::ghaccel::strip_proxy_prefix(&url).unwrap_or(url);
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
        // 老克隆的 origin 可能是代理地址（手抄的 URL / 旧版本留下的）：先修回来 ——
        // 否则本次加速对它无效（insteadOf 只认 https://github.com/），代理一挂就永久拉不动
        let _ = crate::plugin::fix_proxied_remote(&root_clone);
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

/// 把某个插件任务的完整日志导出到 ~/.dsh-starter/logs/，返回文件路径
#[tauri::command]
pub fn export_plugin_job_log(
    jobs: State<'_, crate::plugin::PluginJobState>,
    job_id: u64,
) -> Result<String, String> {
    let text = jobs
        .job_log_text(job_id)
        .ok_or_else(|| format!("任务 {job_id} 不存在（可能已被清理）"))?;
    let dir = settings::starter_home().join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("dsh-plugin-job-{job_id}-{ts}.log"));
    std::fs::write(&path, text).map_err(|e| format!("写日志失败: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// 列出 ~/.dsh-starter/git-plugins 下的克隆仓库（git 状态 + 插件候选）
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
pub async fn delete_cloned_plugin(dir_name: String) -> Result<(), String> {
    // 克隆目录连着 node_modules 可能有几十万个小文件，递归删除动辄数秒
    tauri::async_runtime::spawn_blocking(move || crate::plugin::remove_clone_dir(&dir_name))
        .await
        .map_err(|e| format!("删除失败: {e}"))?
}

/// 返回 git-plugins 目录路径（不存在则创建），供前端在文件管理器里打开
#[tauri::command]
pub fn reveal_git_plugins_dir() -> Result<String, String> {
    let dir = crate::plugin::git_plugins_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// 生成诊断包：环境摘要 + 设置（脱敏）+ 各分类日志尾部，写成一个 txt 并返回路径。
/// 用户报问题时发这一个文件即可，不用逐个追问环境细节。
/// 诊断包导出结果：落盘路径 + 全文（前端弹窗直接展示内容，不必再去开文件夹）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExport {
    pub path: String,
    pub content: String,
}

#[tauri::command]
pub async fn export_diagnostics(state: State<'_, AppState>) -> Result<DiagnosticsExport, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let body = crate::diag::diagnostics(env!("CARGO_PKG_VERSION"), &settings);
        let dir = crate::diag::logs_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
        let name = format!("diagnostics-{}.txt", crate::diag::run_id());
        let path = dir.join(&name);
        std::fs::write(&path, &body).map_err(|e| format!("写诊断包失败: {e}"))?;
        crate::diag::info("app", &format!("已生成诊断包：{}", path.display()));
        Ok(DiagnosticsExport {
            path: path.to_string_lossy().into_owned(),
            content: body,
        })
    })
    .await
    .map_err(|e| format!("生成诊断包失败: {e}"))?
}

/// 前端报错回流到 logs/ui.log：UI 上的异常（渲染报错、未处理的 Promise、
/// 报错 toast）此前只停在用户屏幕上，事后完全无法复盘。
#[tauri::command]
pub fn log_ui(level: String, message: String) {
    let lv = match level.as_str() {
        "warn" => crate::diag::Level::Warn,
        "error" => crate::diag::Level::Error,
        _ => crate::diag::Level::Info,
    };
    crate::diag::log("ui", lv, &message);
}

/// 「系统日志」页：单个分类的文件元数据（不存在也列出，size=0）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogCategory {
    pub category: String,
    pub description: String,
    pub size: u64,
    pub rotated_size: Option<u64>,
    pub modified_ms: Option<u64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogsInfo {
    pub logs_dir: String,
    /// 当前生效级别（小写 debug/info/warn/error）
    pub log_level: String,
    /// 级别是否被 DSH_STARTER_LOG 环境变量锁定（锁定时应用内改级别不生效）
    pub log_level_pinned: bool,
    pub categories: Vec<SystemLogCategory>,
}

#[tauri::command]
pub async fn list_system_logs() -> Result<SystemLogsInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        Ok(SystemLogsInfo {
            logs_dir: crate::diag::logs_dir().to_string_lossy().into_owned(),
            log_level: crate::diag::level_label().to_ascii_lowercase(),
            log_level_pinned: crate::diag::env_pinned(),
            categories: crate::diag::list_log_files()
                .into_iter()
                .map(|f| SystemLogCategory {
                    category: f.category.into(),
                    description: f.description.into(),
                    size: f.size,
                    rotated_size: f.rotated_size,
                    modified_ms: f.modified_ms,
                })
                .collect(),
        })
    })
    .await
    .map_err(|e| format!("读取日志清单失败: {e}"))?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLogContent {
    pub category: String,
    pub content: String,
    /// 尾部截断：更早的内容没显示（文件比 maxBytes 大）
    pub truncated: bool,
}

/// 读某个分类日志的尾部（滚动出的 `.1` + 当前文件连续读，能跨滚动看到更早历史）。
#[tauri::command]
pub async fn read_system_log(
    category: String,
    max_bytes: Option<u32>,
) -> Result<SystemLogContent, String> {
    // 只接受白名单里的分类名，用户输入不拼进路径
    if !crate::diag::CATEGORIES.iter().any(|(name, _)| *name == category) {
        return Err(unknown_log_cat(&category));
    }
    let max = max_bytes.unwrap_or(256 * 1024).clamp(1024, 4 * 1024 * 1024) as u64;
    tauri::async_runtime::spawn_blocking(move || {
        let (content, truncated) = crate::diag::read_cat_tail(&category, max);
        Ok(SystemLogContent {
            category,
            content,
            truncated,
        })
    })
    .await
    .map_err(|e| format!("读取日志失败: {e}"))?
}

/// 分类名白校验的统一报错：说清哪些类别可选（AGENTS：报错要能指导下一步）
fn unknown_log_cat(category: &str) -> String {
    format!(
        "未知日志类别「{category}」，可选：{}",
        crate::diag::CATEGORIES
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(" / ")
    )
}

/// 清除系统日志（「系统日志」页的清理按钮）。`categories` 为空 / 缺省 = 全部类别。
/// 返回释放的字节数；部分文件删不掉时错误里列明是哪一类。
#[tauri::command]
pub async fn clear_system_logs(categories: Option<Vec<String>>) -> Result<u64, String> {
    let cats: Vec<String> = match categories {
        Some(list) if !list.is_empty() => list,
        _ => crate::diag::CATEGORIES
            .iter()
            .map(|(n, _)| (*n).to_string())
            .collect(),
    };
    // 用户输入先过白名单，不拼路径
    if let Some(bad) = cats
        .iter()
        .find(|c| !crate::diag::CATEGORIES.iter().any(|(n, _)| *n == **c))
    {
        return Err(unknown_log_cat(bad));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut freed = 0u64;
        let mut errs: Vec<String> = Vec::new();
        for c in &cats {
            match crate::diag::clear_cat_log(c) {
                Ok(n) => freed += n,
                Err(e) => errs.push(format!("{c}：{e}")),
            }
        }
        // 先删再记：这样「清除过日志」这条记录不会被自己删掉
        crate::diag::info(
            "app",
            &format!(
                "清除 {} 个日志分类，释放 {} 字节{}",
                cats.len(),
                freed,
                if errs.is_empty() {
                    String::new()
                } else {
                    format!("，部分失败：{}", errs.join("；"))
                }
            ),
        );
        if errs.is_empty() {
            Ok(freed)
        } else {
            Err(format!(
                "以下分类的日志未能删除（通常是有程序正占用日志文件）：{}；其余分类已释放 {} 字节",
                errs.join("；"),
                freed
            ))
        }
    })
    .await
    .map_err(|e| format!("清除日志失败: {e}"))?
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

/// 复制 profile。复制后**补装依赖**（dsh 不会自动重装，缺 bundle 启动即报错；
/// 失败则回滚副本）并**自动错开 web 端口**（复用「邻近空闲端口」那套逻辑），
/// 否则两个实例配置同一个端口、无法并行启动。返回新端口（null = 该 profile 没有 webserver 配置）。
#[tauri::command]
pub async fn copy_profile(
    state: State<'_, AppState>,
    source: String,
    new_name: String,
) -> Result<Option<u16>, String> {
    let settings = state.settings.lock().unwrap().clone();
    // 整目录递归复制 + npm 装依赖 + 端口探测都在磁盘/子进程上耗时，
    // 留在同步命令里会卡住主线程
    tauri::async_runtime::spawn_blocking(move || {
        crate::profile_cfg::copy_profile_with_deps(&settings, &source, &new_name)
    })
    .await
    .map_err(|e| format!("复制 profile 失败: {e}"))?
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
    let probes = crate::registry::check_channels(token.as_deref()).await;
    crate::diag::info(
        "network",
        &format!(
            "渠道探测：{}",
            probes
                .iter()
                .map(|p| {
                    let mut d = format!("{}={}", p.name, if p.ok { "可用" } else { "不可用" });
                    if p.ms > 0 {
                        d.push_str(&format!("({}ms)", p.ms));
                    }
                    if let Some(detail) = &p.detail {
                        if !p.ok {
                            d.push_str(&format!("：{detail}"));
                        }
                    }
                    d
                })
                .collect::<Vec<_>>()
                .join("、")
        ),
    );
    Ok(probes)
}

/// 当前 GitHub API 额度（探测/更新检测已走免额度通道，这里只用于展示元数据额度）
#[tauri::command]
pub fn get_github_rate_limit() -> crate::registry::GitHubRateLimit {
    crate::registry::rate_limit()
}

/// 各 dsh 版本的发布说明（GitHub Releases，tag 前缀 `dsh-v`）。
/// 一次拉全量、按版本号倒序返回：更新日志对话框里要能随手翻前后几个版本，
/// 逐版本按需请求会把匿名额度（60/小时）几下用光。
/// 默认命中磁盘缓存就返回、不打 API；`force=true`（对话框「刷新」）才真正拉一次。
#[tauri::command]
pub async fn dsh_release_notes(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<Vec<crate::registry::DshRelease>, String> {
    let token = state.settings.lock().unwrap().github_token.clone();
    let token = crate::registry::github_token(Some(&token));
    crate::registry::fetch_dsh_releases(token.as_deref(), force.unwrap_or(false)).await
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

/// 只读查看被 0.1.7 导入存档的旧全局配置（settings.yaml.imported）；不存在时 Err
#[tauri::command]
pub fn read_imported_settings() -> Result<String, String> {
    let path = crate::profile_cfg::imported_settings_path();
    if !path.is_file() {
        return Err("~/.dsh/settings.yaml.imported 不存在：全局配置还没有被 0.1.7+ 导入过".into());
    }
    std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

/// profile 的配置归属判定（patch=0.1.7+ / legacy=旧版全局），驱动前端各 Tab 的读写路由
#[tauri::command]
pub async fn get_profile_config_mode(
    state: State<'_, AppState>,
    profile: String,
) -> Result<crate::profile_cfg::ProfileConfigMode, String> {
    let settings = state.settings.lock().unwrap().clone();
    let profile = profile.trim().to_string();
    if profile.is_empty() {
        return Err("未指定 profile".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        Ok(crate::profile_cfg::profile_config_mode(&settings, &profile))
    })
    .await
    .map_err(|e| format!("判定失败: {e}"))?
}

/// 读取模型配置。profile 为 None → 全局 settings.yaml（旧入口，兼容首页跳转）；
/// Some → 按该 profile 的归属路由：≥0.1.7 读 cordis.patch.yml，旧版读全局并标注 mode
#[tauri::command]
pub async fn get_model_config(
    state: State<'_, AppState>,
    profile: Option<String>,
) -> Result<crate::modelcfg::ModelConfig, String> {
    let profile = profile.filter(|p| !p.trim().is_empty()).map(|p| p.trim().to_string());
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(profile) = profile else {
            return crate::modelcfg::read();
        };
        let mode = crate::profile_cfg::profile_config_mode(&settings, &profile);
        crate::modelcfg::read_for_profile(&profile, &mode)
    })
    .await
    .map_err(|e| format!("读取失败: {e}"))?
}

/// 保存模型配置：路由同 `get_model_config`。patch 模式以 marker 块整块接管该 profile
/// cordis.patch.yml 的两个模型条目（其余条目逐字节保留，写前自动备份）
#[tauri::command]
pub async fn set_model_config(
    state: State<'_, AppState>,
    profile: Option<String>,
    config: crate::modelcfg::ModelConfigInput,
) -> Result<(), String> {
    let profile = profile.filter(|p| !p.trim().is_empty()).map(|p| p.trim().to_string());
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(profile) = profile else {
            return crate::modelcfg::write(&config);
        };
        let mode = crate::profile_cfg::profile_config_mode(&settings, &profile);
        crate::modelcfg::write_for_profile(&profile, &mode, &config)
    })
    .await
    .map_err(|e| format!("保存失败: {e}"))?
}

/// 把模型配置同步写入多个 profile（仅 ≥0.1.7 的目标有效）。
/// 返回失败清单（空 = 全部成功）；逐个目标写入，互不影响。
#[tauri::command]
pub async fn sync_model_config(
    state: State<'_, AppState>,
    targets: Vec<String>,
    config: crate::modelcfg::ModelConfigInput,
) -> Result<Vec<String>, String> {
    let settings = state.settings.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut failures: Vec<String> = Vec::new();
        for t in targets.iter() {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }
            let mode = crate::profile_cfg::profile_config_mode(&settings, t);
            if mode.mode != "patch" {
                let v = if mode.version.is_empty() {
                    "未知".to_string()
                } else {
                    mode.version.clone()
                };
                failures.push(format!(
                    "「{t}」绑定 dsh {v}（< 0.1.7），模型配置仍走全局 settings.yaml，已跳过"
                ));
                continue;
            }
            if let Err(e) = crate::modelcfg::write_for_profile(t, &mode, &config) {
                failures.push(format!("「{t}」写入失败: {e}"));
            }
        }
        Ok(failures)
    })
    .await
    .map_err(|e| format!("同步失败: {e}"))?
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
pub async fn get_credentials() -> Result<crate::credentials::CredentialFile, String> {
    tauri::async_runtime::spawn_blocking(crate::credentials::read)
        .await
        .map_err(|e| format!("凭据读取任务失败: {e}"))?
}

/// 整表保存凭据 refs（records / version 等其余顶层键原样保留，写前自动备份）。
/// `expected_fingerprint` 为页面读取时拿到的文件指纹，用于拒绝覆盖 dsh 等外部程序的修改。
#[tauri::command]
pub async fn write_credential_refs(
    refs: Vec<crate::credentials::CredentialRefInput>,
    expected_fingerprint: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::credentials::write_refs(&refs, expected_fingerprint.as_deref())
    })
    .await
    .map_err(|e| format!("凭据保存任务失败: {e}"))?
}

#[tauri::command]
pub async fn check_starter_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StarterUpdateStatus, String> {
    let settings = state.settings.lock().unwrap().clone();
    let status = update_check::check(&app, &settings).await;
    if status.mode == "error" {
        crate::diag::warn(
            "app",
            &format!(
                "启动器更新检查失败：{}",
                status.message.clone().unwrap_or_default()
            ),
        );
    } else {
        crate::diag::info(
            "app",
            &format!(
                "启动器更新检查：当前 {} 最新 {} 可用={} mode={}",
                status.current,
                status.latest.clone().unwrap_or_else(|| "（未知）".into()),
                status.available,
                status.mode
            ),
        );
    }
    Ok(status)
}

/// 下载并安装启动器新版本（内置 updater 模式）。
/// 非 Windows 平台上本调用不会返回：安装成功后直接重启应用。
#[tauri::command]
pub async fn install_starter_update(
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

/// 高版本 dsh 的 WebUI 可能写入低版本读不懂的 localStorage 值，降级后页面直接渲染失败。
/// 注入脚本先于页面自身脚本执行，在 dsh 读到坏值之前清掉 `dsh.*` 键。
const PURGE_DSH_STORAGE_JS: &str = r#"
try {
  const keys = [];
  for (let i = 0; i < localStorage.length; i++) {
    const k = localStorage.key(i);
    if (k && k.startsWith("dsh.")) keys.push(k);
  }
  for (const k of keys) localStorage.removeItem(k);
} catch (_) {}
"#;

/// dsh WebUI 是 SPA：HTML 加载完 ≠ 渲染完，中间有一段页面自身的白屏。
/// 注入同主题的加载遮罩（进度条 + 提示），轮询到 body 里出现真实内容
/// （遮罩之外的元素有文字或可交互控件）再淡出移除；10 秒兜底强制撤，
/// 保证导航失败时用户能看到实际错误而不是永远盖着加载页。
fn loading_overlay_js(dark: bool) -> String {
    let (bg, fg, track, bar) = if dark {
        ("#171717", "#a1a1aa", "#27272a", "#5b9dff")
    } else {
        ("#fbf7ef", "#78716c", "#e7dfd2", "#3b82f6")
    };
    format!(
        r#"
(function () {{
  if (window.__dshStarterLoading) return;
  window.__dshStarterLoading = true;
  var o = document.createElement('div');
  o.setAttribute('data-dsh-starter-loading', '1');
  o.style.cssText = 'position:fixed;inset:0;z-index:2147483647;display:flex;flex-direction:column;'
    + 'align-items:center;justify-content:center;gap:14px;background:{bg};color:{fg};'
    + "font:13px -apple-system,system-ui,sans-serif;transition:opacity .35s";
  o.innerHTML = '<div style="width:200px;height:3px;border-radius:9999px;overflow:hidden;background:{track}">'
    + '<div style="width:45%;height:100%;border-radius:9999px;background:{bar};animation:__dsl 1.15s ease-in-out infinite"></div></div>'
    + '<div>正在加载…</div>'
    + '<style>@keyframes __dsl{{0%{{transform:translateX(-110%)}}100%{{transform:translateX(330%)}}}}</style>';
  // 立即挂到 documentElement：不能等 DOMContentLoaded——dsh 前端 bundle 很大且是同步脚本，
  // DOMContentLoaded 要等它整个跑完，这期间页面白底已经先画出来了（用户看到的白屏就在这）。
  // document-start 时 <html> 已存在，fixed 定位相对视口，body 还没解析出来也照样盖住。
  function mount() {{
    var host = document.body || document.documentElement;
    if (host) host.appendChild(o);
    else setTimeout(mount, 10);
  }}
  mount();
  var t0 = Date.now(), done = false;
  function hide() {{
    if (done) return; done = true;
    o.style.opacity = '0';
    setTimeout(function () {{ o.remove(); }}, 400);
  }}
  function pageHasContent() {{
    if (!document.body) return false;
    var kids = document.body.children;
    for (var i = 0; i < kids.length; i++) {{
      var n = kids[i];
      if (n === o || n.hasAttribute('data-dsh-starter-loading')) continue;
      if ((n.textContent || '').trim().length > 0 || n.querySelector('canvas,svg,button,input,a')) return true;
    }}
    return false;
  }}
  var iv = setInterval(function () {{
    var ready = false;
    try {{ ready = pageHasContent(); }} catch (_) {{ ready = true; }}
    if (ready || Date.now() - t0 > 10000) {{ clearInterval(iv); hide(); }}
  }}, 150);
}})();
"#,
        bg = bg,
        fg = fg,
        track = track,
        bar = bar
    )
}

/// 在应用内为实例的 Web UI 开一个独立窗口（无浏览器地址栏，像桌面端一样）。
/// 同一地址复用同一窗口（已开则前置聚焦），多个实例可以同时各开一个窗口。
/// `multi=true` 时不复用：每次点击都新开一个窗口（同地址多个会话并行的场景，
/// 如 DeepSeek 官方对话），label 在基础名后追加最小未占用序号。
///
/// 窗口装饰完全交给系统（标题栏、拖动、缩放都是原生的），启动器不插手：
/// 自绘标题栏要么依赖各平台各自的窗口能力（Linux 的 Wayland 会话下直接不成立，
/// 会把标题栏甩成窗口中间的一块浮面），要么得改 dsh 页面自己的布局，代价都大于收益。
#[tauri::command]
pub fn open_web_window(
    app: AppHandle,
    url: String,
    title: Option<String>,
    theme: Option<String>,
    multi: Option<bool>,
) -> Result<(), String> {
    let parsed: tauri::Url = url
        .parse()
        .map_err(|_| format!("地址无效，无法打开：{url}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("只支持 http/https 地址：{url}"));
    }
    // label 由 scheme+host+port 派生：保证一个地址全局只有一个窗口。
    // 只留字母数字（其余含 IPv6 的 [] : 统统换成 -），否则 `[::1]` 会派生出
    // 含非法字符的 label 导致 build 失败；label 含 scheme 则避免 http/https
    // 同 host 端口共用一个窗口（复用分支只聚焦、不导航到第二个地址）。
    let host: String = parsed
        .host_str()
        .ok_or_else(|| format!("地址缺少主机名，无法打开：{url}"))?
        .chars()
        .map(|c| {
            let l = c.to_ascii_lowercase();
            if l.is_ascii_alphanumeric() {
                l
            } else {
                '-'
            }
        })
        .collect();
    let port = parsed.port().map(|p| format!("-{p}")).unwrap_or_default();
    let base_label = format!("dsh-web-{}-{host}{port}", parsed.scheme());
    let multi = multi.unwrap_or(false);
    let label = if multi {
        // 基础名空闲就用基础名（首个窗口与非 multi 形态一致），否则找最小未占用序号
        if app.get_webview_window(&base_label).is_some() {
            (2u32..)
                .map(|i| format!("{base_label}-{i}"))
                .find(|l| app.get_webview_window(l).is_none())
                .ok_or_else(|| "打开的窗口太多了，请先关闭一些再试".to_string())?
        } else {
            base_label
        }
    } else {
        base_label
    };
    let win_title = title.unwrap_or_else(|| "DSH Web".into());
    if !multi {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.unminimize();
            win.show().map_err(|e| format!("恢复窗口失败: {e}"))?;
            win.set_focus().map_err(|e| format!("聚焦窗口失败: {e}"))?;
            return Ok(());
        }
    }
    // 白屏刺眼的修复：外部页面要等网络/JS 就绪，先按主题铺好窗口底色，
    // 并以隐藏方式创建，首屏加载完成（或 5 秒兜底）再显示。
    let dark = theme.as_deref() != Some("light");
    let bg = if dark {
        tauri::utils::config::Color(23, 23, 23, 255)
    } else {
        tauri::utils::config::Color(251, 247, 239, 255)
    };
    let shown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shown_on_load = shown.clone();
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(parsed))
        .title(&win_title)
        .inner_size(1000.0, 720.0)
        .min_inner_size(480.0, 520.0)
        .resizable(true)
        .center()
        .theme(Some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }))
        .background_color(bg)
        .visible(false)
        .initialization_script(format!(
            "{PURGE_DSH_STORAGE_JS}{}",
            loading_overlay_js(dark)
        ))
        .on_page_load(move |win, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished
                && !shown_on_load.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                let _ = win.show();
                let _ = win.set_focus();
            }
        })
        .build()
        .map_err(|e| format!("打开窗口失败：{e}"))?;
    // 兜底：加载事件不来（实例已挂、导航失败等）也要把窗口亮出来，
    // 让用户至少能看到并读到错误，而不是「点了没窗口」。
    let shown_timeout = shown.clone();
    let app_p = app.clone();
    let label_p = label.clone();
    let _ = std::thread::Builder::new().name("dsh-web-show-fallback".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        if shown_timeout.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if let Some(win) = app_p.get_webview_window(&label_p) {
            let _ = win.show();
        }
    });
    Ok(())
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
        crate::diag::info(
            "app",
            &format!(
                "启动时自动检查更新：当前 {} 最新 {} 可用={} mode={}",
                status.current,
                status.latest.clone().unwrap_or_else(|| "（未知）".into()),
                status.available,
                status.mode
            ),
        );
        if !status.available {
            return;
        }
        if settings.auto_install_update && status.mode == "builtin" && !status.needs_elevation {
            let _ = app2.emit("starter-update", status);
            if let Err(e) = update_check::install_builtin(&app2, &settings).await {
                crate::diag::error("app", &format!("自动更新安装失败：{e}"));
                let _ = app2.emit("toast", format!("自动更新失败：{e}"));
            }
            return;
        }
        let _ = app2.emit("starter-update", status);
    });
}
