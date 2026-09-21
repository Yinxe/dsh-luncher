use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};

use crate::profiles::ProfileInfo;
use crate::procs::ProfileInstance;

const TRAY_ID: &str = "main-tray";

/// 上次菜单内容签名：内容没变不 set_menu，避免菜单无谓重建/闪动
#[derive(Default)]
pub struct MenuState(Mutex<Option<String>>);

/// 唤起主窗口并聚焦。托盘「打开主界面」、二次启动转交都走这里，
/// 保证隐藏到托盘后再次启动启动器时窗口一定能回到前台。
pub fn show_main_window(app: &AppHandle) {
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

/// 采集托盘菜单需要的数据：会枚举进程 / 扫盘 / 探端口（Windows 上一次枚举 1s 级），
/// **只能在后台线程调用**，理由见 `refresh_async`。
fn collect(app: &AppHandle) -> (Vec<ProfileInfo>, Vec<ProfileInstance>) {
    let procs = app.state::<crate::procs::ProcState>();
    (
        crate::profiles::scan_profiles(),
        crate::procs::profile_instances(procs.inner()),
    )
}

/// 按当前 profile 运行状态组装菜单，返回菜单与内容签名。
/// profile 段为纯状态展示（不可点击）：● 运行中 / ○ 未运行。
///
/// `probing` = 还没做过第一次检测（启动瞬间），此时显示「正在检测实例…」，
/// 而不是让用户看到一句错误的「未发现 dsh profile」。
fn build_menu(
    app: &AppHandle,
    known: &[ProfileInfo],
    instances: &[ProfileInstance],
    probing: bool,
) -> tauri::Result<(Menu<tauri::Wry>, String)> {
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
    for p in known {
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
    for i in instances {
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
        // 签名必须让「检测中」和「确实没有 profile」区分开，否则菜单不会从前者换成后者
        sig.push_str(if probing { "probing;" } else { "-" });
        items.push(Box::new(MenuItem::with_id(
            app,
            if probing { "probing" } else { "no-profile" },
            if probing {
                "正在检测实例…"
            } else {
                "未发现 dsh profile"
            },
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

/// 按当前状态刷新托盘菜单（内容变化才真正 set_menu）。
/// ⚠️ 只在后台线程调用：`collect` 会枚举系统进程。
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let (known, instances) = collect(app);
    let Ok((menu, sig)) = build_menu(app, &known, &instances, false) else { return };
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

    // 初始菜单里**不含任何进程枚举**：`setup` 跑在主线程上，而枚举进程（Windows 起
    // PowerShell、逐个探端口）会把它占住几秒，窗口能出现却一直白屏「未响应」
    // （WebView2 拿不到消息泵，渲染不出来）——这正是 issue #1 的现场。
    // 真实状态交给下面的 refresh_async 在后台线程补上，通常几百毫秒内就到位。
    let menu = build_menu(app, &[], &[], true)?.0;
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
            } => refresh_async(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    // 后台补齐真实状态（含已知 profile 与运行中的实例）
    refresh_async(app);

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
