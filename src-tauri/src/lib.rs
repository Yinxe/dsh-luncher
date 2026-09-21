mod commands;
mod credentials;
mod diag;
mod ghaccel;
mod installed;
mod installer;
mod launcher;
mod modelcfg;
mod netports;
mod plugin;
mod pnpm;
mod profiles;
mod procs;
mod profile_cfg;
mod registry;
mod runtime;
mod semver;
mod settings;
mod tray;
mod update_check;
mod verify;
mod util;

use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 先把崩溃留痕挂上：release 构建是 windows_subsystem = "windows"，panic 默认无处可看
    diag::install_panic_hook();
    diag::mark("启动器启动");

    let settings = settings::load_settings();

    let app = tauri::Builder::default()
        // 单实例守卫：必须是第一个注册的插件，才能在其它插件 setup / 窗口创建之前
        // 就把「已经有一个实例在跑」拦下来。
        // 行为：重复启动的那个进程只把这次启动转交给已运行实例，然后自己退出，
        // 因此不会出现第二个主窗口 / 第二个托盘图标 / 两份后台轮询线程。
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 已运行实例收到回调：把主窗口唤回前台（可能因「关闭到托盘」而隐藏着）
            #[cfg(target_os = "macos")]
            let _ = app.show();
            crate::tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(settings::AppState {
            settings: std::sync::Mutex::new(settings),
        })
        .manage(installer::InstallState::default())
        .manage(procs::ProcState::default())
        .manage(plugin::PluginJobState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_environment,
            commands::get_settings,
            commands::save_settings,
            commands::list_remote_versions,
            commands::list_installed,
            commands::install_version,
            commands::cancel_install,
            commands::get_install_status,
            commands::uninstall_version,
            commands::launch_version,
            commands::start_embedded,
            commands::stop_process,
            commands::list_processes,
            commands::list_profile_instances,
            commands::stop_profile_instance,
            commands::export_proc_log,
            commands::read_instance_log,
            commands::list_profiles,
            commands::check_launcher_update,
            commands::install_launcher_update,
            commands::reveal_folder,
            commands::open_external,
            commands::install_runtime,
            commands::get_profile_detail,
            commands::get_patch_reload,
            commands::set_bundle_enabled,
            commands::plugin_install,
            commands::plugin_uninstall,
            commands::plugin_pull_update,
            commands::plugin_clone_install,
            commands::probe_clone_repo,
            commands::get_github_accel,
            commands::list_plugin_jobs,
            commands::cancel_plugin_job,
            commands::clear_plugin_jobs,
            commands::plugin_approve_builds,
            commands::plugin_retry_job,
            commands::export_plugin_job_log,
            commands::list_cloned_plugins,
            commands::probe_local_plugins,
            commands::delete_cloned_plugin,
            commands::reveal_git_plugins_dir,
            commands::read_profile_file,
            commands::write_profile_file,
            commands::get_web_quick_config,
            commands::set_web_quick_config,
            commands::copy_profile,
            commands::rename_profile,
            commands::delete_profile,
            commands::create_recovery_profile,
            commands::list_deleted_profiles,
            commands::restore_deleted_profile,
            commands::purge_deleted_profile,
            commands::search_registry_packages,
            commands::get_github_rate_limit,
            commands::check_channels,
            commands::check_plugin_updates,
            commands::read_global_config,
            commands::write_global_config,
            commands::get_model_config,
            commands::set_model_config,
            commands::fetch_provider_models,
            commands::get_credentials,
            commands::write_credential_refs,
        ])
        .setup(|app| {
            // 进程表快照的守护线程必须先起来：枚举（Windows 上要起 PowerShell，1s 级）
            // 只在后台线程做，托盘与界面都只读快照。在 setup（主线程）里等枚举，
            // 窗口就会「能出现但一直白屏 / 未响应」—— GitHub issue #1 的根因。
            procs::start_process_watcher();
            diag::mark("setup: 进程表守护线程已启动");
            tray::create(app.handle())?;
            diag::mark("setup: 托盘就绪");
            commands::emit_startup_checks(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window
                    .app_handle()
                    .state::<settings::AppState>()
                    .settings
                    .lock()
                    .unwrap()
                    .close_to_tray;
                if close_to_tray {
                    // 关闭按钮 → 隐藏到托盘，后台常驻
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building dsh launcher");

    app.run(|app, event| {
        // 启动器退出 → 结束所有内嵌 dsh 子进程
        if let tauri::RunEvent::Exit = event {
            let state = app.state::<procs::ProcState>();
            procs::stop_all(&app, &state);
        }
    });
}
