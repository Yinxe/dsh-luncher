use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};

const TRAY_ID: &str = "main-tray";

/// 上次菜单内容签名：内容没变不 set_menu，避免菜单无谓重建/闪动
#[derive(Default)]
pub struct MenuState(Mutex<Option<String>>);

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        match w.is_visible() {
            Ok(true) => hide_main_window(app),
            _ => show_main_window(app),
        }
    }
}

/// profile 在菜单里的展示名（未指定 profile 的实例）
fn display_name(profile: &str) -> &str {
    if profile.is_empty() {
        "（无 profile）"
    } else {
        profile
    }
}

/// 按当前 profile 运行状态组装菜单，返回菜单与内容签名。
/// profile 段为纯状态展示（不可点击）：● 运行中 / ○ 未运行。
fn build_menu(app: &AppHandle) -> tauri::Result<(Menu<tauri::Wry>, String)> {
    let procs = app.state::<crate::procs::ProcState>();
    let instances = crate::procs::profile_instances(procs.inner());
    let known = crate::profiles::scan_profiles();

    let mut items: Vec<Box<dyn IsMenuItem<tauri::Wry>>> = Vec::new();
    items.push(Box::new(MenuItem::with_id(
        app,
        "open",
        "打开主界面",
        true,
        None::<&str>,
    )?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));

    let mut sig = String::new();
    let mut listed: Vec<String> = Vec::new();
    for p in &known {
        let running = instances.iter().any(|i| i.profile == p.name);
        sig.push_str(&format!("{}={};", p.name, running as u8));
        items.push(Box::new(MenuItem::with_id(
            app,
            format!("prof:{}", p.name),
            if running {
                format!("● {} · 运行中", p.name)
            } else {
                format!("○ {} · 未运行", p.name)
            },
            false,
            None::<&str>,
        )?));
        listed.push(p.name.clone());
    }
    // 运行中但磁盘上已不存在的 profile（如配置目录启动后被删）
    for i in &instances {
        if listed.contains(&i.profile) {
            continue;
        }
        sig.push_str(&format!("{}=1;", i.profile));
        items.push(Box::new(MenuItem::with_id(
            app,
            format!("prof:{}", i.profile),
            format!("● {} · 运行中", display_name(&i.profile)),
            false,
            None::<&str>,
        )?));
    }
    if items.len() == 2 {
        sig.push('-');
        items.push(Box::new(MenuItem::with_id(
            app,
            "no-profile",
            "未发现 dsh profile",
            false,
            None::<&str>,
        )?));
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "quit",
        "退出",
        true,
        None::<&str>,
    )?));

    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items.iter().map(|i| i.as_ref()).collect();
    let menu = Menu::with_items(app, &refs)?;
    Ok((menu, sig))
}

/// 按当前状态刷新托盘菜单（内容变化才真正 set_menu）
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let Ok((menu, sig)) = build_menu(app) else { return };
    let state = app.state::<MenuState>();
    {
        let mut last = state.0.lock().unwrap();
        if last.as_deref() == Some(sig.as_str()) {
            return;
        }
        *last = Some(sig);
    }
    let _ = tray.set_menu(Some(menu));
}

/// 把菜单刷新挪到独立线程再执行。
/// 事件监听器是**同步**回调（Tauri 在 emit 的线程上直接调用），而 `refresh`
/// 会枚举系统进程（Windows 走 PowerShell、约 1s）并重建菜单；命令线程（主线程）
/// 上 emit 时会直接把界面卡住。同理，emit 方也绝不能在持锁状态下回调进来。
fn refresh_async(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("tray-refresh-event".into())
        .spawn(move || refresh(&app));
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    app.manage(MenuState::default());

    let menu = build_menu(app)?.0;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("DSH Launcher")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => toggle_main_window(tray.app_handle()),
            TrayIconEvent::Click {
                button: MouseButton::Right,
                ..
            } => refresh(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    refresh(app);

    // 内嵌进程启停即时刷新
    let app_exit = app.clone();
    app.listen("proc-exit", move |_| refresh_async(&app_exit));
    let app_start = app.clone();
    app.listen("proc-started", move |_| refresh_async(&app_start));

    // 终端等外部启动的 dsh 没有事件可订阅，定时轮询兜底
    let app_poll = app.clone();
    std::thread::Builder::new()
        .name("tray-refresh".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(3));
            refresh(&app_poll);
        })
        .ok();

    Ok(())
}
