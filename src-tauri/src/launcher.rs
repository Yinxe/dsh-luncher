use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::Manager;

use crate::installed::InstalledVersion;
use crate::settings::Settings;
use crate::util;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub ok: bool,
    pub message: String,
}

fn ok(msg: impl Into<String>) -> LaunchResult {
    LaunchResult {
        ok: true,
        message: msg.into(),
    }
}

fn fail(msg: impl Into<String>) -> LaunchResult {
    LaunchResult {
        ok: false,
        message: msg.into(),
    }
}

/// 组装在终端里执行的命令。managed/global 安装用绝对 node + bin.js，PATH 安装直接执行其入口。
pub fn build_inner_command(
    target: &InstalledVersion,
    node: Option<&Path>,
    args: &str,
    profile: &str,
) -> String {
    // --profile 放在最前（值为单个 token，整体转义）；args 逐个 token 转义
    let mut arg_str = String::new();
    let prof = profile.trim();
    if !prof.is_empty() {
        arg_str.push_str("--profile ");
        arg_str.push_str(&util::shell_quote(prof));
    }
    for a in args.trim().split_whitespace() {
        arg_str.push(' ');
        arg_str.push_str(&util::shell_quote(a));
    }
    let arg_part = if arg_str.is_empty() {
        String::new()
    } else {
        format!(" {arg_str}")
    };

    let run_line = if let (Some(bin_js), Some(n)) = (target.bin_js.as_deref(), node) {
        format!(
            "{} {}{}",
            util::shell_quote(n.to_string_lossy().as_ref()),
            util::shell_quote(bin_js),
            arg_part
        )
    } else {
        // 兜底：bin.js 有 shebang 可直接执行，否则退回可执行文件路径
        let bin = target
            .bin_js
            .clone()
            .unwrap_or_else(|| target.location.clone());
        format!("{}{}", util::shell_quote(&bin), arg_part)
    };

    let path_line = node
        .and_then(|n| n.parent())
        .map(|dir| format!("export PATH='{}':\"$PATH\"\n", dir.display()))
        .unwrap_or_default();

    // 不用 exec：退出后显示退出码并等待回车，报错不会被终端闪退吞掉
    #[cfg(windows)]
    return format!("{path_line}{run_line}");
    #[cfg(not(windows))]
    return format!(
        "{path_line}{run_line}\ncode=$?\necho\necho \"—— dsh 已退出（退出码 $code），按回车关闭 ——\"\nread _"
    );
}

#[cfg(target_os = "macos")]
fn spawn_terminal(settings: &Settings, inner: &str) -> Result<(), String> {
    let dir = std::env::temp_dir();
    let script = dir.join(format!(
        "dsh-launch-{}.command",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let body = format!("#!/bin/bash\ncd \"$HOME\"\n{inner}\nexec $SHELL\n");
    std::fs::write(&script, body).map_err(|e| format!("写临时脚本失败: {e}"))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("设置脚本权限失败: {e}"))?;
    std::process::Command::new("open")
        .args(["-a", "Terminal", &script.to_string_lossy()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("打开 Terminal 失败: {e}"))?;
    let _ = settings;
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_terminal(settings: &Settings, inner: &str) -> Result<(), String> {
    // 优先 Windows Terminal
    if let Some(wt) = util::which("wt") {
        let _ = std::process::Command::new(wt)
            .args(["cmd", "/K", inner])
            .spawn();
        return Ok(());
    }
    std::process::Command::new("cmd")
        .args(["/C", "start", "", "cmd", "/K", inner])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动终端失败: {e}"))?;
    let _ = settings;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_terminal(settings: &Settings, inner: &str) -> Result<(), String> {
    // 覆盖设置：直接用指定的终端可执行文件
    let override_path = settings.terminal.trim();
    let candidates: Vec<(PathBuf, Vec<String>)> =
        if !override_path.is_empty() && override_path != "auto" {
            let p = PathBuf::from(override_path);
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let flag = if name == "gnome-terminal" { "--" } else { "-e" }.to_string();
            vec![(p, vec![flag])]
        } else {
            let mut v = Vec::new();
            let presets: &[(&str, &str)] = &[
                ("gnome-terminal", "--"),
                ("konsole", "-e"),
                ("x-terminal-emulator", "-e"),
                ("xfce4-terminal", "-x"),
                ("alacritty", "-e"),
                ("tilix", "-e"),
                ("kitty", ""),
                ("wezterm", "start --"),
                ("foot", ""),
            ];
            for (name, flag) in presets {
                if let Some(bin) = util::which(name) {
                    let args: Vec<String> = if flag.is_empty() {
                        vec![]
                    } else {
                        flag.split_whitespace().map(|s| s.to_string()).collect()
                    };
                    v.push((bin, args));
                }
            }
            v
        };

    for (bin, mut targs) in candidates {
        targs.push("/bin/sh".into());
        targs.push("-c".into());
        targs.push(inner.to_string());
        let spawned = std::process::Command::new(&bin)
            .args(&targs)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => return Ok(()),
            Err(_) => continue,
        }
    }
    Err("未找到可用的终端模拟器。可在设置中指定终端路径，或复制页面上的命令到任意终端运行。".into())
}

/// 在新终端窗口里启动指定版本
pub fn launch(
    settings: &Settings,
    target: &InstalledVersion,
    args: &str,
    profile: &str,
) -> LaunchResult {
    if target.version == "unknown" {
        return fail("该 PATH 记录缺少版本信息，无法启动；请用本启动器安装一个版本");
    }
    let node = util::find_node(&settings.node_path);
    let inner = build_inner_command(target, node.as_deref(), args, profile);
    match spawn_terminal(settings, &inner) {
        Ok(()) => {
            let prof_note = if profile.trim().is_empty() {
                String::new()
            } else {
                format!("（profile: {}）", profile.trim())
            };
            ok(format!(
                "已在终端窗口启动 dsh {}{prof_note}",
                target.version
            ))
        }
        Err(e) => fail(format!("{e}\n手动命令：{inner}")),
    }
}

/// 托盘/命令用的“启动最新已安装版本”
pub fn launch_latest(app: &tauri::AppHandle) -> LaunchResult {
    let state = app.state::<crate::settings::AppState>();
    let settings = state.settings.lock().unwrap().clone();
    let installed = crate::installed::collect_installed(&settings);
    match crate::installed::pick_latest(&installed) {
        Some(t) => launch(&settings, &t, &settings.default_args, &settings.default_profile),
        None => fail("还没有已安装的 dsh 版本，请先在界面中安装"),
    }
}
