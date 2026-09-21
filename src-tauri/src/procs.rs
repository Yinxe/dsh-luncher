use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Stdio};
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

    let mut cmd = util::hidden_command(&node);
    cmd.arg(&bin_js);
    let prof = profile.trim();
    if !prof.is_empty() {
        cmd.arg("--profile").arg(prof);
    }
    for a in util::split_args(args) {
        cmd.arg(a);
    }
    // 非 shell 启动：node 目录放进 PATH 供 dsh 的子进程使用
    util::with_node_on_path(&mut cmd, Some(&node));
    cmd.env("DSH_STARTER_MANAGED", "1");
    // 启动器死亡（含被强杀）时由内核立即结束 dsh
    util::bind_to_parent_lifetime(&mut cmd);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| {
        crate::diag::error(
            "instance",
            &format!(
                "内嵌启动失败：dsh {} profile={prof:?}\n  命令: {}\n  错误: {e}",
                target.version,
                util::cmd_line(&cmd)
            ),
        );
        format!("启动 dsh 失败: {e}")
    })?;
    let id = child.id();
    crate::diag::info(
        "instance",
        &format!(
            "内嵌启动：pid={id} dsh={} profile={prof:?}\n  命令: {}",
            target.version,
            util::cmd_line(&cmd)
        ),
    );
    let child_cell = Arc::new(Mutex::new(Some(child)));
    let handle = ProcHandle {
        version: target.version.clone(),
        profile: prof.to_string(),
        started_at: SystemTime::now(),
        child: child_cell.clone(),
    };
    // 先登记、后接管道：中间任何一步 panic（如 reader 线程创建失败）都不会留下
    // 「已在运行、但注册表里查无此进程 → Windows 上杀不掉也没人收尸」的孤儿 dsh。
    state.procs.lock().unwrap().insert(id, handle);

    let (version, profile) = (target.version.clone(), prof.to_string());
    // 登记后 stop() 可能并发把 child 摘走（置 None），这里用 if let 而非 unwrap 兜底，
    // 取不到管道只意味着这一瞬已被停止，跳过挂 reader 即可。
    if let Some(out) = child_cell.lock().unwrap().as_mut().and_then(|c| c.stdout.take()) {
        spawn_reader(
            app.clone(),
            ProcLogEvent {
                id,
                version: version.clone(),
                profile: profile.clone(),
                line: String::new(),
                stream: "stdout".into(),
            },
            out,
        );
    }
    if let Some(err) = child_cell.lock().unwrap().as_mut().and_then(|c| c.stderr.take()) {
        spawn_reader(
            app.clone(),
            ProcLogEvent {
                id,
                version,
                profile,
                line: String::new(),
                stream: "stderr".into(),
            },
            err,
        );
    }

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
/// 启动器退出后继续运行；重启后由 /proc 扫描重新识别（environ 标记 DSH_STARTER_DETACHED）。
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

    let mut cmd = util::hidden_command(&node);
    cmd.arg(&bin_js);
    let prof = profile.trim();
    if !prof.is_empty() {
        cmd.arg("--profile").arg(prof);
    }
    for a in util::split_args(args) {
        cmd.arg(a);
    }
    util::with_node_on_path(&mut cmd, Some(&node));
    cmd.env("DSH_STARTER_MANAGED", "1");
    // 独立进程标记：/proc/<pid>/environ 扫描据此区分「启动器派生」与「终端启动」
    cmd.env("DSH_STARTER_DETACHED", "1");
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
        // DETACHED_PROCESS 已表示「不分配控制台」（会覆盖 hidden_command 的
        // CREATE_NO_WINDOW，两者语义一致：都不会弹黑框）
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.stdin(Stdio::null());
    // 日志写入文件而非管道：管道会绑住启动器生命周期（写已关闭的管道会被 SIGPIPE 杀死）
    let logs_dir = crate::settings::starter_home().join("instance-logs");
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

    let mut child = cmd.spawn().map_err(|e| {
        crate::diag::error(
            "instance",
            &format!(
                "独立进程启动失败：dsh {} profile={prof:?}\n  命令: {}\n  错误: {e}",
                target.version,
                util::cmd_line(&cmd)
            ),
        );
        format!("启动 dsh 失败: {e}")
    })?;
    let id = child.id();
    crate::diag::info(
        "instance",
        &format!(
            "独立进程启动：pid={id} dsh={} profile={prof:?} 日志={}\n  命令: {}",
            target.version,
            log_path.display(),
            util::cmd_line(&cmd)
        ),
    );

    // 先登记注册表、再放手交给收尸线程：登记失败时（磁盘满 / 权限等）macOS/Windows 没有
    // /proc 兜底，会留下界面既看不见也停不掉的独立进程。宁可立刻终止刚拉起的 dsh 并如实
    // 报错，也不假装启动成功、把孤儿留给用户。
    if let Err(e) = append_detached_record(&DetachedRecord {
        pid: id,
        profile: prof.to_string(),
        version: target.version.clone(),
        started_at: now_millis(),
        log_file: log_path.to_string_lossy().into_owned(),
    }) {
        let _ = child.kill();
        let _ = child.wait();
        crate::diag::error(
            "instance",
            &format!("登记独立进程失败，已终止刚启动的 dsh：pid={id} profile={prof:?} 错误={e}"),
        );
        return Err(format!("记录独立进程失败，已终止刚启动的 dsh（PID {id}）：{e}"));
    }

    // 后台收尸线程防僵尸；启动器先退出时由 init 接管收尸
    std::thread::Builder::new()
        .name("dsh-detached-reap".into())
        .spawn(move || {
            let _ = child.wait();
        })
        .ok();

    // 进程表快照里还没有这个新 pid（Windows TTL 8 秒、枚举本身还要 1 秒级），
    // 立刻要求重算一轮：实例在新表里可被认成 dsh，避免「刚起来就被当成 PID 复用」。
    invalidate_process_cache();

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
    crate::diag::info(
        "instance",
        &format!("停止内嵌实例：pid={id} dsh={version} profile={profile:?}"),
    );
    // 进程已死：从快照里摘掉它，界面下一轮不必再看到「运行中」
    forget_pid(id);
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
// pid/profile/version 记入 ~/.dsh-starter/detached.json，扫描时校验存活
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
    crate::settings::starter_home().join("detached.json")
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

/// 改名后同步独立进程登记表里的 profile 名。
/// 不同步的话，实例列表会残留「旧名 + 有 PID + profiles 目录里找不到」的幽灵条目。
pub fn rename_detached_profile(old: &str, new: &str) {
    let _g = registry_lock().lock().unwrap();
    let mut records = read_detached_registry();
    let mut changed = false;
    for r in records.iter_mut() {
        if r.profile == old {
            r.profile = new.to_string();
            changed = true;
        }
    }
    if changed {
        let _ = write_detached_registry(&records);
    }
}

/// 删除 profile 时清掉它的独立进程登记（正常情况下「运行中拒绝删除」已先拦住）
pub fn drop_detached_profile(name: &str) {
    let _g = registry_lock().lock().unwrap();
    let mut records = read_detached_registry();
    let before = records.len();
    records.retain(|r| r.profile != name);
    if records.len() != before {
        let _ = write_detached_registry(&records);
    }
}

/// cmdline 是否为 dsh 进程。两种形态都要认：
/// - 包内入口（启动器自己就是这么起的）：`.../@deepseek-ai/dsh/...bin.js`；
/// - npm 安装痕迹：argv 里出现 `.../node_modules/.bin/dsh`（Windows 是 `.bin\dsh.cmd`
///   / `.ps1`）。终端里 `npm exec @deepseek-ai/dsh …` / `npx @deepseek-ai/dsh …` /
///   全局安装的 `dsh` 命令**全是这个形态**，漏了它这些实例就「看不到、也停不掉」。
fn is_dsh_cmdline(cmd: &str) -> bool {
    let pkg_entry =
        (cmd.contains("@deepseek-ai/dsh") || cmd.contains("@deepseek-ai\\dsh")) && cmd.contains("bin.js");
    if pkg_entry {
        return true;
    }
    cmd.split_whitespace().any(|part| {
        let p = part.trim_matches(|c| c == '"' || c == '\'');
        let p = p
            .strip_suffix(".cmd")
            .or_else(|| p.strip_suffix(".ps1"))
            .unwrap_or(p);
        p.ends_with("node_modules/.bin/dsh") || p.ends_with("node_modules\\.bin\\dsh")
    })
}

/// PID 判定结果：`Unknown` 只在本机进程表整轮拿不到时出现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PidCheck {
    /// 确认是本机 dsh 进程
    Dsh,
    /// 确认不是：进程不存在，或存在但不是 dsh
    NotDsh,
    /// 进程表不可用，无法判定 —— 既不能当成「已死」，也不能当成「就是本 profile 的实例」
    Unknown,
}

/// 目标 pid 是否仍是一个 dsh 进程（存活校验 + PID 复用防护），全平台。
///
/// 判据与 cmdline 扫描完全同一套（`is_dsh_cmdline`）：早先 Windows 靠
/// `tasklist /FI "PID eq <pid>"` 逐 PID 起子进程、只认映像名 `node.exe`，
/// macOS 逐 PID 起 `ps`，既慢又会把恰好占着端口的无关 node 进程算成 dsh。
/// 现在一律查**带缓存的进程表快照**，不再为单个 PID 起任何子进程。
///
/// 端口占用归属判断也复用它：端口可能被无关程序占用，**必须**先确认是 dsh 才敢杀。
pub(crate) fn check_pid(pid: u32) -> PidCheck {
    if let Some((ok, list)) = read_snapshot() {
        // 这一轮枚举是失败的（如 PowerShell 起不来）：表不可信，不得据此判定生死
        if !ok {
            return PidCheck::Unknown;
        }
        return match list.iter().find(|(p, _)| *p == pid) {
            Some((_, cmd)) => {
                if is_dsh_cmdline(cmd) {
                    PidCheck::Dsh
                } else {
                    PidCheck::NotDsh
                }
            }
            // 表是全量的（含拿不到 cmdline 的进程）：查不到就是它已经不在了
            None => PidCheck::NotDsh,
        };
    }
    // 守护线程还没产出第一份快照：Linux 上 /proc 是即时的，直接读；其它平台不猜
    #[cfg(target_os = "linux")]
    {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| is_dsh_cmdline(&String::from_utf8_lossy(&b).replace('\0', " ")))
            .map(|dsh| if dsh { PidCheck::Dsh } else { PidCheck::NotDsh })
            .unwrap_or(PidCheck::NotDsh)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        PidCheck::Unknown
    }
}

/// 是否**确认**是本机 dsh 进程（`Unknown` / `NotDsh` 都是 false）。
/// 用于「不确定就不动手」的判定：宁可不动，也绝不误杀。
pub(crate) fn pid_is_dsh(pid: u32) -> bool {
    matches!(check_pid(pid), PidCheck::Dsh)
}

/// 校验独立进程注册表：剔除已死亡或不再是 dsh 的陈旧条目，返回仍存活的记录。
/// 有清理时回写注册表文件。
pub fn validate_detached_registry() -> Vec<DetachedRecord> {
    let _g = registry_lock().lock().unwrap();
    let records = read_detached_registry();
    let mut valid = Vec::new();
    let mut changed = false;
    for r in records {
        match check_pid(r.pid) {
            // 确认已死 / PID 已被复用成别的程序 → 清掉
            PidCheck::NotDsh => {
                // 例外：登记比当前进程表还新 —— 这张表拍完之后它才起来，查不到是必然，
                // 此刻清掉就等于把刚拉起的独立进程的日志通道（log_file）永久丢掉。
                // 保守留到下一轮枚举（TTL 8 秒内），那时表比启动时刻新，判定才作数。
                if snapshot_predates(r.started_at) {
                    crate::diag::debug("instance", || {
                        format!(
                            "独立进程 pid={} profile={:?} 晚于当前进程表快照，暂不判定存活（下一轮枚举再见分晓）",
                            r.pid, r.profile
                        )
                    });
                    valid.push(r);
                    continue;
                }
                crate::diag::info(
                    "instance",
                    &format!(
                        "清理失效的独立进程登记：pid={} profile={:?}（进程已退出或该 PID 已不是 dsh）",
                        r.pid, r.profile
                    ),
                );
                changed = true;
            }
            // 确认是 dsh，或本机进程表整轮不可用（保守保留）
            PidCheck::Dsh | PidCheck::Unknown => valid.push(r),
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
        let out = util::hidden_command("taskkill")
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

/// /proc/<pid>/environ 是否包含指定 KV（如 DSH_STARTER_DETACHED=1）。
/// environ 可能含非 UTF-8 字节，按字节读再容错转码。
///
/// 注意：这是 Linux 专属手段，Windows / macOS 上没有 /proc，恒返回 false。
/// 那两端「启动器派生的独立进程」靠 `detached.json` 登记识别（注册表分支给
/// `source: detached`）；只有登记被清掉后仍存活、又被 cmdline 扫到的实例，
/// 才会被标成 external —— 两者都能看能停，只是标签不同。
fn detached_marker(pid: u32) -> bool {
    std::fs::read(format!("/proc/{pid}/environ"))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .split('\0')
                .any(|kv| kv == "DSH_STARTER_DETACHED=1")
        })
        .unwrap_or(false)
}

/// 全平台进程枚举 (pid, cmdline 单行空格分隔)：Linux 读 /proc；macOS 用
/// `ps -axo pid=,command=`；Windows 用 PowerShell CIM（EncodedCommand 免引号转义）。
///
/// 这里有四条**不能退回**的约束（GitHub issue #1 的白屏就是踩了它们）：
///
/// 1. **绝不在 UI 线程上枚举**：Windows 上这要起 PowerShell（1s 级）、macOS 要起 `ps`，
///    放在 `setup` / 托盘创建这类主线程路径上，窗口能出现但一直白屏「未响应」
///    （WebView2 拿不到消息泵，渲染不出来）。
/// 2. **绝不逐 PID 起子进程**：早先 Windows 用 `tasklist /FI "PID eq <pid>"` 做存活校验，
///    按端口反向发现时**每个监听端口各起一个**，一轮 16～20 个，还会闪一屏黑色控制台。
/// 3. **拿不到 cmdline 的进程也要留在表里**（cmdline 记空串）：Windows 的
///    `Win32_Process.CommandLine` 对非本用户进程（svchost 等系统服务）是空的，
///    早先这些条目被整条丢掉，于是「端口占用者不在表里」→ 每个端口都回退一次逐 PID
///    探测，这正是 tasklist 风暴的直接来源；留在表里才能判定「占端口的是别的程序」。
/// 4. **枚举失败 ≠ 进程不存在**：`ok=false` 时调用方不得据此清掉实例登记（见 `check_pid`）。
struct ProcessSnapshot {
    at: Instant,
    /// 这份表**建立时的墙钟毫秒**（与 `DetachedRecord::started_at` 同一把钟）。
    /// 用途只有一个：判断「某进程是在这张表拍完之后才起来的」——那时表里查不到它
    /// 属于正常，不能据此说它已经死了（见 `snapshot_predates`）。
    taken_at_ms: u64,
    /// 进程状态已变化（停止 / 杀掉实例后置位）——下次读取需要重算
    stale: bool,
    /// 本轮是否拿到了完整进程表
    ok: bool,
    list: Arc<Vec<(u32, String)>>,
}

fn snapshot_cache() -> &'static Mutex<Option<ProcessSnapshot>> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Mutex<Option<ProcessSnapshot>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// 唤醒枚举守护线程的信号：停止实例后要求立刻重算，不必干等 TTL
fn watcher_poke() -> &'static Mutex<Option<std::sync::mpsc::Sender<()>>> {
    use std::sync::OnceLock;
    static POKE: OnceLock<Mutex<Option<std::sync::mpsc::Sender<()>>>> = OnceLock::new();
    POKE.get_or_init(|| Mutex::new(None))
}

/// 枚举超时：子进程卡死时杀掉它，而不是把调用线程永久占住
/// （守护线程一旦被占住，实例状态就再也不更新了）
#[cfg(any(target_os = "macos", windows))]
const ENUM_TIMEOUT: Duration = Duration::from_secs(6);

/// 快照 TTL。Windows 枚举要起 PowerShell，拉长一点减少后台开销；
/// Linux 读 /proc、macOS 起一次 `ps` 都很快，短一点让状态更实时。
fn snapshot_ttl() -> Duration {
    Duration::from_secs(if cfg!(windows) { 8 } else { 2 })
}

/// 读快照（纯内存，永不阻塞、永不起子进程）。None = 还没有任何一次枚举结果。
fn read_snapshot() -> Option<(bool, Arc<Vec<(u32, String)>>)> {
    snapshot_cache()
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.ok, s.list.clone()))
}

fn snapshot_is_fresh(s: &ProcessSnapshot) -> bool {
    !s.stale && s.at.elapsed() < snapshot_ttl()
}

/// 当前进程表快照的墙钟拍摄时刻（毫秒）；还没有任何快照时为 `None`。
fn snapshot_taken_at_ms() -> Option<u64> {
    snapshot_cache()
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.taken_at_ms)
}

/// 这份进程表是不是**早于**该时刻拍的（表里查不到那时还没起来的进程，属于正常）。
///
/// 为什么必须区分：Windows 上枚举一轮要起 PowerShell（1s 级），快照 TTL 8 秒。
/// 独立进程刚被拉起、前端立刻就轮询校验时，手里往往还是**拉起之前**那张表 ——
/// 查不到新 pid 便被当成「陈旧登记」清掉，而注册表里的 `log_file` 是实例终端
/// 读取独立进程日志的**唯一**来源：清掉之后实例日志文件还在、终端却永远读不到，
/// 界面上它还会降级成「不是启动器拉起的」。
/// 快照晚于进程启动（或一样早）时 `check_pid` 的判定才是可信的，照常清理。
fn snapshot_predates(started_at_ms: u64) -> bool {
    snapshot_taken_at_ms()
        .map(|taken| taken <= started_at_ms)
        .unwrap_or(true)
}

/// 后台线程：维护进程表快照，并保证**任何时刻只有一个枚举在跑**。
///
/// 早先每轮枚举由调用方各自触发，前端 3 秒轮询与托盘 3 秒轮询会一轮叠一轮
/// （issue #1 里 `tasklist.exe` 数量 16 → 18 → 20 持续增长就是这个），
/// 现在统一由这一个线程串起来；调用方只读快照，永不被枚举拖住。
/// 在 `setup` 里调用一次（Windows 下这里要起 PowerShell，所以必须是后台线程）。
pub fn start_process_watcher() {
    use std::sync::OnceLock;
    static STARTED: OnceLock<()> = OnceLock::new();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    if STARTED.set(()).is_err() {
        return; // 已经起过（重复调用是安全的）
    }
    *watcher_poke().lock().unwrap() = Some(tx);
    let _ = std::thread::Builder::new()
        .name("proc-watcher".into())
        .spawn(move || {
            let mut first = true;
            loop {
                let started = Instant::now();
                refresh_process_snapshot();
                let (ok, ms) = (
                    read_snapshot().map(|(ok, _)| ok).unwrap_or(false),
                    started.elapsed().as_millis(),
                );
                // 首轮与异常轮留痕：白屏/实例状态不更新这类问题的第一现场证据。
                // 正常轮走 debug，否则每 8 秒一行会把日志刷掉。
                let slow = started.elapsed() > Duration::from_secs(3);
                if first {
                    crate::diag::info("app", &format!("首轮进程枚举 {ms}ms ok={ok}"));
                    first = false;
                } else if !ok || slow {
                    crate::diag::warn(
                        "app",
                        &format!("进程枚举异常：{ms}ms ok={ok}（超时或 PowerShell/ps 不可用）"),
                    );
                } else {
                    crate::diag::debug("app", || format!("进程枚举 {ms}ms ok={ok}"));
                }
                match rx.recv_timeout(snapshot_ttl()) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
}

/// 强制重新枚举并写入快照（单飞：同一时刻只允许一个线程枚举，杜绝多轮叠加）。
pub fn refresh_process_snapshot() {
    use std::sync::OnceLock;
    static REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _g = REFRESH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let (ok, list) = enumerate_processes_uncached();
    *snapshot_cache().lock().unwrap() = Some(ProcessSnapshot {
        at: Instant::now(),
        taken_at_ms: now_millis(),
        stale: false,
        ok,
        list: Arc::new(list),
    });
}

/// 读进程表：快照新鲜就直接用，过期（或还没有）才在**当前线程**枚举一次。
///
/// ⚠️ 只允许后台线程 / `spawn_blocking` 调用 —— 在主线程上调用就等于把界面冻住
/// （Windows 一次枚举 1s 起）。托盘菜单、命令处理都已挪到后台线程，别在主线程加回来。
pub fn enumerate_processes() -> Arc<Vec<(u32, String)>> {
    {
        let guard = snapshot_cache().lock().unwrap();
        if let Some(s) = guard.as_ref() {
            if snapshot_is_fresh(s) {
                return s.list.clone();
            }
        }
    }
    refresh_process_snapshot();
    read_snapshot()
        .map(|(_, l)| l)
        .unwrap_or_else(|| Arc::new(Vec::new()))
}

/// 进程状态已变化（停止 / 杀掉实例后调用）：把快照标记为过期并立刻唤醒守护线程重算。
///
/// 非阻塞：调用方可能就在主线程上（`stop` 之后要 emit 事件）。
/// **不再**清空快照：读到的旧表配上接下来的重算，比「没有表」更好——
/// 没有表时 Windows/macOS 只能保守地「不判定」，实例会短暂显示成运行中。
pub fn invalidate_process_cache() {
    if let Some(s) = snapshot_cache().lock().unwrap().as_mut() {
        s.stale = true;
    }
    if let Some(tx) = watcher_poke().lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

/// 把一个已确认结束的 PID 从快照里摘掉：界面不必等下一轮枚举才不再显示「运行中」。
fn forget_pid(pid: u32) {
    if let Some(s) = snapshot_cache().lock().unwrap().as_mut() {
        Arc::make_mut(&mut s.list).retain(|(p, _)| *p != pid);
    }
}

/// 起一个**隐藏窗口**的子进程并收集 stdout，带超时（超时/kill 后返回 None）。
///
/// 两个必须：GUI 主程序起控制台子进程不隐藏会闪黑框（issue #1 里用户看到的一屏黑窗口）；
/// 没有超时的 `.output()` 在子进程卡死时会永久占住调用线程。
#[cfg(any(target_os = "macos", windows))]
fn run_capture_hidden(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut cmd = util::hidden_command(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.args(args).spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = std::io::BufReader::new(stdout).read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let mut finished = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                finished = true;
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let buf = reader.join().unwrap_or_default();
    if !finished {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// 真正跑一次平台枚举。返回 `(是否拿到完整进程表, [(pid, cmdline)])`。
/// cmdline 拿不到的进程以空串保留在表里（见上面约束 3）。
fn enumerate_processes_uncached() -> (bool, Vec<(u32, String)>) {
    #[cfg(target_os = "linux")]
    {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir("/proc") else {
            return (false, out);
        };
        for entry in rd.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            // 读不到 cmdline（内核线程、权限、刚好退出）也保留 PID：
            // 它能回答「这个端口不是 dsh 占的」，从而免掉逐端口探测
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
                .unwrap_or_default();
            out.push((pid, cmd));
        }
        (!out.is_empty(), out)
    }
    #[cfg(target_os = "macos")]
    {
        let mut out = Vec::new();
        let Some(text) = run_capture_hidden("ps", &["-axo", "pid=,command="], ENUM_TIMEOUT) else {
            return (false, out);
        };
        for line in text.lines() {
            let Some((pid, rest)) = line.trim_start().split_once(char::is_whitespace) else {
                continue;
            };
            let Ok(pid) = pid.trim().parse::<u32>() else {
                continue;
            };
            // 其它用户的进程 macOS 只给可执行文件路径、不给参数：同样保留 PID
            out.push((pid, rest.trim().to_string()));
        }
        (!out.is_empty(), out)
    }
    #[cfg(windows)]
    {
        let mut out = Vec::new();
        // 先把输出编码钉成 UTF-8：命令行里的中文（用户名 / 路径 / profile 名）不再
        // 按系统代码页变成乱码。用 try 包住 —— 万一某个 Windows 环境里 stdout 被重定向
        // 时不允许改 OutputEncoding，也只是退回旧行为，绝不能让整轮枚举失败。
        // CommandLine 为空（非本用户进程）时仍会输出 "pid<TAB>"，必须保留 —— 见约束 3。
        let script = "try { [Console]::OutputEncoding=[System.Text.Encoding]::UTF8 } catch {}; \
             Get-CimInstance Win32_Process | ForEach-Object { \"{0}`t{1}\" -f $_.ProcessId, $_.CommandLine }".to_string();
        let encoded = crate::util::ps_encoded_command(&script);
        let args = [
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            encoded.as_str(),
        ];
        let Some(text) = run_capture_hidden("powershell", &args, ENUM_TIMEOUT) else {
            return (false, out);
        };
        for line in text.lines() {
            let Some((pid, cmd)) = line.split_once('\t') else {
                continue;
            };
            let Ok(pid) = pid.trim().parse::<u32>() else {
                continue;
            };
            out.push((pid, cmd.trim().to_string()));
        }
        (!out.is_empty(), out)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        (false, Vec::new())
    }
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
    /// embedded = 启动器子进程；detached = 启动器派生的独立进程；
    /// external = 终端/外部启动；port = 按监听端口反向发现
    pub source: Option<String>,
    /// 内嵌实例对应的 dsh 版本（外部实例未知）
    pub version: Option<String>,
    /// web 实例的监听端口（按端口发现时提供，用于展示与区分同名实例）
    pub port: Option<u16>,
    /// 独立进程的日志文件路径（内嵌实例走日志管道；外部实例没有文件日志）
    pub log_file: Option<String>,
    /// 从实例日志里解析出的 dsh 访问地址（内嵌实例由前端从实时日志解析；
    /// 独立进程在这里由后端解析，外部实例没有日志可解）
    pub web_url: Option<String>,
    /// 启动时间（毫秒时间戳）；外部实例未知
    pub started_at: Option<u64>,
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
                port: None,
                log_file: None,
                web_url: None,
                started_at: Some(
                    h.started_at
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                ),
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
            port: None,
            web_url: web_url_from_log(&r.log_file),
            log_file: Some(r.log_file.clone()),
            started_at: Some(r.started_at),
        });
    }
    // 端口归属放在 cmdline 扫描**之前**：端子/独立实例只要占着某个 profile 配的 web
    // 端口就先认下来并把 PID 记入 claimed，这样后面 cmdline 扫描不会把同一实例
    // 再报一遍（终端 npx 起的 dsh cmdline 里没有 --profile，只能靠端口归属）。
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
        if let Some(pid) = pid {
            claimed.push(pid);
        }
        out.push(ProfileInstance {
            profile: p.name.clone(),
            running: true,
            pid,
            source: Some("port".into()),
            version: None,
            port: Some(port),
            log_file: None,
            web_url: None,
            started_at: None,
        });
    }
    // 通用端口反向发现：系统里任何 dsh 正在监听的端口都列出来——不要求该端口出现在
    // profile 配置里（终端 `dsh web --port 9999`、没写过快捷配置的 profile 都算），
    // 也不要求经过启动器（所以外部独立进程、启动器重启后的残留都能看到并停掉）。
    for (port, pid) in dsh_listening_ports() {
        if claimed.contains(&pid) {
            continue;
        }
        claimed.push(pid);
        // 能解析出 profile 就用它；解析不出（终端 `dsh web` 没带 --profile）就留空，
        // 前端按 `:端口` 展示，仍可按 PID 停止。
        let name = pid_cmdline(pid)
            .map(|cmd| parse_dsh_profile(&cmd))
            .unwrap_or_default();
        if !name.is_empty() && out.iter().any(|i| i.profile == name) {
            continue;
        }
        out.push(ProfileInstance {
            profile: name,
            running: true,
            pid: Some(pid),
            source: Some("port".into()),
            version: None,
            port: Some(port),
            log_file: None,
            web_url: None,
            started_at: None,
        });
    }
    // cmdline 外部扫描兜底：没有配 web 端口、也没监听端口的实例（desktop/headless）
    // 只能靠 cmdline 里的 --profile 认出来；已由端口归属或注册表认领的 PID 会跳过。
    for (pid, profile) in external_running_profile_pids(&claimed) {
        // 带 DSH_STARTER_DETACHED 标记的是启动器派生的独立进程，其余为终端/外部启动
        let source = if detached_marker(pid) { "detached" } else { "external" };
        out.push(ProfileInstance {
            profile,
            running: true,
            pid: Some(pid),
            source: Some(source.into()),
            version: None,
            port: None,
            log_file: None,
            web_url: None,
            started_at: None,
        });
    }
    out.sort_by(|a, b| a.profile.cmp(&b.profile));
    out
}

/// 系统里所有「dsh 进程正在监听的 TCP 端口」→ `(端口, PID)`。
///
/// 这是「按端口反向发现」的入口：不预设端口、不看启动器注册表，只要是 dsh 且在
/// 监听就列出来。dsh 启动器之外的终端/npx/独立进程因此同样能被发现与停止。
///
/// 归属**只**按进程表快照里的 cmdline 判定，不做逐 PID 探测：表是全量的（含拿不到
/// cmdline 的进程），查不到只说明它已退出或本轮枚举失败。早期为每个监听端口各起一个
/// `tasklist.exe` 的兜底，正是 issue #1 里 16～20 个 tasklist 叠加、弹黑框、
/// 把界面拖到「未响应」的根源。
pub fn dsh_listening_ports() -> Vec<(u16, u32)> {
    let procs = enumerate_processes(); // 带快照缓存，避免逐 PID 起进程探测
    let cmd_of = |pid: u32| -> Option<&str> {
        procs
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, cmd)| cmd.as_str())
    };
    crate::netports::listening_tcp()
        .into_iter()
        .filter(|(_, pid)| cmd_of(*pid).map(is_dsh_cmdline).unwrap_or(false))
        .collect()
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
#[derive(Debug)]
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
    crate::diag::debug("instance", || {
        format!("端口 {host}:{port} 有人在监听，判定归属（profile={profile:?}）")
    });
    let Some(pid) = crate::netports::listener_pid(port) else {
        return Some(PortOwner::Unknown);
    };
    // 「是不是 dsh」用与 cmdline 扫描同一套判据（`is_dsh_cmdline`），保证三端一致；
    // 快照里查不到这个 PID 时才退回 `pid_is_dsh`（它同样基于快照，不会再起子进程）。
    let cmd = pid_cmdline(pid);
    let is_dsh = match cmd.as_deref() {
        Some(cmd) => is_dsh_cmdline(cmd),
        None => pid_is_dsh(pid),
    };
    if !is_dsh {
        return Some(PortOwner::OtherProcess(pid));
    }
    // 拿不到 cmdline 时既证明不了它是 dsh、也判不出属于哪个 profile，
    // 绝不能保守归到「本 profile」——否则停本 profile 会杀掉任何恰好占着该端口的无关进程。
    // 这种情况一律 Unknown 交回「请手动处理」，检测路径仍会把它算作运行中（只是不给可停的 PID）。
    let Some(cmd) = cmd.as_deref() else {
        return Some(PortOwner::Unknown);
    };
    let owner = parse_dsh_profile(cmd);
    if owner.is_empty() || owner == profile {
        Some(PortOwner::ThisProfile(pid))
    } else {
        Some(PortOwner::OtherProfile { pid, profile: owner })
    }
}

/// 从实例日志里解析 dsh 的访问地址（`dsh web: <url>`）。
/// 与前端 App.tsx 解析实时日志用的是同一判据，保证独立进程与内嵌实例的「打开」一致。
fn parse_web_url(text: &str) -> Option<String> {
    let idx = text.find("dsh web:")?;
    let url = text[idx + "dsh web:".len()..].split_whitespace().next()?;
    if url.starts_with("http://") || url.starts_with("https://") {
        Some(url.trim_end_matches([',', ')', ']']).to_string())
    } else {
        None
    }
}

/// 从日志文件里取访问地址。`dsh web: <url>` 在启动早期打印，所以小文件整读、
/// 大文件只读「开头 + 结尾」各 64KB，避免每次状态轮询都整读长日志。
fn web_url_from_log(path: &str) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    const WINDOW: u64 = 64 * 1024;
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut text = String::new();
    if len <= WINDOW * 2 {
        f.read_to_string(&mut text).ok()?;
    } else {
        let mut head = vec![0u8; WINDOW as usize];
        let n = f.read(&mut head).ok()?;
        text.push_str(&String::from_utf8_lossy(&head[..n]));
        f.seek(SeekFrom::Start(len - WINDOW)).ok()?;
        let mut tail = Vec::new();
        f.read_to_end(&mut tail).ok()?;
        text.push_str(&String::from_utf8_lossy(&tail));
    }
    parse_web_url(&text)
}

/// 读取独立进程实例日志的尾部：`(日志路径, 内容, 是否被截断)`。
///
/// 只认注册表里登记的日志文件（不接受任意路径），因此外部实例/内嵌实例返回 `None`
/// —— 外部实例的日志在启动它的终端里，内嵌实例走日志管道。
pub fn read_instance_log_tail(
    pid: u32,
    max_bytes: usize,
) -> Result<Option<(String, String, bool)>, String> {
    let Some(rec) = validate_detached_registry()
        .into_iter()
        .find(|r| r.pid == pid)
    else {
        return Ok(None);
    };
    let (text, truncated) = read_tail(&rec.log_file, max_bytes)?;
    Ok(Some((rec.log_file, text, truncated)))
}

/// 读文件尾部（最多 max_bytes），截断时丢掉半行并加提示
fn read_tail(path: &str, max_bytes: usize) -> Result<(String, bool), String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).map_err(|e| format!("打开日志失败: {e}"))?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let max = max_bytes.max(4096) as u64;
    let truncated = len > max;
    if truncated {
        f.seek(SeekFrom::Start(len - max)).map_err(|e| e.to_string())?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).map_err(|e| format!("读取日志失败: {e}"))?;
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        if let Some(i) = text.find('\n') {
            text.drain(..=i);
        }
        text = format!("…（仅显示末尾 {} KB）\n{text}", max / 1024);
    }
    Ok((text, truncated))
}

/// 按 PID 结束一个**非内嵌**实例（启动器派生的独立进程 / 终端外部启动）。
///
/// 这是「任何被发现的实例都必须能关掉」的兜底：只要它出现在注册表里、或确认是
/// dsh 进程就结束；否则返回 `Ok(false)` 不动它（绝不误杀）。
pub fn stop_external_pid(pid: u32) -> Result<bool, String> {
    let in_registry = validate_detached_registry().iter().any(|r| r.pid == pid);
    if !in_registry && !pid_is_dsh(pid) {
        crate::diag::warn(
            "instance",
            &format!("拒绝停止 pid={pid}：既不在独立进程登记表，也没被确认是 dsh 进程"),
        );
        return Ok(false);
    }
    crate::diag::info(
        "instance",
        &format!("停止外部实例：pid={pid}（登记表命中={in_registry}）"),
    );
    force_kill_pid(pid)?;
    forget_pid(pid);
    if in_registry {
        remove_detached_record(pid);
    }
    invalidate_process_cache();
    Ok(true)
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
        crate::diag::info(
            "instance",
            &format!("按 profile 停止：profile={profile:?} 命中独立进程登记 pid={pid}"),
        );
        force_kill_pid(pid)?;
        forget_pid(pid);
        remove_detached_record(pid);
        invalidate_process_cache();
        return Ok(true);
    }
    // 3) 外部进程（跨平台强杀：unix SIGKILL / Windows taskkill）
    for (pid, prof) in external_running_profile_pids(&[]) {
        if prof == profile {
            crate::diag::info(
                "instance",
                &format!("按 profile 停止：profile={profile:?} 命中外部 dsh 进程 pid={pid}"),
            );
            force_kill_pid(pid)?;
            forget_pid(pid);
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
                forget_pid(pid);
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
    use std::process::Command;

    #[test]
    fn is_dsh_cmdline_matches_npm_shim() {
        // 终端 npx / npm exec / 全局安装起的 dsh：进程 argv 是 node_modules/.bin/dsh
        // 这种形态没有 @deepseek-ai/dsh 字样，必须靠 shim 路径认出来
        assert!(is_dsh_cmdline(
            "node /home/u/.npm/_npx/1e7f/node_modules/.bin/dsh web"
        ));
        assert!(is_dsh_cmdline(
            r"node C:\Users\u\AppData\Local\npm-cache\_npx\1e7f\node_modules\.bin\dsh web"
        ));
        // Windows 上 npm 生成的是 .cmd / .ps1 包装
        assert!(is_dsh_cmdline(
            r#"cmd /c "C:\Users\u\AppData\Roaming\npm\node_modules\.bin\dsh.cmd" web"#
        ));
        // 只应匹配 dsh 本身，不能把同前缀的其它 bin 也认进来
        assert!(!is_dsh_cmdline("node /home/u/node_modules/.bin/dsh-something"));
        assert!(!is_dsh_cmdline("node /home/u/node_modules/.bin/codex"));
    }

    #[test]
    fn is_dsh_cmdline_matches_both_separators() {
        assert!(is_dsh_cmdline(
            "node /home/u/.dsh-starter/versions/0.1.5/node_modules/@deepseek-ai/dsh/bin.js --profile web"
        ));
        // Windows 路径分隔符
        assert!(is_dsh_cmdline(
            "node C:\\Users\\u\\.dsh-starter\\versions\\0.1.5\\node_modules\\@deepseek-ai\\dsh\\bin.js --profile web"
        ));
        assert!(!is_dsh_cmdline("node /some/other/bin.js --profile web"));
        assert!(!is_dsh_cmdline("node /home/u/@deepseek-ai/dsh/other.js"));
    }

    #[test]
    fn detached_registry_roundtrip_and_stale_purge() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-detreg-{}", std::process::id()));
        // starter_home() 只认存在的目录（否则回落到 $HOME），受限环境里必须先建出来
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

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

        // 校验：不存在的 PID（u32::MAX / MAX-1 必然无进程）作为陈旧条目被清出并回写文件。
        // 存活判定依赖进程表快照，先按守护线程的方式刷一轮（进程表拿不到的环境只能保守保留）
        refresh_process_snapshot();
        let valid = validate_detached_registry();
        match read_snapshot() {
            Some((true, _)) => {
                assert!(valid.is_empty());
                assert!(read_detached_registry().is_empty());
            }
            // 平台枚举不可用（PowerShell 被禁用、超时）时必须保守保留，绝不能误删实例登记
            _ => assert_eq!(valid.len(), 2),
        }

        // remove_detached_record：只摘掉指定 PID，不动其它条目
        remove_detached_record(u32::MAX);
        assert!(!read_detached_registry().iter().any(|r| r.pid == u32::MAX));
    }

    /// Windows 回归：独立进程**刚拉起**时，存活校验手里往往还是「拉起之前」那张进程表
    /// （枚举一轮要 1 秒级、TTL 8 秒）。表里当然没有新 pid —— 不得据此当成陈旧登记清掉：
    /// 注册表里的 `log_file` 是实例终端读取独立进程日志的唯一来源，清掉之后就是
    /// 「日志文件里明明有内容，侧边实例终端却什么都读不到」，实例还会降级成「非内嵌」。
    #[test]
    fn fresh_detached_record_survives_snapshot_taken_before_it_started() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        // 表先拍一轮（守护线程的常态），进程"随后"才起来
        refresh_process_snapshot();
        let started_at = now_millis();
        let taken = snapshot_taken_at_ms().expect("刚刷过，快照应存在");
        assert!(started_at >= taken, "登记时刻不应早于快照拍摄时刻");
        // 表与登记同龄（或更早）→ 查不到不能作数；登记远早于表 → 表的判定才可信
        assert!(snapshot_predates(taken));
        assert!(!snapshot_predates(taken.saturating_sub(60_000)));

        // u32::MAX 必然不是本机进程：check_pid 一定回 NotDsh，唯一能救它的是「表比登记旧」
        let log_path = tmp.join("web-fresh.log");
        std::fs::write(&log_path, "booting…\ndsh web: http://127.0.0.1:39998/?token=t\n").unwrap();
        append_detached_record(&DetachedRecord {
            pid: u32::MAX,
            profile: "web-fresh".into(),
            version: "0.0.0".into(),
            started_at,
            log_file: log_path.to_string_lossy().into_owned(),
        })
        .unwrap();

        // 把快照钉在「登记之前」：消除并行跑测试时别的用例恰好刷新快照的时序抖动，
        // 让这一轮校验手里的表就是生产里那张「拉起之前」的表
        {
            let mut guard = snapshot_cache().lock().unwrap();
            let snap = guard.as_mut().expect("刚刷过，快照应存在");
            snap.taken_at_ms = started_at.saturating_sub(1);
        }

        let valid = validate_detached_registry();
        assert!(
            valid.iter().any(|r| r.pid == u32::MAX),
            "刚拉起、进程表还没重算的独立进程不能被当成陈旧登记清掉"
        );
        // 该实例的日志通道必须还在：实例终端就是按 PID 从这里读日志的
        let (_p, text, _cut) = read_instance_log_tail(u32::MAX, 4096)
            .unwrap()
            .expect("登记保留时实例终端应能读到独立进程日志");
        assert!(text.contains("dsh web: http://127.0.0.1:39998/"), "日志内容: {text:?}");

        // 真正重算过一轮、表已晚于启动时刻后仍查不到 → 这时才判定已死并清理
        std::thread::sleep(Duration::from_millis(20));
        refresh_process_snapshot();
        assert!(
            validate_detached_registry().is_empty(),
            "表比登记新之后，确认不存在的 PID 应被清掉"
        );
        assert!(read_instance_log_tail(u32::MAX, 4096).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("DSH_STARTER_HOME");
    }

    /// issue #1 回归：存活 / 端口归属判定必须走内存快照，**绝不能逐 PID 起子进程**。
    /// 早先 Windows 每个 PID 起一次 `tasklist`（≈200ms）、macOS 一次 `ps`，
    /// 600 次判定要跑几分钟，而且正是它把界面拖成「未响应」。
    #[test]
    fn pid_checks_do_not_spawn_processes() {
        refresh_process_snapshot();
        if !read_snapshot().map(|(ok, _)| ok).unwrap_or(false) {
            eprintln!("本环境进程枚举不可用，跳过");
            return;
        }
        let start = Instant::now();
        for pid in 0..300u32 {
            let _ = check_pid(pid);
            let _ = pid_is_dsh(pid);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "600 次存活判定耗时 {elapsed:?}：说明又退回逐 PID 起子进程了"
        );
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

    /// 端到端：**不是启动器启动的** dsh（伪造进程，cmdline 像 dsh 且自己监听端口）
    /// 必须能被「按端口反向发现」；启动器派生的独立进程必须能被注册表校验保留。
    #[cfg(unix)]
    #[test]
    fn discovers_external_dsh_by_port_and_registry() {
        use std::os::unix::fs::PermissionsExt;

        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        // 端口先用 bind(0) 占一个号再放掉，交给伪进程去 bind
        let port = {
            let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        // 路径同时含 @deepseek-ai/dsh 与 bin.js —— 满足 is_dsh_cmdline
        let fake = tmp.join("@deepseek-ai/dsh/lib/bin.js");
        std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
        std::fs::write(
            &fake,
            format!(
                r#"#!/usr/bin/env python3
import socket, time
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", {port}))
s.listen(5)
time.sleep(120)
"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut child = Command::new(&fake)
            .args(["--profile", "web-ext"])
            .spawn()
            .unwrap();
        let pid = child.id();

        let mut up = false;
        for _ in 0..100 {
            if crate::netports::is_listening("127.0.0.1", port) {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(up, "伪造的 dsh 进程未能在超时内监听端口");

        // 1) 认得出它是 dsh（cmdline 判据，与单实例守卫/端口归属同一套）。
        //    存活判定走进程表快照，先刷一轮（守护线程平时负责这件事）
        refresh_process_snapshot();
        assert!(pid_is_dsh(pid), "伪造进程应被认成 dsh");
        // 2) 端口 → PID
        assert_eq!(crate::netports::listener_pid(port), Some(pid));
        // 3) 反向发现：不依赖 profile 配置、不依赖注册表，也能扫到这个端口
        assert!(
            dsh_listening_ports().contains(&(port, pid)),
            "应按端口发现非启动器启动的 dsh"
        );
        // 4) 启动器派生的独立进程：写入注册表后必须被校验保留（而不是当陈旧条目清掉）
        let log_path = tmp.join("web-ext.log");
        std::fs::write(
            &log_path,
            "line1\ndsh web: http://127.0.0.1:39999/?token=abc (LAN: http://10.0.0.2:39999/?token=abc)\nline3\n",
        )
        .unwrap();
        append_detached_record(&DetachedRecord {
            pid,
            profile: "web-ext".into(),
            version: "0.0.0".into(),
            started_at: 1,
            log_file: log_path.to_string_lossy().into_owned(),
        })
        .unwrap();
        let valid = validate_detached_registry();
        assert!(
            valid.iter().any(|r| r.pid == pid),
            "detached 注册表应保留存活实例，否则界面看不到、也停不掉"
        );

        // 5) 独立进程日志通道：实例终端按 PID 读它的日志文件尾部
        let (path, text, truncated) = read_instance_log_tail(pid, 64 * 1024)
            .unwrap()
            .expect("注册表里的独立进程应能读到日志");
        assert_eq!(path, log_path.to_string_lossy());
        assert!(text.contains("line3") && !truncated, "应读到日志内容: {text:?}");
        // 独立进程的 web 地址由后端从日志解析（前端据此显示「打开」，与内嵌实例一致）
        assert_eq!(
            web_url_from_log(&path).as_deref(),
            Some("http://127.0.0.1:39999/?token=abc")
        );

        // 超长日志按 max_bytes 截断，并带上截断标记
        std::fs::write(&log_path, format!("{}\n尾部标记\n", "x".repeat(200_000))).unwrap();
        let (_p, tail, cut) = read_instance_log_tail(pid, 8192).unwrap().unwrap();
        assert!(cut && tail.len() < 20_000, "超长日志应被截断");
        assert!(tail.contains("尾部标记"), "截断后应保留末尾内容");

        // 6) 统一停止入口：按 PID 能结束它（外部实例路径）
        assert!(stop_external_pid(pid).unwrap());
        let _ = child.wait();
        // 注册表记录已被清掉 → 日志通道也随之失效（外部实例本来就没有文件日志）
        assert!(read_instance_log_tail(pid, 4096).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("DSH_STARTER_HOME");
    }

    #[test]
    fn parse_web_url_matches_frontend_rule() {
        // 真实日志行：取第一个 URL，忽略后面的 (LAN: …)
        let line = "dsh web: http://127.0.0.1:3081/?token=0tg2uteA (LAN: http://192.168.1.114:3081/?token=0tg2uteA)";
        assert_eq!(
            parse_web_url(line).as_deref(),
            Some("http://127.0.0.1:3081/?token=0tg2uteA")
        );
        // 与前端 /dsh web:\s*(https?:\/\/\S+)/ 对齐：只认 http(s)
        assert_eq!(parse_web_url("dsh web: not-a-url"), None);
        assert_eq!(parse_web_url("no marker here"), None);
        assert_eq!(
            parse_web_url("boot…\ndsh web:   https://a.b:1/?t=1\n"),
            Some("https://a.b:1/?t=1".to_string())
        );
    }

    /// 改名/删除 profile 时，独立进程登记表必须同步，否则实例列表会残留旧名幽灵条目
    #[test]
    fn detached_registry_follows_rename_and_delete() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-regsync-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        // 注意 pid 必须不同：append 会按 pid 去重覆盖
        let rec = |pid: u32, profile: &str| DetachedRecord {
            pid,
            profile: profile.into(),
            version: "0.0.0".into(),
            started_at: 1,
            log_file: "/tmp/x.log".into(),
        };
        append_detached_record(&rec(424242, "old-name")).unwrap();
        append_detached_record(&rec(424243, "keep")).unwrap();

        rename_detached_profile("old-name", "new-name");
        let after = read_detached_registry();
        assert!(
            after.iter().any(|r| r.profile == "new-name"),
            "改名应同步登记表"
        );
        assert!(!after.iter().any(|r| r.profile == "old-name"));
        assert!(after.iter().any(|r| r.profile == "keep"), "其它记录不受影响");

        drop_detached_profile("new-name");
        let after = read_detached_registry();
        assert!(!after.iter().any(|r| r.profile == "new-name"), "删除应清掉登记");
        assert!(after.iter().any(|r| r.profile == "keep"));

        let _ = std::fs::remove_dir_all(&tmp);
        std::env::remove_var("DSH_STARTER_HOME");
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

        // 端口探测是**真实 connect + 300ms 超时**：整套测试并发跑（尤其还有网络用例
        // 占着 CPU/网络栈）时，单次探测偶尔会超时，从而把"正在监听"误判成"空闲"。
        // 这里给它一个重试窗口，而不是拿一次结果下断言 —— 断言的是"最终能不能判对"。
        let settle = |want_listening: bool| -> Option<PortOwner> {
            let mut last = None;
            for _ in 0..20 {
                let got = port_owner("127.0.0.1", port, "web");
                if got.is_some() == want_listening {
                    return got;
                }
                last = got;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            last
        };

        match settle(true) {
            Some(PortOwner::OtherProcess(pid)) => assert_eq!(pid, std::process::id()),
            Some(PortOwner::ThisProfile(_)) | Some(PortOwner::OtherProfile { .. }) => {
                panic!("非 dsh 的监听者不应被认成某个 profile 的实例")
            }
            Some(PortOwner::Unknown) => eprintln!("本环境拿不到占用进程，跳过断言"),
            None => panic!("已 bind 的端口不应判定为空闲"),
        }
        drop(l);
        let after = settle(false);
        assert!(after.is_none(), "已释放的端口应判定为空闲（最后一次：{after:?}）");
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
