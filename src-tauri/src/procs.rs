use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
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
    let node = util::find_node(settings)
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

    let info = ProcInfo {
        id,
        version: target.version.clone(),
        profile: prof.to_string(),
        started_at: now_millis(),
        running: true,
    };
    // 通知托盘等无 UI 依赖的监听方立即刷新状态
    let _ = app.emit("proc-started", &info);
    Ok(info)
}

/// 以独立进程方式运行 dsh：自成进程组、不绑 PDEATHSIG，日志重定向到文件，
/// 启动器退出后继续运行；重启后由 /proc 扫描重新识别（environ 标记 DSH_LAUNCHER_DETACHED）。
/// 不登记 ProcState（不随启动器退出被杀），实例感知与停止走外部进程扫描通道。
pub fn spawn_detached(
    settings: &Settings,
    target: &InstalledVersion,
    profile: &str,
    args: &str,
) -> Result<ProcInfo, String> {
    if target.version == "unknown" {
        return Err("该 PATH 记录缺少版本信息，无法启动".into());
    }
    let bin_js = target
        .bin_js
        .clone()
        .ok_or_else(|| "该版本缺少 bin.js，安装可能不完整，请重装".to_string())?;
    let node = util::find_node(settings)
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
    util::with_node_on_path(&mut cmd, Some(&node));
    cmd.env("DSH_LAUNCHER_MANAGED", "1");
    // 独立进程标记：/proc/<pid>/environ 扫描据此区分「启动器派生」与「终端启动」
    cmd.env("DSH_LAUNCHER_DETACHED", "1");
    // 脱离启动器进程组，终端信号与启动器退出都不波及
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.stdin(Stdio::null());
    // 日志写入文件而非管道：管道会绑住启动器生命周期（写已关闭的管道会被 SIGPIPE 杀死）
    let logs_dir = crate::settings::launcher_home().join("instance-logs");
    std::fs::create_dir_all(&logs_dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
    let log_path = logs_dir.join(format!(
        "{}-{}.log",
        if prof.is_empty() { "default" } else { prof },
        now_millis()
    ));
    let log_out = std::fs::File::create(&log_path).map_err(|e| format!("创建日志文件失败: {e}"))?;
    let log_err = log_out
        .try_clone()
        .map_err(|e| format!("复制日志文件句柄失败: {e}"))?;
    cmd.stdout(log_out).stderr(log_err);

    let mut child = cmd.spawn().map_err(|e| format!("启动 dsh 失败: {e}"))?;
    let id = child.id();
    // 后台收尸线程防僵尸；启动器先退出时由 init 接管收尸
    std::thread::Builder::new()
        .name("dsh-detached-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .ok();

    // 登记注册表（跨平台扫描依据；best-effort，Linux 另有 /proc 兜底）
    let _ = append_detached_record(&DetachedRecord {
        pid: id,
        profile: prof.to_string(),
        version: target.version.clone(),
        started_at: now_millis(),
        log_file: log_path.to_string_lossy().into_owned(),
    });

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
    // 关键：摘下句柄后就释放 state.procs 锁，kill/wait 与 emit 都必须在锁外做。
    // `app.emit` 是**同步**的：它会在当前线程直接回调 Rust 监听器
    // （tray::refresh → profile_instances），而后者会再次获取同一把 std::sync::Mutex
    // （不可重入）——持锁 emit 必然自死锁，界面随即无响应。
    let (version, profile, child) = {
        let mut guard = state.procs.lock().unwrap();
        let Some(handle) = guard.remove(&id) else {
            return false;
        };
        let child = handle.child.lock().unwrap().take();
        (handle.version.clone(), handle.profile.clone(), child)
    };

    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    // 进程已死：清掉枚举缓存，避免状态轮询在 TTL 内仍把它当活实例
    invalidate_process_cache();

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

// ── 独立进程注册表（跨平台） ─────────────────
// Linux 有 /proc 可直接扫描；macOS/Windows 没有，因此独立进程启动时把
// pid/profile/version 记入 ~/.dsh-launcher/detached.json，扫描时校验存活
// 并清除陈旧条目（PID 复用防护：要求进程 cmdline 仍为 dsh）。

#[derive(Clone, Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DetachedRecord {
    pub pid: u32,
    pub profile: String,
    pub version: String,
    pub started_at: u64,
    pub log_file: String,
}

fn detached_registry_path() -> std::path::PathBuf {
    crate::settings::launcher_home().join("detached.json")
}

fn registry_lock() -> &'static Mutex<()> {
    use std::sync::OnceLock;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_detached_registry() -> Vec<DetachedRecord> {
    std::fs::read_to_string(detached_registry_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_detached_registry(records: &[DetachedRecord]) -> Result<(), String> {
    let path = detached_registry_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("写入失败: {e}"))
}

fn append_detached_record(rec: &DetachedRecord) -> Result<(), String> {
    let _g = registry_lock().lock().unwrap();
    let mut records = read_detached_registry();
    records.retain(|r| r.pid != rec.pid);
    records.push(rec.clone());
    write_detached_registry(&records)
}

fn remove_detached_record(pid: u32) {
    let _g = registry_lock().lock().unwrap();
    let mut records = read_detached_registry();
    let before = records.len();
    records.retain(|r| r.pid != pid);
    if records.len() != before {
        let _ = write_detached_registry(&records);
    }
}

/// cmdline 是否为 dsh 进程（@deepseek--ai/dsh 的 bin.js；Windows 路径分隔符为反斜杠）
fn is_dsh_cmdline(cmd: &str) -> bool {
    (cmd.contains("@deepseek-ai/dsh") || cmd.contains("@deepseek-ai\\dsh"))
        && cmd.contains("bin.js")
}

/// 目标 pid 是否仍是一个 dsh 进程（存活校验 + PID 复用防护），全平台。
///
/// 端口占用归属判断也复用它：端口可能被无关程序占用，**必须**先确认是 dsh 才敢杀。
pub(crate) fn pid_is_dsh(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| is_dsh_cmdline(&String::from_utf8_lossy(&b).replace('\0', " ")))
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "command="])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| is_dsh_cmdline(String::from_utf8_lossy(&o.stdout).trim()))
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // tasklist 快且原生；只校验存活与 node 映像（PID 复用为其他 node 进程的概率可忽略）
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains(&pid.to_string()) && out.to_ascii_lowercase().contains(".exe")
            })
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = pid;
        false
    }
}

/// 校验独立进程注册表：剔除已死亡或不再是 dsh 的陈旧条目，返回仍存活的记录。
/// 有清理时回写注册表文件。
pub fn validate_detached_registry() -> Vec<DetachedRecord> {
    let _g = registry_lock().lock().unwrap();
    let records = read_detached_registry();
    let mut valid = Vec::new();
    let mut changed = false;
    for r in records {
        if pid_is_dsh(r.pid) {
            valid.push(r);
        } else {
            changed = true;
        }
    }
    if changed {
        let _ = write_detached_registry(&valid);
    }
    valid
}

/// 跨平台结束独立进程：unix 用 SIGKILL，Windows 用 taskkill /F
fn force_kill_pid(pid: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        if rc != 0 {
            return Err(format!("结束进程 {pid} 失败"));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("taskkill");
        hide_window(&mut cmd);
        let out = cmd
            .args(["/F", "/PID", &pid.to_string()])
            .output()
            .map_err(|e| format!("执行 taskkill 失败: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "结束进程 {pid} 失败：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err("当前平台不支持结束独立进程".into())
    }
}

/// /proc/<pid>/environ 是否包含指定 KV（如 DSH_LAUNCHER_DETACHED=1）。
/// environ 可能含非 UTF-8 字节，按字节读再容错转码。
fn detached_marker(pid: u32) -> bool {
    std::fs::read(format!("/proc/{pid}/environ"))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .split('\0')
                .any(|kv| kv == "DSH_LAUNCHER_DETACHED=1")
        })
        .unwrap_or(false)
}

/// Windows 下生成的控制台子进程不弹窗（启动器为 GUI 程序）
#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn hide_window(_cmd: &mut Command) {}

/// 全平台进程枚举 (pid, cmdline 单行空格分隔)：
/// Linux 读 /proc；macOS 用 `ps -axo pid=,command=`；Windows 用 PowerShell CIM
/// （EncodedCommand 免引号转义）。Windows 枚举慢（1s 级），按平台 TTL 缓存。
type ProcessCache = Mutex<Option<(Instant, std::sync::Arc<Vec<(u32, String)>>)>>;

fn process_cache() -> &'static ProcessCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<ProcessCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// 清空进程枚举缓存。停止/杀掉实例后必须调用：否则运行状态轮询会在 TTL 内
/// （Windows 5s / 其他 2s）继续把已死的 PID 当活实例，重启流程会反复重试，
/// 甚至在 PID 被复用后误杀无关进程。
pub fn invalidate_process_cache() {
    if let Ok(mut guard) = process_cache().lock() {
        *guard = None;
    }
}

pub fn enumerate_processes() -> std::sync::Arc<Vec<(u32, String)>> {
    let ttl = if cfg!(windows) {
        Duration::from_secs(5)
    } else {
        Duration::from_secs(2)
    };
    {
        let guard = process_cache().lock().unwrap();
        if let Some((at, list)) = guard.as_ref() {
            if at.elapsed() < ttl {
                return list.clone();
            }
        }
    }
    let list = std::sync::Arc::new(enumerate_processes_uncached());
    *process_cache().lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

fn enumerate_processes_uncached() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    #[cfg(target_os = "linux")]
    {
        if let Ok(rd) = std::fs::read_dir("/proc") {
            for entry in rd.flatten() {
                let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
                    continue;
                };
                let cmd = String::from_utf8_lossy(&cmdline).replace('\0', " ");
                if cmd.trim().is_empty() {
                    continue;
                }
                out.push((pid, cmd));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(o) = Command::new("ps").args(["-axo", "pid=,command="]).output() {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let Some((pid, rest)) = line.trim_start().split_once(char::is_whitespace) else {
                    continue;
                };
                let Ok(pid) = pid.trim().parse::<u32>() else {
                    continue;
                };
                let cmd = rest.trim();
                if !cmd.is_empty() {
                    out.push((pid, cmd.to_string()));
                }
            }
        }
    }
    #[cfg(windows)]
    {
        let script = "Get-CimInstance Win32_Process | ForEach-Object { \"{0}`t{1}\" -f $_.ProcessId, $_.CommandLine }";
        let mut cmd = Command::new("powershell");
        hide_window(&mut cmd);
        if let Ok(o) = cmd
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &crate::util::ps_encoded_command(script),
            ])
            .output()
        {
            for line in String::from_utf8_lossy(&o.stdout).lines() {
                let Some((pid, rest)) = line.split_once('\t') else {
                    continue;
                };
                let Ok(pid) = pid.trim().parse::<u32>() else {
                    continue;
                };
                let cmd = rest.trim();
                if !cmd.is_empty() {
                    out.push((pid, cmd.to_string()));
                }
            }
        }
    }
    out
}

/// 从 dsh cmdline 解析 profile 名：`--profile <name>` 或 `bin.js <name>`（位置参数）
fn parse_dsh_profile(cmd: &str) -> String {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut profile = String::new();
    for (i, part) in parts.iter().enumerate() {
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
    profile
}

/// 扫描系统中所有 dsh 进程（含终端/外部启动的），返回 (pid, profile)。
/// 用于保证「同一 profile 全系统同时只能有一个实例」。
pub fn external_running_profile_pids(exclude: &[u32]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for (pid, cmd) in enumerate_processes().iter() {
        if exclude.contains(pid) {
            continue;
        }
        if !is_dsh_cmdline(cmd) {
            continue;
        }
        out.push((*pid, parse_dsh_profile(cmd)));
    }
    out
}

/// 扫描外部 dsh 进程中，bin.js 位于指定版本目录下的 PID。
/// 用于卸载/重装前确认该版本目录没有正在运行的外部实例在使用。
pub fn external_pids_under_dir(dir: &Path) -> Vec<u32> {
    let prefix = format!("{}{}", dir.display(), std::path::MAIN_SEPARATOR);
    enumerate_processes()
        .iter()
        .filter(|(_, cmd)| cmd.contains(&prefix))
        .map(|(pid, _)| *pid)
        .collect()
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
    // 独立进程注册表（跨平台）：校验存活后并入；Linux 上 /proc 扫描同样会发现它们，
    // 因此把这些 PID 一并排除，避免同一实例既算 detached 又算 external
    let detached = validate_detached_registry();
    let mut claimed = embedded_pids;
    for r in &detached {
        claimed.push(r.pid);
        out.push(ProfileInstance {
            profile: r.profile.clone(),
            running: true,
            pid: Some(r.pid),
            source: Some("detached".into()),
            version: Some(r.version.clone()),
        });
    }
    for (pid, profile) in external_running_profile_pids(&claimed) {
        // 带 DSH_LAUNCHER_DETACHED 标记的是启动器派生的独立进程，其余为终端/外部启动
        let source = if detached_marker(pid) { "detached" } else { "external" };
        out.push(ProfileInstance {
            profile,
            running: true,
            pid: Some(pid),
            source: Some(source.into()),
            version: None,
        });
    }
    // 端口兜底：cmdline 认不出来的实例（换了包装、参数写法不同、跨启动器重启）只要
    // 还占着该 profile 配置的 web 端口，就同样算「运行中」——重启时才不会漏，
    // 也不会因为「以为没在跑」而重复拉起。
    for p in crate::profiles::scan_profiles() {
        if out.iter().any(|i| i.profile == p.name) {
            continue;
        }
        let Some((host, port)) = crate::profile_cfg::web_addr(&p.name) else {
            continue;
        };
        // 只有确认是本 profile 的 dsh（或归属未知时的保守认定）才算「运行中」：
        // 端口被无关程序、或被**另一个 profile**（复制 profile 会连端口一起抄）
        // 占用时都不能算，否则会凭空多出一个停不掉的「运行中」，还可能停错实例。
        let (pid, ok) = match port_owner(&host, port, &p.name) {
            Some(PortOwner::ThisProfile(pid)) => (Some(pid), true),
            Some(PortOwner::Unknown) => (None, true),
            _ => (None, false),
        };
        if !ok {
            continue;
        }
        out.push(ProfileInstance {
            profile: p.name.clone(),
            running: true,
            pid,
            source: Some("port".into()),
            version: None,
        });
    }
    out.sort_by(|a, b| a.profile.cmp(&b.profile));
    out
}

/// 目标 PID 的 cmdline（走带 TTL 缓存的进程枚举）。
fn pid_cmdline(pid: u32) -> Option<String> {
    enumerate_processes()
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, cmd)| cmd.clone())
}

/// 监听某端口的进程相对目标 profile 的归属。
///
/// 端口本身只说明「有人在服务」，不说明是谁。复制 profile 会把 `cordis.patch.yml`
/// 连 webserver 端口一起抄走，于是**多个 profile 可能配置同一端口**——只看端口会
/// 认错实例、甚至停错实例。因此对 dsh 进程再用它 cmdline 里的 `--profile` 二次确认。
pub(crate) enum PortOwner {
    /// 就是本 profile 的实例（cmdline 没写 profile 时也保守归到本 profile）
    ThisProfile(u32),
    /// 另一个 profile 的 dsh 实例
    OtherProfile { pid: u32, profile: String },
    /// 非 dsh 的其它进程
    OtherProcess(u32),
    /// 有人在监听，但确定不了是谁（权限不足等）
    Unknown,
}

/// 判定 `host:port` 的监听者归属；端口空闲返回 `None`。
pub(crate) fn port_owner(host: &str, port: u16, profile: &str) -> Option<PortOwner> {
    if !crate::netports::is_listening(host, port) {
        return None;
    }
    let Some(pid) = crate::netports::listener_pid(port) else {
        return Some(PortOwner::Unknown);
    };
    // 「是不是 dsh」用与 cmdline 扫描同一套判据（`is_dsh_cmdline`），保证三端一致；
    // 枚举不到 cmdline 时才退回平台存活校验。Windows 的存活校验只认 exe，
    // 单靠它会把任何占用该端口的程序都当成 dsh。
    let cmd = pid_cmdline(pid);
    let is_dsh = match cmd.as_deref() {
        Some(cmd) => is_dsh_cmdline(cmd),
        None => pid_is_dsh(pid),
    };
    if !is_dsh {
        return Some(PortOwner::OtherProcess(pid));
    }
    let owner = cmd.as_deref().map(parse_dsh_profile).unwrap_or_default();
    if owner.is_empty() || owner == profile {
        Some(PortOwner::ThisProfile(pid))
    } else {
        Some(PortOwner::OtherProfile { pid, profile: owner })
    }
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
    // 2) 独立进程注册表（跨平台：unix SIGKILL / Windows taskkill）
    let registry_hit = validate_detached_registry()
        .into_iter()
        .find(|r| r.profile == profile)
        .map(|r| r.pid);
    if let Some(pid) = registry_hit {
        force_kill_pid(pid)?;
        remove_detached_record(pid);
        invalidate_process_cache();
        return Ok(true);
    }
    // 3) 外部进程（跨平台强杀：unix SIGKILL / Windows taskkill）
    for (pid, prof) in external_running_profile_pids(&[]) {
        if prof == profile {
            force_kill_pid(pid)?;
            invalidate_process_cache();
            return Ok(true);
        }
    }
    // 4) 端口兜底：cmdline 认不出来的实例，只要还占着该 profile 的 web 端口就算它在跑。
    //    只杀确认属于**本 profile** 的 dsh——端口可能被无关程序、甚至另一个 profile
    //    （复制 profile 会连端口一起抄）占用，认错就停错，误杀是不可接受的。
    if let Some((host, port)) = crate::profile_cfg::web_addr(profile) {
        return match port_owner(&host, port, profile) {
            None => Ok(false),
            Some(PortOwner::ThisProfile(pid)) => {
                force_kill_pid(pid)?;
                invalidate_process_cache();
                Ok(true)
            }
            Some(PortOwner::OtherProfile { pid, profile: other }) => Err(format!(
                "端口 {port} 被 profile「{other}」的 dsh 实例（PID {pid}）占用；请先停止那个 profile，或到「快捷配置」改用其它端口"
            )),
            Some(PortOwner::OtherProcess(pid)) => Err(format!(
                "端口 {port} 被 PID {pid} 占用，但它不是 dsh 进程；为避免误杀已跳过，请手动处理"
            )),
            Some(PortOwner::Unknown) => Err(format!(
                "端口 {port} 仍在监听，但无法确定占用进程（可能需要更高权限）；请手动处理"
            )),
        };
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::DSH_ENV_LOCK;

    #[test]
    fn is_dsh_cmdline_matches_both_separators() {
        assert!(is_dsh_cmdline(
            "node /home/u/.dsh-launcher/versions/0.1.5/node_modules/@deepseek-ai/dsh/bin.js --profile web"
        ));
        // Windows 路径分隔符
        assert!(is_dsh_cmdline(
            "node C:\\Users\\u\\.dsh-launcher\\versions\\0.1.5\\node_modules\\@deepseek-ai\\dsh\\bin.js --profile web"
        ));
        assert!(!is_dsh_cmdline("node /some/other/bin.js --profile web"));
        assert!(!is_dsh_cmdline("node /home/u/@deepseek-ai/dsh/other.js"));
    }

    #[test]
    fn detached_registry_roundtrip_and_stale_purge() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-detreg-{}", std::process::id()));
        std::env::set_var("DSH_LAUNCHER_HOME", &tmp);

        let mk = |pid: u32, profile: &str| DetachedRecord {
            pid,
            profile: profile.into(),
            version: "0.1.0".into(),
            started_at: 1,
            log_file: "/tmp/x.log".into(),
        };

        // 写入可读回；同 pid 重复登记覆盖不叠加
        append_detached_record(&mk(u32::MAX, "web")).unwrap();
        append_detached_record(&mk(u32::MAX, "web")).unwrap();
        append_detached_record(&mk(u32::MAX - 1, "web2")).unwrap();
        assert_eq!(read_detached_registry().len(), 2);
        assert_eq!(read_detached_registry()[0].profile, "web");

        // 校验：不存在的 PID（u32::MAX / MAX-1 必然无进程）作为陈旧条目被清出并回写文件
        let valid = validate_detached_registry();
        assert!(valid.is_empty());
        assert!(read_detached_registry().is_empty());

        // remove_detached_record：不存在时不动文件
        remove_detached_record(u32::MAX);
        assert!(read_detached_registry().is_empty());
    }

    #[test]
    fn enumerate_processes_includes_self_and_parses_profile() {
        // 三端都应能枚举到测试进程自身（/proc、ps、PowerShell 皆含自身）
        let list = enumerate_processes();
        assert!(
            list.iter().any(|(pid, _)| *pid == std::process::id()),
            "进程枚举应包含自身 pid"
        );
    }

    #[test]
    fn invalidate_process_cache_forces_reenumeration() {
        // TTL 内两次枚举应命中同一份缓存
        let a = enumerate_processes();
        let b = enumerate_processes();
        assert!(std::sync::Arc::ptr_eq(&a, &b), "TTL 内应命中枚举缓存");

        // 显式失效（停止实例后调用）后必须重新枚举，否则会把已死 PID 当活实例
        invalidate_process_cache();
        let c = enumerate_processes();
        assert!(
            !std::sync::Arc::ptr_eq(&b, &c),
            "invalidate_process_cache 后应重新枚举"
        );
        assert!(c.iter().any(|(pid, _)| *pid == std::process::id()));
    }

    #[test]
    fn port_owner_ignores_non_dsh_listener() {
        // 测试进程自己在监听一个端口，但它不是 dsh：必须判为 OtherProcess，
        // 否则端口被别人占着就会凭空冒出「profile 运行中」，还可能停错进程。
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        let got = port_owner("127.0.0.1", port, "web");
        match got {
            Some(PortOwner::OtherProcess(pid)) => assert_eq!(pid, std::process::id()),
            Some(PortOwner::ThisProfile(_)) | Some(PortOwner::OtherProfile { .. }) => {
                panic!("非 dsh 的监听者不应被认成某个 profile 的实例")
            }
            Some(PortOwner::Unknown) => eprintln!("本环境拿不到占用进程，跳过断言"),
            None => panic!("已 bind 的端口不应判定为空闲"),
        }
        drop(l);
        assert!(port_owner("127.0.0.1", port, "web").is_none(), "已释放的端口应判定为空闲");
    }

    #[test]
    fn parse_dsh_profile_handles_both_forms() {
        assert_eq!(
            parse_dsh_profile("node /x/@deepseek-ai/dsh/bin.js --profile web --no-open"),
            "web"
        );
        // 位置参数形式：bin.js <profile>
        assert_eq!(
            parse_dsh_profile("node /x/@deepseek-ai/dsh/bin.js web"),
            "web"
        );
        assert_eq!(
            parse_dsh_profile("node /x/@deepseek-ai/dsh/bin.js"),
            ""
        );
    }
}
