mod commands;
mod installed;
mod installer;
mod launcher;
mod profiles;
mod registry;
mod semver;
mod settings;
mod tray;
mod update_check;
mod util;

use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = settings::load_settings();

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(settings::AppState {
            settings: std::sync::Mutex::new(settings),
        })
        .manage(installer::InstallState::default())
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
            commands::list_profiles,
            commands::check_launcher_update,
            commands::reveal_folder,
            commands::open_external,
        ])
        .setup(|app| {
            tray::create(app.handle())?;
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
        .run(tauri::generate_context!())
        .expect("error while running dsh launcher");
}
