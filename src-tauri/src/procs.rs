use serde::Serialize;
use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

use crate::installed::InstalledVersion;
use crate::settings::Settings;
use crate::util;

/// 所有内嵌运行的 dsh 子进程；启动器退出时全部结束
#[derive(Default, Clone)]
pub struct ProcState {
    pub procs: Arc<Mutex<HashMap<u32, ProcHandle>>>,
    /// 全局停止标记（保留给未来优雅退出用）
    #[allow(dead_code)]
    pub shutting_down: Arc<AtomicBool>,
}

pub struct ProcHandle {
    pub version: String,
    pub profile: String,
    pub started_at: SystemTime,
    pub child: Arc<Mutex<Option<Child>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcInfo {
    pub id: u32,
    pub version: String,
    pub profile: String,
    pub started_at: u64,
    pub running: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcLogEvent {
    pub id: u32,
    pub version: String,
    pub profile: String,
    pub line: String,
    pub stream: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcExitEvent {
    pub id: u32,
    pub version: String,
    pub profile: String,
    /// None = 被启动器停止或被信号杀死
    pub code: Option<i32>,
    /// true = 由停止按钮触发
    pub stopped: bool,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn emit_log(app: &AppHandle, ev: &ProcLogEvent) {
    let _ = app.emit("proc-log", ev);
}

/// 以启动器子进程方式运行 dsh（stdin 关闭、stdout/stderr 管道回传日志）。
///
/// 必须在**专属常驻线程**中调用（见 `spawn_keeper`）：PDEATHSIG 绑定的是 fork
/// 它的线程，tokio spawn_blocking 的工作线程约 10s 就会被回收，会把 dsh 一起杀掉。
pub fn spawn_embedded(
    app: AppHandle,
    state: &ProcState,
    settings: &Settings,
    target: &InstalledVersion,
    profile: &str,
    args: &str,
) -> Result<ProcInfo, String> {
    if target.version == "unknown" {
        return Err("该 PATH 记录缺少版本信息，无法内嵌启动".into());
    }
    let bin_js = target
        .bin_js
        .clone()
        .ok_or_else(|| "该版本缺少 bin.js，安装可能不完整，请重装".to_string())?;
    let node = util::find_node(&settings.node_path)
        .ok_or_else(|| "未找到 Node.js，无法启动 dsh".to_string())?;

    let mut cmd = Command::new(&node);
    cmd.arg(&bin_js);
    let prof = profile.trim();
    if !prof.is_empty() {
        cmd.arg("--profile").arg(prof);
    }
    for a in args.trim().split_whitespace() {
        cmd.arg(a);
    }
    // 非 shell 启动：node 目录放进 PATH 供 dsh 的子进程使用
    util::with_node_on_path(&mut cmd, Some(&node));
    cmd.env("DSH_LAUNCHER_MANAGED", "1");
    // 启动器死亡（含被强杀）时由内核立即结束 dsh
    util::bind_to_parent_lifetime(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("启动 dsh 失败: {e}"))?;
    let id = child.id();
    let handle = ProcHandle {
        version: target.version.clone(),
        profile: prof.to_string(),
        started_at: SystemTime::now(),
        child: Arc::new(Mutex::new(Some(child))),
    };

    if let Some(out) = handle.child.lock().unwrap().as_mut().unwrap().stdout.take() {
        spawn_reader(
            app.clone(),
            ProcLogEvent {
                id,
                version: handle.version.clone(),
                profile: handle.profile.clone(),
                line: String::new(),
                stream: "stdout".into(),
            },
            out,
        );
    }
    if let Some(err) = handle.child.lock().unwrap().as_mut().unwrap().stderr.take() {
        spawn_reader(
            app.clone(),
            ProcLogEvent {
                id,
                version: handle.version.clone(),
                profile: handle.profile.clone(),
                line: String::new(),
                stream: "stderr".into(),
            },
            err,
        );
    }

    state.procs.lock().unwrap().insert(id, handle);

    Ok(ProcInfo {
        id,
        version: target.version.clone(),
        profile: prof.to_string(),
        started_at: now_millis(),
        running: true,
    })
}

/// 在专属 keeper 线程中 spawn dsh 并守候其退出。
/// 线程存活期 == 子进程存活期，保证 PDEATHSIG 不会提前误杀（tokio 线程约 10s 回收）。
/// 返回接收结果的 channel；spawn 结果（含错误）通过它回传。
pub fn spawn_keeper(
    app: AppHandle,
    state: ProcState,
    settings: Settings,
    target: InstalledVersion,
    profile: String,
    args: String,
) -> std::sync::mpsc::Receiver<Result<ProcInfo, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("dsh-keeper".into())
        .spawn(move || {
            let result = spawn_embedded(app.clone(), &state, &settings, &target, &profile, &args);
            match result {
                Ok(info) => {
                    let _ = tx.send(Ok(info.clone()));
                    // 阻塞到子进程退出；期间本线程不能结束，否则 PDEATHSIG 会误杀子进程
                    wait_exit(app, state, info.id);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        })
        .expect("创建 dsh keeper 线程失败");
    rx
}

fn spawn_reader<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    mut template: ProcLogEvent,
    pipe: R,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let l = l.trim_end();
                    if l.is_empty() {
                        continue;
                    }
                    template.line = l.to_string();
                    emit_log(&app, &template);
                }
                Err(_) => break,
            }
        }
    })
}

/// 等待进程退出并广播事件；被 stop() 拿走 child 时按“已停止”上报
fn wait_exit(app: AppHandle, state: ProcState, id: u32) {
    let (version, profile, outcome) = loop {
        let mut guard = state.procs.lock().unwrap();
        let Some(handle) = guard.get_mut(&id) else {
            return; // 已被 stop() 移除并上报
        };
        let mut child_guard = handle.child.lock().unwrap();
        match child_guard.as_mut() {
            Some(c) => match c.try_wait() {
                Ok(Some(status)) => {
                    let version = handle.version.clone();
                    let profile = handle.profile.clone();
                    drop(child_guard);
                    guard.remove(&id);
                    break (version, profile, Ok(status.code()));
                }
                Ok(None) => {}
                Err(_) => {
                    let version = handle.version.clone();
                    let profile = handle.profile.clone();
                    drop(child_guard);
                    guard.remove(&id);
                    break (version, profile, Err(()));
                }
            },
            None => return, // stop() 已处理
        }
        drop(child_guard);
        drop(guard);
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    let _ = app.emit(
        "proc-exit",
        ProcExitEvent {
            id,
            version,
            profile,
            code: outcome.ok().flatten(),
            stopped: false,
        },
    );
}

/// 停止指定进程（SIGKILL）；返回是否存在。停止后会广播 stopped 退出事件。
pub fn stop(app: &AppHandle, state: &ProcState, id: u32) -> bool {
    let mut guard = state.procs.lock().unwrap();
    let Some(handle) = guard.remove(&id) else {
        return false;
    };
    let version = handle.version.clone();
    let profile = handle.profile.clone();
    if let Some(mut c) = handle.child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let _ = app.emit(
        "proc-exit",
        ProcExitEvent {
            id,
            version,
            profile,
            code: None,
            stopped: true,
        },
    );
    true
}

/// 停止所有内嵌进程（启动器退出时调用）
pub fn stop_all(app: &AppHandle, state: &ProcState) {
    let ids: Vec<u32> = {
        let guard = state.procs.lock().unwrap();
        guard.keys().copied().collect()
    };
    for id in ids {
        stop(app, state, id);
    }
}

/// 列出仍在运行的进程
pub fn list(state: &ProcState) -> Vec<ProcInfo> {
    let guard = state.procs.lock().unwrap();
    let mut out: Vec<ProcInfo> = guard
        .iter()
        .map(|(id, h)| ProcInfo {
            id: *id,
            version: h.version.clone(),
            profile: h.profile.clone(),
            started_at: h
                .started_at
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            running: true,
        })
        .collect();
    out.sort_by_key(|p| p.id);
    out
}

/// 扫描系统中所有 dsh 进程（含终端/外部启动的），返回 (pid, profile)。
/// 用于保证「同一 profile 全系统同时只能有一个实例」。
pub fn external_running_profile_pids(exclude: &[u32]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if exclude.contains(&pid) {
            continue;
        }
        let Ok(cmdline) = std::fs::read_to_string(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        if !cmdline.contains("@deepseek-ai/dsh") || !cmdline.contains("bin.js") {
            continue;
        }
        let parts: Vec<&str> = cmdline.split('\0').filter(|s| !s.is_empty()).collect();
        let mut profile = String::new();
        for (i, part) in parts.iter().enumerate() {
            // 形式一：--profile <name>；形式二：bin.js <name>（位置参数）
            if *part == "--profile" {
                if let Some(v) = parts.get(i + 1) {
                    profile = (*v).to_string();
                }
                break;
            }
            if part.ends_with("bin.js") {
                if let Some(v) = parts.get(i + 1) {
                    if !v.starts_with('-') {
                        profile = (*v).to_string();
                    }
                }
            }
        }
        out.push((pid, profile));
    }
    out
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInstance {
    pub profile: String,
    pub running: bool,
    pub pid: Option<u32>,
    /// embedded = 启动器子进程；external = 终端/外部启动
    pub source: Option<String>,
    /// 内嵌实例对应的 dsh 版本（外部实例未知）
    pub version: Option<String>,
}

/// 汇总各 profile 的实例状态（内嵌 + 系统中的外部进程）
pub fn profile_instances(state: &ProcState) -> Vec<ProfileInstance> {
    let mut out: Vec<ProfileInstance> = Vec::new();
    let mut embedded_pids: Vec<u32> = Vec::new();
    {
        let guard = state.procs.lock().unwrap();
        for (id, h) in guard.iter() {
            embedded_pids.push(*id);
            out.push(ProfileInstance {
                profile: h.profile.clone(),
                running: true,
                pid: Some(*id),
                source: Some("embedded".into()),
                version: Some(h.version.clone()),
            });
        }
    }
    for (pid, profile) in external_running_profile_pids(&embedded_pids) {
        // 已有同 profile 内嵌实例时外部进程按 PID 并列列出
        out.push(ProfileInstance {
            profile,
            running: true,
            pid: Some(pid),
            source: Some("external".into()),
            version: None,
        });
    }
    out.sort_by(|a, b| a.profile.cmp(&b.profile));
    out
}

/// 停止某个 profile 的实例：优先内嵌，其次外部进程
pub fn stop_profile(app: &AppHandle, state: &ProcState, profile: &str) -> Result<bool, String> {
    let profile = profile.trim();
    if profile.is_empty() {
        return Err("profile 名称为空".into());
    }
    // 1) 内嵌实例
    let embedded_id = {
        let guard = state.procs.lock().unwrap();
        guard
            .iter()
            .find(|(_, h)| h.profile == profile)
            .map(|(id, _)| *id)
    };
    if let Some(id) = embedded_id {
        let _ = stop(app, state, id);
        return Ok(true);
    }
    // 2) 外部进程（同用户可直接 SIGKILL）
    for (pid, prof) in external_running_profile_pids(&[]) {
        if prof == profile {
            #[cfg(target_os = "linux")]
            {
                let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                if rc != 0 {
                    return Err(format!("结束进程 {pid} 失败"));
                }
                return Ok(true);
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = pid;
                return Err("当前平台暂不支持停止外部实例".into());
            }
        }
    }
    Ok(false)
}
