//! 插件管理任务运行器 —— 内置终端的数据源。
//!
//! 所有插件的安装 / 卸载 / 升级都由本模块统一执行，并**逐行实时**把输出推给前端：
//! - `dsh plugin --profile <p> add|remove|update …`（官方命令，pnpm 的薄转发器）
//! - `git clone` / `git pull`（clone+link 安装与升级的来源准备步骤）
//! - `pnpm install` / `pnpm run build`（仓库未提交构建产物时的可选构建步骤）
//!
//! 一个「任务」= 一串顺序步骤（Step），任一步失败即中止；每个任务有独立日志缓冲，
//! 前端可随时取消正在运行的任务，窗口重挂载后也能从快照恢复完整历史。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Runtime};

use crate::profile_cfg;
use crate::profiles;
use crate::settings::Settings;
use crate::util;

/// 日志行事件名（前端流式终端订阅）
pub const EVENT_LOG: &str = "plugin-log";
/// 任务状态事件名（开始 / 结束）
pub const EVENT_JOB: &str = "plugin-job";

/// 单个任务保留的最大日志行数（超出丢最旧的，并计数提示）
const MAX_LINES_PER_JOB: usize = 5000;
/// 快照回传前端的最大日志行数（避免一次 IPC 载荷过大）
const SNAPSHOT_LINES: usize = 1500;
/// 保留的历史任务数
const MAX_JOBS: usize = 24;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── 数据模型 ────────────────────────────────────────────────

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogLine {
    /// stdout | stderr | info
    pub stream: String,
    pub text: String,
    pub at: u64,
}

/// 一个插件管理任务（前端任务标签页 = 一条记录）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginJob {
    pub id: u64,
    pub profile: String,
    /// install | uninstall | upgrade | pull | clone | build | link
    pub kind: String,
    pub label: String,
    /// 首条命令的可读形式（多步任务只显示第一步，完整过程在日志里）
    pub command: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub running: bool,
    /// None = 运行中或已取消
    pub ok: Option<bool>,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    /// 失败时的对症建议（前端 toast / 终端提示都用它）
    pub hint: Option<String>,
    pub lines: Vec<PluginLogLine>,
    /// 因缓冲上限被丢弃的行数
    pub dropped: u64,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginJobEvent {
    pub job_id: u64,
    pub profile: String,
    pub kind: String,
    pub label: String,
    pub running: bool,
    pub ok: Option<bool>,
    pub exit_code: Option<i32>,
    pub cancelled: bool,
    /// 失败时的对症建议
    pub hint: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginLogEvent {
    pub job_id: u64,
    pub profile: String,
    pub stream: String,
    pub line: String,
}

/// 任务的一步
#[derive(Clone, Debug)]
pub enum Step {
    /// 外部命令
    Cmd {
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        label: String,
        /// 失败不中止后续步骤（如可选的构建）
        soft: bool,
    },
    /// 官方插件命令：dsh plugin --profile <p> <args…>
    Dsh { args: Vec<String> },
    /// 安装前预检远端可匿名访问（放任务里跑：点击立即有反馈，不必让 UI 等网络）
    ProbeRemote { url: String },
    /// GitHub 直装前的把关：目标包必须声明 dsh.bundle 且**已带 lib/ 构建产物**
    /// （GitHub 直装不构建，缺 lib 等于装个加载不了的东西；克隆安装会构建，是另一条路）
    ProbeGithubPackage { spec: String },
    /// 内部步骤：删除目录（克隆出的本地仓库）
    RmDir { path: PathBuf },
    /// 内部步骤：只写一行提示
    Note { text: String },
}

#[derive(Clone, Debug)]
pub struct JobRequest {
    pub profile: String,
    pub kind: String,
    pub label: String,
    pub steps: Vec<Step>,
}

// ── 任务状态 ────────────────────────────────────────────────

struct JobHandle {
    meta: Mutex<PluginJob>,
    lines: Mutex<Vec<PluginLogLine>>,
    dropped: AtomicU64,
    child: Mutex<Option<Child>>,
    cancelled: AtomicBool,
}

#[derive(Default, Clone)]
pub struct PluginJobState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    seq: u64,
    order: Vec<u64>,
    jobs: HashMap<u64, Arc<JobHandle>>,
}

impl PluginJobState {
    /// 同一 profile 是否已有运行中的任务（pnpm 并发写同一 profile 会损坏 node_modules）
    fn busy_profile(&self, profile: &str) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.order.iter().any(|id| {
            inner
                .jobs
                .get(id)
                .map(|j| {
                    let m = j.meta.lock().unwrap();
                    m.running && (m.profile == profile || m.profile.is_empty())
                })
                .unwrap_or(false)
        })
    }

    fn register(&self, profile: &str, kind: &str, label: &str, command: &str) -> (u64, Arc<JobHandle>) {
        let mut inner = self.inner.lock().unwrap();
        inner.seq += 1;
        let id = inner.seq;
        let handle = Arc::new(JobHandle {
            meta: Mutex::new(PluginJob {
                id,
                profile: profile.to_string(),
                kind: kind.to_string(),
                label: label.to_string(),
                command: command.to_string(),
                started_at: now_ms(),
                finished_at: None,
                running: true,
                ok: None,
                exit_code: None,
                cancelled: false,
                hint: None,
                lines: Vec::new(),
                dropped: 0,
            }),
            lines: Mutex::new(Vec::new()),
            dropped: AtomicU64::new(0),
            child: Mutex::new(None),
            cancelled: AtomicBool::new(false),
        });
        inner.jobs.insert(id, handle.clone());
        inner.order.push(id);
        // 超出上限：丢掉最旧的**已结束**任务
        while inner.order.len() > MAX_JOBS {
            let victim = inner
                .order
                .iter()
                .copied()
                .find(|i| inner.jobs.get(i).map(|j| !j.meta.lock().unwrap().running).unwrap_or(true));
            match victim {
                Some(v) => {
                    inner.order.retain(|i| *i != v);
                    inner.jobs.remove(&v);
                }
                None => break,
            }
        }
        (id, handle)
    }

    /// 任务列表（新→旧），带日志尾部
    pub fn snapshot(&self) -> Vec<PluginJob> {
        let inner = self.inner.lock().unwrap();
        let mut out: Vec<PluginJob> = inner
            .order
            .iter()
            .rev()
            .filter_map(|id| inner.jobs.get(id))
            .map(|h| {
                let mut job = h.meta.lock().unwrap().clone();
                let lines = h.lines.lock().unwrap();
                let start = lines.len().saturating_sub(SNAPSHOT_LINES);
                job.lines = lines[start..].to_vec();
                job.dropped = h.dropped.load(Ordering::Relaxed) + start as u64;
                job
            })
            .collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }

    /// 取某个任务的完整日志文本（导出用；包含被快照截断的头部）
    pub fn job_log_text(&self, id: u64) -> Option<String> {
        let handle = { self.inner.lock().unwrap().jobs.get(&id).cloned() }?;
        let meta = handle.meta.lock().unwrap().clone();
        let lines = handle.lines.lock().unwrap();
        let mut out = String::new();
        out.push_str(&format!(
            "# {} · profile={} · kind={}\n# $ {}\n",
            meta.label, meta.profile, meta.kind, meta.command
        ));
        for l in lines.iter() {
            out.push_str(&l.text);
            out.push('\n');
        }
        out.push_str(&format!(
            "\n# 退出状态: {}\n",
            match (meta.running, meta.ok) {
                (true, _) => "运行中".to_string(),
                (_, Some(true)) => "成功".to_string(),
                (_, Some(false)) if meta.cancelled => "已取消".to_string(),
                (_, Some(false)) => format!("失败（退出码 {:?}）", meta.exit_code),
                _ => "未知".to_string(),
            }
        ));
        Some(out)
    }
    /// 取消运行中的任务（kill 进程）
    pub fn cancel(&self, id: u64) -> bool {
        let handle = { self.inner.lock().unwrap().jobs.get(&id).cloned() };
        let Some(handle) = handle else { return false };
        if !handle.meta.lock().unwrap().running {
            return false;
        }
        handle.cancelled.store(true, Ordering::SeqCst);
        if let Some(mut child) = handle.child.lock().unwrap().take() {
            kill_tree(&mut child);
        }
        true
    }

    /// 清理已结束的任务，返回清理条数
    pub fn clear_finished(&self) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.order.len();
        let inner = &mut *inner;
        let keep: Vec<u64> = inner
            .order
            .iter()
            .copied()
            .filter(|id| {
                inner
                    .jobs
                    .get(id)
                    .map(|j| j.meta.lock().unwrap().running)
                    .unwrap_or(false)
            })
            .collect();
        inner.order = keep;
        let live = inner.order.clone();
        inner.jobs.retain(|id, _| live.contains(id));
        before - inner.order.len()
    }

    fn finish(&self, id: u64, ok: Option<bool>, exit_code: Option<i32>, cancelled: bool) {
        let handle = { self.inner.lock().unwrap().jobs.get(&id).cloned() };
        if let Some(h) = handle {
            let mut m = h.meta.lock().unwrap();
            m.running = false;
            m.ok = ok;
            m.exit_code = exit_code;
            m.cancelled = cancelled;
            m.finished_at = Some(now_ms());
        }
    }
}

// ── 事件推送 ────────────────────────────────────────────────

/// 记下第一条对症建议（任务结束事件会带上，前端 toast 直接显示）
fn remember_hint(handle: &Arc<JobHandle>, hint: &str) {
    let mut m = handle.meta.lock().unwrap();
    if m.hint.is_none() {
        m.hint = Some(hint.to_string());
    }
}

fn emit_line<R: Runtime>(app: &AppHandle<R>, handle: &Arc<JobHandle>, id: u64, stream: &str, text: &str) {
    let line = PluginLogLine {
        stream: stream.to_string(),
        text: text.to_string(),
        at: now_ms(),
    };
    {
        let mut buf = handle.lines.lock().unwrap();
        buf.push(line);
        let len = buf.len();
        if len > MAX_LINES_PER_JOB {
            buf.drain(0..len - MAX_LINES_PER_JOB);
            handle.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    let profile = handle.meta.lock().unwrap().profile.clone();
    let _ = app.emit(
        EVENT_LOG,
        PluginLogEvent {
            job_id: id,
            profile,
            stream: stream.to_string(),
            line: text.to_string(),
        },
    );
}

fn emit_job<R: Runtime>(app: &AppHandle<R>, handle: &Arc<JobHandle>) {
    let m = handle.meta.lock().unwrap();
    let _ = app.emit(
        EVENT_JOB,
        PluginJobEvent {
            job_id: m.id,
            profile: m.profile.clone(),
            kind: m.kind.clone(),
            label: m.label.clone(),
            running: m.running,
            ok: m.ok,
            exit_code: m.exit_code,
            cancelled: m.cancelled,
            hint: m.hint.clone(),
            started_at: m.started_at,
            finished_at: m.finished_at,
        },
    );
}

/// 结束子进程**及其整个进程组**。
///
/// pnpm / node / sh 都会派生子进程（构建、sleep 等），只 kill 直接子进程时，
/// 孙进程仍持有 stdout/stderr 管道，读取线程不会返回，任务看起来「卡住不结束」。
/// 因此 spawn 时把子进程设为新进程组组长（unix），取消时按组下发 SIGKILL。
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // 负 pid = 发往整个进程组；失败再退回只杀直接子进程
        let killed = unsafe { libc::kill(-pid, libc::SIGKILL) == 0 };
        if !killed {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        let pid = child.id().to_string();
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.kill();
    }
    let _ = child.wait();
}

// ── 执行 ────────────────────────────────────────────────────

/// 启动一个插件任务（不阻塞调用方），返回任务 id
pub fn start_job<R: Runtime>(
    app: AppHandle<R>,
    state: &PluginJobState,
    settings: Settings,
    req: JobRequest,
) -> Result<u64, String> {
    if req.profile.trim().is_empty() {
        return Err("缺少 profile".into());
    }
    if state.busy_profile(&req.profile) {
        return Err(format!(
            "profile「{}」已有插件任务正在运行，请等待完成或在终端里取消",
            req.profile
        ));
    }
    let command = describe_first_step(&req.steps);
    let (id, handle) = state.register(&req.profile, &req.kind, &req.label, &command);
    emit_job(&app, &handle);

    let state2 = state.clone();
    std::thread::spawn(move || {
        let mut ok = true;
        let mut cancelled = false;
        let mut exit_code: Option<i32> = None;
        // 变更前的 profile 清单快照（安装后校验 / 卸载后对账用）
        let mut pre_snapshot: Option<crate::verify::ProfileSnapshot> = None;

        for step in &req.steps {
            if handle.cancelled.load(Ordering::SeqCst) {
                cancelled = true;
                ok = false;
                break;
            }
            match step {
                Step::Note { text } => emit_line(&app, &handle, id, "info", text),
                Step::ProbeRemote { url } => {
                    emit_line(&app, &handle, id, "info", &format!("$ 预检远端可访问性 {url}"));
                    // 任务线程是普通 std::thread：这里用 block_on 跑只读探测，
                    // 不占用 UI/运行时线程；探测本身 0.5s 级（自实现 ref 发现）
                    match tauri::async_runtime::block_on(crate::registry::probe_remote_access(url, None)) {
                        crate::registry::RepoAccess::Public => {
                            emit_line(&app, &handle, id, "info", "✔ 远端可匿名访问");
                        }
                        crate::registry::RepoAccess::NotPublic => {
                            let spec = crate::registry::parse_github_spec(url);
                            let msg = spec
                                .map(|sp| crate::registry::private_repo_reject(&sp.owner, &sp.repo))
                                .unwrap_or_else(|| crate::plugin::PRIVATE_REPO_REJECT.to_string());
                            emit_line(&app, &handle, id, "stderr", &msg);
                            remember_hint(&handle, &msg);
                            ok = false;
                            break;
                        }
                        crate::registry::RepoAccess::Unknown(reason) => {
                            emit_line(
                                &app,
                                &handle,
                                id,
                                "info",
                                &format!(
                                    "⚠ 未能预检远端（{reason}）：直接尝试安装，失败原因见下方输出"
                                ),
                            );
                        }
                    }
                }
                Step::ProbeGithubPackage { spec } => {
                    let parsed = crate::registry::parse_github_spec(spec);
                    match parsed {
                        // 打包产物直链没有仓库树可查：跳过（装后校验会兜底）
                        None => {}
                        Some(sp) if sp.tarball_url.is_some() => {
                            emit_line(
                                &app,
                                &handle,
                                id,
                                "info",
                                "（打包产物直链：无法预先核对 lib/，装后校验会兜底）",
                            );
                        }
                        Some(sp) => {
                            emit_line(
                                &app,
                                &handle,
                                id,
                                "info",
                                &format!(
                                    "$ 核对 {} {} 的 dsh.bundle 与 lib/ 构建产物",
                                    sp.owner, sp.repo
                                ),
                            );
                            let token = crate::registry::github_token(Some(&settings.github_token));
                            // 任务线程是普通 std::thread，这里用 block_on 跑只读探测
                            match tauri::async_runtime::block_on(crate::registry::fetch_github_repo(
                                spec,
                                token.as_deref(),
                            )) {
                                Ok(info) => {
                                    match crate::registry::preflight_direct_install(
                                        &info.candidates,
                                        sp.plugin_path.as_deref(),
                                    ) {
                                        Ok(detail) => {
                                            emit_line(&app, &handle, id, "info", &format!("✔ {detail}"));
                                        }
                                        Err(reason) => {
                                            emit_line(&app, &handle, id, "stderr", &reason);
                                            remember_hint(&handle, &reason);
                                            ok = false;
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    // 探测失败不该挡住安装（可能只是元数据/额度问题）：
                                    // 装后校验仍然会拦下真正加载不了的包
                                    emit_line(
                                        &app,
                                        &handle,
                                        id,
                                        "info",
                                        &format!("⚠ 无法预先核对（{e}）：直接安装，装后校验会兜底"),
                                    );
                                }
                            }
                        }
                    }
                }
                Step::RmDir { path } => {
                    emit_line(&app, &handle, id, "info", &format!("$ rm -rf {}", path.display()));
                    match remove_clone_path(path) {
                        Ok(()) => emit_line(&app, &handle, id, "info", "已删除本地克隆目录"),
                        Err(e) => {
                            emit_line(&app, &handle, id, "stderr", &e);
                            ok = false;
                            break;
                        }
                    }
                }
                Step::Cmd { program, args, cwd, label, soft } => {
                    emit_line(&app, &handle, id, "info", &format!("$ {label}"));
                    match run_streamed(&app, &handle, id, program, args, cwd.as_deref(), &[], &settings) {
                        Ok((code, out)) => {
                            exit_code = Some(code);
                            if code != 0 {
                                if let Some(hint) = failure_hint(program, &out, None) {
                                    remember_hint(&handle, &hint);
                                    for l in hint.lines() {
                                        emit_line(&app, &handle, id, "info", l);
                                    }
                                }
                                if *soft {
                                    emit_line(
                                        &app,
                                        &handle,
                                        id,
                                        "info",
                                        &format!("⚠ 步骤退出码 {code}（可忽略，继续后续步骤）"),
                                    );
                                } else {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            emit_line(&app, &handle, id, "stderr", &e);
                            if program == "git" && looks_like_auth_error(&e) {
                                emit_line(&app, &handle, id, "info", &auth_hint());
                            }
                            if !*soft {
                                ok = false;
                                break;
                            }
                        }
                    }
                }
                Step::Dsh { args } => {
                    let profile = handle.meta.lock().unwrap().profile.clone();
                    if let Err(e) = validate_profile_name(&profile) {
                        emit_line(&app, &handle, id, "stderr", &e);
                        ok = false;
                        break;
                    }
                    let dir = profiles::profiles_dir().join(&profile);
                    let (node, bin_js) = match profile_cfg::resolve_dsh_bin(&settings) {
                        Ok(v) => v,
                        Err(e) => {
                            emit_line(&app, &handle, id, "stderr", &e);
                            ok = false;
                            break;
                        }
                    };
                    if !dir.is_dir() {
                        // profile 骨架交给 dsh 自己初始化（它按 profile 名挑模板），
                        // 这里只保证工作目录存在
                        if let Err(e) = std::fs::create_dir_all(&dir) {
                            let msg = format!("创建 profile 目录失败（{}）: {e}", dir.display());
                            emit_line(&app, &handle, id, "stderr", &msg);
                            ok = false;
                            break;
                        }
                        emit_line(
                            &app,
                            &handle,
                            id,
                            "info",
                            &format!(
                                "profile 目录不存在，已创建 {}（首次使用会自动初始化骨架）",
                                dir.display()
                            ),
                        );
                    }
                    let prog = node.to_string_lossy().into_owned();
                    let dsh_home = profiles::dsh_native_home().to_string_lossy().into_owned();

                    // 唯一入口：`dsh plugin --profile <p> <pnpm 参数…>`。
                    // dsh plugin 本身就是 pnpm 的薄转发器，所以「注入 -w」「加一次性覆盖参数」
                    // 「重建 node_modules」都只是给它换参数，绝不绕开它直接调 pnpm。
                    let exec = |argv: &[String]| -> Result<(i32, String), String> {
                        let mut full: Vec<String> = Vec::with_capacity(argv.len() + 4);
                        full.push(bin_js.to_string_lossy().into_owned());
                        full.push("plugin".into());
                        full.push("--profile".into());
                        full.push(profile.clone());
                        full.extend(argv.iter().cloned());
                        emit_line(
                            &app,
                            &handle,
                            id,
                            "info",
                            &format!("$ dsh plugin --profile {profile} {}", argv.join(" ")),
                        );
                        let mut envs = vec![
                            ("DSH_HOME".to_string(), dsh_home.clone()),
                            // pnpm 10+ 在没有 TTY 时会卡在静默交互提示上；CI 模式让它直接干活或报错
                            ("CI".to_string(), "true".to_string()),
                        ];
                        // 每个 --config.<key> 再以 PNPM_CONFIG_<KEY> 给一遍（pnpm 12 会忽略部分 CLI 覆盖）
                        envs.extend(crate::pnpm::config_env_for(argv));
                        run_streamed(&app, &handle, id, &prog, &full, Some(&dir), &envs, &settings)
                    };

                    let base_argv = crate::pnpm::plugin_args_for(&dir, args);
                    let before = pre_snapshot
                        .get_or_insert_with(|| crate::verify::snapshot(&dir))
                        .clone();

                    // 卸载前检查（对齐 dshmarket 的 uninstall 路由）：
                    // 用户自己的 cordis.patch.yml 若仍 insert 着这个包，卸载会让下次启动缺模块；
                    // 启动器不替用户改他的补丁文件，所以这里直接拒绝并指出要删哪几行。
                    if base_argv.first().map(String::as_str) == Some("remove") {
                        for name in base_argv
                            .iter()
                            .skip(1)
                            .filter(|a| !a.starts_with('-') && *a != "-w")
                        {
                            let pkg_dir = dir.join("node_modules").join(name);
                            let ids = crate::verify::declared_ids(&pkg_dir);
                            match crate::verify::user_patch_references(&dir, name, &ids) {
                                Some(hits) if !hits.is_empty() => {
                                    let msg = format!(
                                        "无法卸载 {name}：profile 的 cordis.patch.yml 仍通过补丁引用 {}。\n                                         请先在自己的补丁文件里删掉这些引用（启动器不会替你改写用户补丁），然后重试。",
                                        hits.join("、")
                                    );
                                    emit_line(&app, &handle, id, "stderr", &msg);
                                    remember_hint(&handle, &msg);
                                    ok = false;
                                    break;
                                }
                                None => {
                                    emit_line(
                                        &app,
                                        &handle,
                                        id,
                                        "info",
                                        "⚠ cordis.patch.yml 无法解析为补丁列表，无法排除它仍在引用这个包；继续卸载，若下次启动报缺模块请检查该文件",
                                    );
                                }
                                _ => {}
                            }
                            if crate::verify::holds_native_addon(&dir, name) {
                                emit_line(
                                    &app,
                                    &handle,
                                    id,
                                    "info",
                                    &format!(
                                        "⚠ {name} 带原生模块（.node）：Node 在进程退出前不会释放它，卸载后需要重启 dsh 才能重新安装（Windows 上尤其如此）"
                                    ),
                                );
                            }
                        }
                    }

                    match exec(&base_argv) {
                        Err(e) => {
                            emit_line(&app, &handle, id, "stderr", &e);
                            ok = false;
                            break;
                        }
                        Ok((mut code, mut out)) => {
                            // 失败时按 pnpm 的真实报错**一次性重试**（对齐 dshmarket 的 withHoistRecovery）
                            if code != 0 {
                                if let Some(failure) = crate::pnpm::classify(&out, Some(code)) {
                                    let first = failure
                                        .message(&dir, &profile)
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                        .to_string();
                                    if failure.needs_relink() {
                                        emit_line(
                                            &app,
                                            &handle,
                                            id,
                                            "info",
                                            &format!(
                                                "⚠ {first}（{}）—— 先重建 node_modules 再重试一次",
                                                failure.code()
                                            ),
                                        );
                                        // 重建同样走 dsh plugin；带上一次性放行，否则策略挡住时重建也会失败
                                        match exec(&[
                                            "install".to_string(),
                                            "--no-frozen-lockfile".to_string(),
                                            crate::pnpm::RELEASE_AGE_OVERRIDE.to_string(),
                                        ]) {
                                            Ok((c2, _)) if c2 == 0 => {}
                                            Ok(_) => emit_line(
                                                &app,
                                                &handle,
                                                id,
                                                "info",
                                                "⚠ 重建未成功，继续尝试原命令",
                                            ),
                                            Err(e) => emit_line(&app, &handle, id, "stderr", &e),
                                        }
                                        if let Ok((c3, o3)) = exec(&base_argv) {
                                            code = c3;
                                            out = o3;
                                        }
                                    } else if let Some(over) = failure.retry_override().filter(|_| {
                                        // 宿主 peer 的重试只在「确实不是本 profile 直接依赖」时做
                                        // （参考 dshmarket 的 isUnpublishedHostPeer 判断）
                                        match &failure {
                                            crate::pnpm::PnpmFailure::HostPeer { pkg } => pkg
                                                .as_deref()
                                                .map(|p| {
                                                    crate::pnpm::is_unpublished_host_peer(p, &dir)
                                                })
                                                .unwrap_or(false),
                                            _ => true,
                                        }
                                    }) {
                                        let mut retry: Vec<String> = vec![base_argv[0].clone()];
                                        if !over.is_empty() {
                                            retry.push(over.to_string());
                                        }
                                        retry.extend(base_argv.iter().skip(1).cloned());
                                        emit_line(
                                            &app,
                                            &handle,
                                            id,
                                            "info",
                                            &format!(
                                                "⚠ {first}（{}）—— 已自动放行重试一次{}",
                                                failure.code(),
                                                if over.is_empty() {
                                                    String::new()
                                                } else {
                                                    format!("（{over}）")
                                                }
                                            ),
                                        );
                                        match exec(&retry) {
                                            Ok((c3, o3)) => {
                                                code = c3;
                                                out = o3;
                                            }
                                            Err(e) => emit_line(&app, &handle, id, "stderr", &e),
                                        }
                                    }
                                }
                            }

                            if code == 0 {
                                // 安装/卸载成功后的核对与修复（对齐 dshmarket：
                                // validateAddedPlugins 的假成功防护 + removeAndReconcile 的清单对账）
                                let kind = handle.meta.lock().unwrap().kind.clone();
                                let checks = crate::verify::post_mutation_checks(
                                    &dir,
                                    &before,
                                    &base_argv,
                                    &kind,
                                );
                                for l in &checks.logs {
                                    emit_line(&app, &handle, id, "info", l);
                                }
                                for f in &checks.follow_ups {
                                    match exec(&["remove".to_string(), f.name.clone()]) {
                                        Ok((c, _)) if c == 0 => {}
                                        Ok((c, o)) => emit_line(
                                            &app,
                                            &handle,
                                            id,
                                            "stderr",
                                            &format!("✘ 后续卸载 {} 失败（退出码 {c}）：{}", f.name, o.lines().next().unwrap_or("")),
                                        ),
                                        Err(e) => emit_line(&app, &handle, id, "stderr", &e),
                                    }
                                    if f.reason == "manifest-residue"
                                        || !crate::verify::installed_on_disk(&dir, &f.name)
                                    {
                                        match crate::verify::drop_from_manifest(&dir, &f.name) {
                                            Ok(true) => emit_line(
                                                &app,
                                                &handle,
                                                id,
                                                "info",
                                                &format!(
                                                    "✔ 已按磁盘事实删除 {} 在 package.json 里的依赖/bundle 残留行（原件备份为 package.json.launcher-bak）",
                                                    f.name
                                                ),
                                            ),
                                            Ok(false) => {}
                                            Err(e) => emit_line(&app, &handle, id, "stderr", &e),
                                        }
                                    }
                                }
                                if !checks.report.removed_broken.is_empty()
                                    || !checks.report.conflicts.is_empty()
                                {
                                    let msg = format!(
                                        "本次安装有 {} 个包不合格已被卸掉（原因见内置终端）：{}",
                                        checks.report.removed_broken.len(),
                                        checks
                                            .report
                                            .removed_broken
                                            .iter()
                                            .map(|(n, r)| format!("{n}（{r}）"))
                                            .collect::<Vec<_>>()
                                            .join("、")
                                    );
                                    remember_hint(&handle, &msg);
                                } else if !checks.report.stale_updates.is_empty() {
                                    let msg = format!(
                                        "升级后版本没变：{}（pnpm 可能因 minimumReleaseAge 保留了旧版本）",
                                        checks.report.stale_updates.join("、")
                                    );
                                    remember_hint(&handle, &msg);
                                } else if !checks.report.manifest_repaired.is_empty() {
                                    remember_hint(
                                        &handle,
                                        &format!(
                                            "已按磁盘事实清理清单残留行：{}",
                                            checks.report.manifest_repaired.join("、")
                                        ),
                                    );
                                }
                                pre_snapshot = Some(crate::verify::snapshot(&dir));
                            }
                            exit_code = Some(code);
                            if code != 0 {
                                ok = false;
                                // 重试后仍失败：给出分类后的原因与下一步（而不是 pnpm 的原始输出墙）
                                match crate::pnpm::classify(&out, Some(code)) {
                                    Some(f) => {
                                        let msg = f.message(&dir, &profile);
                                        remember_hint(&handle, &msg);
                                        for l in msg.lines() {
                                            emit_line(&app, &handle, id, "info", l);
                                        }
                                    }
                                    None => {
                                        if let Some(hint) =
                                            failure_hint("dsh plugin", &out, Some(&dir))
                                        {
                                            remember_hint(&handle, &hint);
                                            for l in hint.lines() {
                                                emit_line(&app, &handle, id, "info", l);
                                            }
                                        }
                                    }
                                }
                                // dsh 对任何 github: 规格的失败都会打印 allowBuilds 提示，
                                // 真实原因不是构建脚本时明确纠正一下，别让人白改配置
                                if !out.to_lowercase().contains("allowbuilds")
                                    && out.contains("dsh: git-hosted plugins build on install")
                                {
                                    emit_line(
                                        &app,
                                        &handle,
                                        id,
                                        "info",
                                        "（上面那句 allowBuilds 提示是 dsh 对 github: 规格的通用兜底，本次失败原因以上面分类的结论为准）",
                                    );
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        if handle.cancelled.load(Ordering::SeqCst) {
            cancelled = true;
            ok = false;
        }
        emit_line(
            &app,
            &handle,
            id,
            "info",
            if cancelled {
                "■ 已取消"
            } else if ok {
                "✔ 任务完成"
            } else {
                "✘ 任务失败"
            },
        );
        state2.finish(id, Some(ok), exit_code, cancelled);
        emit_job(&app, &handle);
    });

    Ok(id)
}

/// 首条命令的可读形式（用于任务列表副标题）
fn describe_first_step(steps: &[Step]) -> String {
    for s in steps {
        match s {
            Step::Cmd { label, .. } => return label.clone(),
            Step::Dsh { args } => return format!("dsh plugin {}", args.join(" ")),
            Step::RmDir { path } => return format!("rm -rf {}", path.display()),
            Step::Note { .. }
            | Step::ProbeRemote { .. }
            | Step::ProbeGithubPackage { .. } => continue,
        }
    }
    String::new()
}

/// 启动外部进程并逐行流式推送输出；返回退出码
/// 返回值：`(退出码, 合并输出尾部)`——尾部用于把失败翻译成对症建议
fn run_streamed<R: Runtime>(
    app: &AppHandle<R>,
    handle: &Arc<JobHandle>,
    id: u64,
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    envs: &[(String, String)],
    settings: &Settings,
) -> Result<(i32, String), String> {
    use std::process::Command;
    let prog_path = PathBuf::from(program);
    let mut cmd: Command = util::spawn_command(&prog_path, args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    // 保留 pnpm 的真实过程输出（不设 CI / NO_COLOR 之类的静默开关）
    cmd.env("npm_config_update_notifier", "false");
    // git（含 pnpm 内部调用 git）一律不要弹出账号密码提示：宁可失败，也要把原因写进日志
    for (k, v) in git_no_prompt_env() {
        cmd.env(k, v);
    }
    util::bind_to_parent_lifetime(&mut cmd);
    util::with_node_on_path(&mut cmd, util::find_node(settings).as_deref());
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // 自成进程组：取消时能一次带走 pnpm/node 派生出的全部孙进程
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 `{program}` 失败: {e}（请确认命令在 PATH 中）"))?;
    // 合并输出的尾部：失败时用来识别真实原因（供应链策略 / 构建脚本 / 鉴权 / 404…）
    let tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(app.clone(), handle.clone(), id, out, "stdout", tail.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(app.clone(), handle.clone(), id, err, "stderr", tail.clone()));
    }
    *handle.child.lock().unwrap() = Some(child);

    // 与 installer.rs 相同：轮询 try_wait，便于取消时把 child 取走 kill
    let status = loop {
        {
            let mut guard = handle.child.lock().unwrap();
            match guard.as_mut() {
                Some(c) => match c.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => {}
                    Err(e) => {
                        *guard = None;
                        for r in readers {
                            let _ = r.join();
                        }
                        return Err(format!("等待进程失败: {e}"));
                    }
                },
                None => break None,
            }
        }
        if handle.cancelled.load(Ordering::SeqCst) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(150));
    };

    for r in readers {
        let _ = r.join();
    }
    let collected = tail.lock().unwrap().join("\n");
    match status {
        Some(st) => Ok((
            st.code().unwrap_or(if st.success() { 0 } else { 1 }),
            collected,
        )),
        None => {
            if handle.cancelled.load(Ordering::SeqCst) {
                Err("已取消".into())
            } else {
                Ok((1, collected))
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_reader<R: Runtime, P: std::io::Read + Send + 'static>(
    app: AppHandle<R>,
    handle: Arc<JobHandle>,
    id: u64,
    pipe: P,
    stream: &'static str,
    tail: Arc<Mutex<Vec<String>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines() {
            let Ok(l) = line else { break };
            // 进度条用 \r 原地重绘：按 \r 拆行，前端才不会糊成一大段
            for seg in l.split('\r') {
                let seg = strip_ansi(seg.trim_end());
                if seg.is_empty() {
                    continue;
                }
                {
                    let mut t = tail.lock().unwrap();
                    t.push(seg.clone());
                    let len = t.len();
                    if len > 60 {
                        t.drain(0..len - 60);
                    }
                }
                emit_line(&app, &handle, id, stream, &seg);
            }
        }
    })
}

/// 后端先剥掉 ANSI 转义（前端终端按纯文本渲染，逐行等宽显示）
fn strip_ansi(s: &str) -> String {
    if !s.contains('\u{1b}') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // CSI 序列: ESC [ … 终止于 @-~
        if chars.peek() == Some(&'[') {
            chars.next();
            for c2 in chars.by_ref() {
                if ('@'..='~').contains(&c2) {
                    break;
                }
            }
        } else {
            let _ = chars.next();
        }
    }
    out
}

// ── 安装 / 升级步骤编排 ─────────────────────────────────────

/// clone + link 安装（或对已存在的克隆执行更新）的输入
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneInstallInput {
    /// git 远端（https 或 git@ 形式）
    pub url: String,
    pub git_ref: Option<String>,
    /// 插件相对仓库根的路径（monorepo 子包）
    pub sub_path: Option<String>,
    /// 安装依赖并构建（仓库未提交 lib/ 时需要）
    pub build: bool,
}

/// 由远端 url 推导克隆目录名（优先解析 owner/repo）
pub fn clone_dir_for_url(url: &str) -> String {
    if let Some(spec) = crate::registry::parse_github_spec(url) {
        if !spec.owner.is_empty() && !spec.repo.is_empty() {
            return clone_dir_name(&spec.owner, &spec.repo);
        }
    }
    let tail = url
        .trim()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("plugin")
        .trim_end_matches(".git");
    clone_dir_name("git", tail)
}

fn sub_dir_of(clone_root: &Path, sub_path: Option<&str>) -> PathBuf {
    match sub_path.map(|s| s.trim().trim_matches('/')).filter(|s| !s.is_empty()) {
        Some(s) => clone_root.join(s),
        None => clone_root.to_path_buf(),
    }
}

/// 仓库可能没提交构建产物：安装依赖并构建（两步都设为「失败不中断」，
/// 真正能不能用由最后的 dsh plugin add 结果与日志说话）
fn build_steps(clone_root: &Path, plugin_dir: &Path) -> Vec<Step> {
    let install_label = format!("pnpm install（{}）", clone_root.display());
    let build_label = format!("pnpm run build（{}）", plugin_dir.display());
    vec![
        Step::Note {
            text: "仓库可能未提交构建产物（lib/），先安装依赖并构建…".into(),
        },
        Step::Cmd {
            program: "pnpm".into(),
            args: vec!["install".into()],
            cwd: Some(clone_root.to_path_buf()),
            label: install_label,
            soft: true,
        },
        Step::Cmd {
            program: "pnpm".into(),
            args: vec!["run".into(), "build".into()],
            cwd: Some(plugin_dir.to_path_buf()),
            label: build_label,
            soft: true,
        },
    ]
}

/// 「clone 仓库 + 本地 link 安装」的步骤序列；返回 (步骤, 克隆根目录)
pub fn steps_for_clone_install(
    input: &CloneInstallInput,
) -> Result<(Vec<Step>, PathBuf), String> {
    let url = input.url.trim();
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
    let dir_name = clone_dir_for_url(url);
    let root = git_plugins_dir().join(&dir_name);
    let plugin_dir = sub_dir_of(&root, input.sub_path.as_deref());
    let existed = root.join(".git").is_dir();
    let git_ref = input.git_ref.clone().filter(|r| !r.is_empty());
    let mut steps: Vec<Step> = Vec::new();

    if existed {
        let dir = root.to_string_lossy().into_owned();
        steps.push(Step::Note {
            text: format!("已存在本地克隆，先同步远端：{}", root.display()),
        });
        steps.push(Step::Cmd {
            program: "git".into(),
            args: vec!["-C".into(), dir.clone(), "fetch".into(), "--all".into(), "--prune".into()],
            cwd: None,
            label: format!("git -C {} fetch --all --prune", root.display()),
            soft: true,
        });
        match &git_ref {
            Some(r) => steps.push(Step::Cmd {
                program: "git".into(),
                args: vec!["-C".into(), dir.clone(), "checkout".into(), r.clone()],
                cwd: None,
                label: format!("git -C {} checkout {r}", root.display()),
                soft: true,
            }),
            None => steps.push(Step::Cmd {
                program: "git".into(),
                args: vec!["-C".into(), dir.clone(), "pull".into(), "--ff-only".into()],
                cwd: None,
                label: format!("git -C {} pull --ff-only", root.display()),
                soft: false,
            }),
        }
    } else {
        let mut args: Vec<String> = vec!["clone".into(), "--depth".into(), "1".into()];
        if let Some(r) = &git_ref {
            args.push("--branch".into());
            args.push(r.clone());
        }
        args.push(url.to_string());
        args.push(root.to_string_lossy().into_owned());
        steps.push(Step::Cmd {
            program: "git".into(),
            args,
            cwd: None,
            label: format!("git clone {url} {}", root.display()),
            soft: false,
        });
    }

    if input.build {
        steps.extend(build_steps(&root, &plugin_dir));
    }
    steps.push(Step::Dsh {
        args: vec!["add".into(), format!("link:{}", plugin_dir.to_string_lossy())],
    });
    Ok((steps, root))
}

/// 「git pull 更新已 link 的克隆仓库」步骤序列（按 git 工作树绝对路径，
/// 兼容启动器管理的 ~/.dsh-launcher/git-plugins 与用户自建 clone）
pub fn steps_for_pull_update_root(
    root: &Path,
    sub_path: Option<&str>,
    build: bool,
) -> Result<(Vec<Step>, PathBuf), String> {
    if !root.join(".git").is_dir() {
        return Err(format!("{} 不是 git 仓库", root.display()));
    }
    let root = root.to_path_buf();
    let plugin_dir = sub_dir_of(&root, sub_path);
    let mut steps = vec![
        Step::Note {
            text: format!("git pull（{}）", root.display()),
        },
        Step::Cmd {
            program: "git".into(),
            args: vec!["-C".into(), root.to_string_lossy().into_owned(), "pull".into(), "--ff-only".into()],
            cwd: None,
            label: format!("git -C {} pull --ff-only", root.display()),
            soft: false,
        },
    ];
    if build {
        steps.extend(build_steps(&root, &plugin_dir));
    }
    steps.push(Step::Dsh {
        args: vec!["add".into(), format!("link:{}", plugin_dir.to_string_lossy())],
    });
    Ok((steps, root))
}

// ── 本地克隆仓库（clone + link 安装的落点） ─────────────────

/// clone 安装的仓库根目录：~/.dsh-launcher/git-plugins
pub fn git_plugins_dir() -> PathBuf {
    crate::settings::launcher_home().join("git-plugins")
}

/// 仓库目录名：<owner>-<repo>
pub fn clone_dir_name(owner: &str, repo: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-._".contains(*c))
            .collect::<String>()
    };
    format!("{}-{}", clean(owner), clean(repo))
}

/// 禁止 git / ssh 走交互式登录的环境变量。
///
/// 启动器起的进程没有可交互的 stdin：一旦 git 需要账号密码，它会直接往**继承来的
/// 控制终端**（`/dev/tty`，也就是启动 launcher / tauri dev 的那个终端）打印
/// `Username for 'https://github.com':` 并一直等输入——用户看到的就是这样一行莫名其妙的提示，
/// 而我们这边只能干等到超时。统一设上下面的变量后，git 会立刻失败并把原因写进 stderr，
/// 由内置终端/更新检测如实展示。
pub fn git_no_prompt_env() -> Vec<(String, String)> {
    vec![
        // 不要在终端上询问用户名/密码
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        // Git Credential Manager 也别弹图形登录框
        ("GCM_INTERACTIVE".into(), "never".into()),
        // 禁止 ssh 走 askpass 弹窗（OpenSSH 8.4+）
        ("SSH_ASKPASS_REQUIRE".into(), "never".into()),
    ]
}

/// 只读探测（ls-remote 之类）额外加码：ssh 一律非交互，缺 key/口令就立即失败
fn git_probe_env() -> Vec<(String, String)> {
    let mut envs = git_no_prompt_env();
    envs.push(("GIT_SSH_COMMAND".into(), "ssh -o BatchMode=yes".into()));
    envs
}

/// profile 名合法性（防路径穿越：只允许单层目录名）
fn validate_profile_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() || n.contains('/') || n.contains('\\') || n.contains("..") {
        return Err(format!("非法 profile 名称：{name}"));
    }
    Ok(())
}

/// 校验并解析 git-plugins 下的一个直接子目录（拒绝路径穿越）
pub fn clone_dir_path(dir_name: &str) -> Result<PathBuf, String> {
    let name = dir_name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("非法仓库目录名：{dir_name}"));
    }
    Ok(git_plugins_dir().join(name))
}

/// 删除一个本地克隆目录（只允许删 git-plugins 下的直接子目录）
pub fn remove_clone_dir(dir_name: &str) -> Result<(), String> {
    remove_clone_path(&clone_dir_path(dir_name)?)
}

/// 按绝对路径删除克隆目录（做 git-plugins 边界与 .git 校验）
fn remove_clone_path(target: &Path) -> Result<(), String> {
    let root = git_plugins_dir();
    if !target.is_dir() {
        return Err(format!("目录不存在：{}", target.display()));
    }
    let canon_root = root.canonicalize().map_err(|e| format!("解析目录失败: {e}"))?;
    let canon = target.canonicalize().map_err(|e| format!("解析目录失败: {e}"))?;
    if !canon.starts_with(&canon_root) || canon == canon_root {
        return Err("拒绝删除 git-plugins 之外的目录".into());
    }
    if !canon.join(".git").is_dir() {
        // 只删启动器自己克隆的仓库：非 git 目录不让删，避免误伤手工放进去的插件
        return Err("该目录不是 git 仓库，为安全起见不自动删除".into());
    }
    std::fs::remove_dir_all(&canon).map_err(|e| format!("删除失败: {e}"))
}

/// 一个本地克隆仓库的概览
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClonedPlugin {
    pub dir_name: String,
    pub path: String,
    pub url: Option<String>,
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub subject: Option<String>,
    pub dirty: bool,
    /// 仓库内探测到的可安装插件子包
    pub candidates: Vec<crate::registry::PluginCandidate>,
}

/// 列出 ~/.dsh-launcher/git-plugins 下的克隆仓库（带 git 状态与插件探测）
pub fn list_cloned() -> Vec<ClonedPlugin> {
    let root = git_plugins_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_dir() || !path.join(".git").exists() {
            continue;
        }
        let dir_name = e.file_name().to_string_lossy().into_owned();
        let url = git_output(&path, &["config", "--get", "remote.origin.url"]);
        let branch = git_output(&path, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let commit = git_output(&path, &["rev-parse", "--short", "HEAD"]);
        let subject = git_output(&path, &["log", "-1", "--pretty=%s"]);
        let dirty = git_output(&path, &["status", "--porcelain"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        out.push(ClonedPlugin {
            dir_name: dir_name.clone(),
            path: path.to_string_lossy().into_owned(),
            url,
            branch,
            commit,
            subject,
            dirty,
            candidates: crate::registry::probe_local_plugins(&path).unwrap_or_default(),
        });
    }
    out.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    out
}

/// 在某目录执行 git 子命令并取 stdout（失败/超时返回 None）
pub fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
    let git = util::which("git")?;
    let all: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match util::run_captured_in(&git, &all, Some(cwd), &git_no_prompt_env(), Duration::from_secs(20)) {
        Some((true, text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// `git ls-remote <url> <ref>` 取远端提交（本地 git 命令，不吃 GitHub API 速率限制）。
///
/// 全程非交互：私有仓库/缺凭据时**不会**在用户终端里弹出账号密码提示，
/// 而是返回 Err(git 的原始报错) 供上层给出可读提示。
pub fn git_remote_head(url: &str, git_ref: Option<&str>) -> Result<String, String> {
    let git = util::which("git").ok_or("未找到 git 命令（请安装 git 后重试）")?;
    let mut args: Vec<String> = vec!["ls-remote".into(), url.into()];
    args.push(git_ref.filter(|r| !r.is_empty()).unwrap_or("HEAD").to_string());
    // 6s 上限：本机实测 git 二进制可能卡几十秒（凭据/地址族协商），
    // 而同样的事情走自实现的 ref 发现只要 0.5s——这里只作最后兜底
    let (ok, text) = util::run_captured_in(&git, &args, None, &git_probe_env(), Duration::from_secs(6))
        .ok_or("git ls-remote 执行失败或超时")?;
    if !ok {
        return Err(if text.trim().is_empty() {
            "git ls-remote 失败（无输出）".to_string()
        } else {
            text
        });
    }
    let sha = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .ok_or("远端没有返回任何 ref")?;
    if sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(sha.to_string())
    } else {
        Err(format!("远端 ref 不是提交哈希：{sha}"))
    }
}

/// 私有仓库/需登录的统一拒绝话术（启动器不替用户登录，也不支持私有远端）
pub const PRIVATE_REPO_REJECT: &str = "暂不支持私有仓库：请手动 git clone 到本地后，用「链接 / 本地」标签页的 link 路径安装（本地 link 源改代码即时生效）";

/// 远端不存在/地址写错的提示
pub const MISSING_REPO_REJECT: &str = "远端不存在或不可匿名访问：请检查仓库地址，或手动 git clone 后用 link 路径安装";

/// 该 git 命令的报错是否属于「需要登录凭据」
pub fn looks_like_auth_error(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "could not read username",
        "could not read password",
        "authentication failed",
        "terminal prompts disabled",
        "unable to read askpass",
        "permission denied (publickey",
        "host key verification failed",
        "no such identity",
        "无法读取远程仓库",
        "认证失败",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// 该报错是否属于「仓库不存在」（与「需要登录」区分开，给不同的行动建议）
pub fn looks_like_missing_repo(text: &str) -> bool {
    let t = text.to_lowercase();
    [
        "repository not found",
        "not found",
        "does not appear to be a git repository",
        "could not read from remote repository",
        "无法读取远程仓库",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// 凭据类失败的统一提示：私有仓库直接拒绝，指引手动 clone + link
pub fn auth_hint() -> String {
    format!("远端需要登录凭据。{PRIVATE_REPO_REJECT}")
}

/// 从失败输出里提取「被 minimumReleaseAge 拦下的条目」（pnpm 每行一条：`pkg@ver was published at …`）
fn release_age_entries(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        if !line.contains("was published at") {
            continue;
        }
        if let Some(tok) = line.split_whitespace().next() {
            let tok = tok.trim();
            // 形如 name@version 或 @scope/name@version
            if tok.contains('@') && !tok.starts_with('[') && !tok.ends_with(':') {
                let t = tok.to_string();
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// 把「子进程失败输出」翻译成**对症**的下一步建议。
///
/// 背景：`dsh` 对所有 `github:` 规格的失败都会打印一句 allowBuilds 提示（它的判断只看参数里
/// 有没有 github:），实际原因可能是供应链策略、包不存在等等。这里按真实报错分类，
/// 只给对应的那一条建议；认不出来就返回 None（不制造噪音）。
pub fn failure_hint(program: &str, output: &str, profile_dir: Option<&Path>) -> Option<String> {
    let low = output.to_lowercase();
    let dir = profile_dir
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<profile 目录>".into());

    // ① pnpm 供应链策略：minimumReleaseAge（锁定文件里的包发布太新）
    if low.contains("minimum_release_age") || low.contains("minimumreleaseage") {
        let entries = release_age_entries(output);
        let mut hint = String::from(
            "这次失败与插件本身无关：pnpm 的供应链策略（minimumReleaseAge）拒绝了这个 profile 的锁定文件。",
        );
        if entries.is_empty() {
            hint.push_str(&format!(
                "\n处理方式：在 {dir}/pnpm-workspace.yaml 里放行被拦下的版本（minimumReleaseAgeExclude），                 或执行 `pnpm clean --lockfile && pnpm install` 重新解析后再装。"
            ));
        } else {
            hint.push_str(&format!(
                "\n被拦下的条目（发布时间未达门槛）：{}",
                entries.join("、")
            ));
            hint.push_str(&format!(
                "\n\n放行它们——在 {dir}/pnpm-workspace.yaml 里加：\nminimumReleaseAgeExclude:\n{}",
                entries
                    .iter()
                    .map(|e| format!("  - {e}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
            hint.push_str(&format!(
                "\n（web-test 若是从别的 profile 复制来的，锁定文件沿用源 profile 的解析结果，\
                 而放行列表是跟着 pnpm-workspace.yaml 一起复制的——补上上面几行即可。）\
                 \n也可以改成重建锁定：cd {dir} && pnpm clean --lockfile && pnpm install。"
            ));
        }
        return Some(hint);
    }

    // ② 构建脚本被拦（prepare / postinstall 未放行）
    if low.contains("allowbuilds")
        || low.contains("ignored build scripts")
        || low.contains("blocked build scripts")
    {
        return Some(format!(
            "pnpm 拦下了依赖的构建脚本（prepare / postinstall）。\n把上面 pnpm 打印的键加进 {dir}/pnpm-workspace.yaml 的 allowBuilds（例如 `esbuild: true`）后重跑；\
             只有确实需要构建的包才放行。"
        ));
    }

    // ③ 需要登录凭据（私有仓库）
    if looks_like_auth_error(output) {
        return Some(auth_hint());
    }

    // ④ 包/版本/子路径不存在
    if low.contains("err_pnpm_no_matching_version")
        || low.contains("err_pnpm_fetch_404")
        || low.contains("404 not found")
        || low.contains("no matching version found")
    {
        return Some(
            "registry 上找不到这个包/版本（404 或版本不存在）。\n检查包名与版本号，或换一个 registry 镜像后重试。"
                .into(),
        );
    }
    if low.contains("err_pnpm_linked_pkg_dir_not_found") {
        return Some(
            "link 目标目录不存在。\n确认本地插件目录还在（clone 安装的仓库被删除后会出现这种情况），或者改用其它安装方式。"
                .into(),
        );
    }
    if low.contains("err_pnpm_outdated_lockfile") {
        return Some(format!(
            "锁定文件与 package.json 不一致（ERR_PNPM_OUTDATED_LOCKFILE）。\n在 {dir} 执行 `pnpm install` 重建锁定文件后重试。"
        ));
    }
    if low.contains("err_pnpm_git_fetch") || low.contains("prepare script") {
        return Some(format!(
            "git 依赖拉取/构建失败。\n若为构建脚本被拦，请按上面 pnpm 打印的键写入 {dir}/pnpm-workspace.yaml → allowBuilds；\
             若为鉴权失败，请改用 SSH 远端或手动 git clone 后 link 安装。"
        ));
    }
    let _ = program;
    None
}

/// `git ls-remote` 的报错 → 给用户的拒绝话术（纯函数，便于离线测试）
pub fn probe_error_hint(err: &str) -> String {
    let first = err.lines().next().unwrap_or("").trim();
    if looks_like_auth_error(err) {
        // 不复述 git 的 "could not read Username…"：那正是用户看不懂、也不该再看到的一行
        PRIVATE_REPO_REJECT.to_string()
    } else if looks_like_missing_repo(err) {
        MISSING_REPO_REJECT.to_string()
    } else {
        format!("无法访问远端：{first}")
    }
}

/// GitHub API 是否只是被限流/网络不可达（仅测试用：这种情况下跳过网络用例，不视为功能失败）
#[cfg(test)]
pub fn api_unavailable(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("403")
        || e.contains("rate limit")
        || e.contains("速率限制")
        || e.contains("请求 github api 失败")
        || e.contains("超时")
        || e.contains("timeout")
}

/// 解析 profile 依赖里 `link:` 指向的本地路径
pub fn link_target(spec: &str) -> Option<PathBuf> {
    let s = spec.trim();
    let raw = s
        .strip_prefix("link:")
        .or_else(|| s.strip_prefix("file:"))
        .unwrap_or(s);
    if raw.is_empty() {
        return None;
    }
    let p = PathBuf::from(raw);
    Some(p)
}

/// link 目标是否落在启动器管理的 git-plugins 目录内
pub fn in_git_plugins(path: &Path) -> bool {
    let root = git_plugins_dir();
    path.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_csi_sequences() {
        assert_eq!(strip_ansi("\u{1b}[32m+ @dshp/x\u{1b}[0m"), "+ @dshp/x");
        assert_eq!(strip_ansi("plain text"), "plain text");
        assert_eq!(strip_ansi("a\u{1b}[1mb\u{1b}[22mc"), "abc");
    }

    #[test]
    fn clone_dir_name_is_path_safe() {
        assert_eq!(clone_dir_name("Yinxe", "deepseek-harness-plugins"), "Yinxe-deepseek-harness-plugins");
        assert_eq!(clone_dir_name("a/../b", "r"), "a..b-r");
    }

    #[test]
    fn link_target_parses_specs() {
        assert_eq!(
            link_target("link:/home/u/x").unwrap().to_string_lossy(),
            "/home/u/x"
        );
        assert_eq!(link_target("file:../x").unwrap().to_string_lossy(), "../x");
        assert!(link_target("").is_none());
    }

    #[test]
    fn job_state_tracks_running_and_clear() {
        let st = PluginJobState::default();
        let (_id, h) = st.register("web", "install", "安装 @x", "dsh plugin add @x");
        assert!(st.busy_profile("web"));
        assert!(!st.busy_profile("other"));
        assert_eq!(st.snapshot().len(), 1);
        h.meta.lock().unwrap().running = false;
        assert!(!st.busy_profile("web"));
        st.finish(_id, Some(true), Some(0), false);
        assert_eq!(st.clear_finished(), 1);
        assert!(st.snapshot().is_empty());
    }

    /// 禁止交互式登录的环境变量必须覆盖 git/ssh 的三条提示通道
    #[test]
    fn no_prompt_env_covers_git_and_ssh() {
        let envs = git_no_prompt_env();
        let get = |k: &str| envs.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
        assert_eq!(get("GCM_INTERACTIVE").as_deref(), Some("never"));
        assert_eq!(get("SSH_ASKPASS_REQUIRE").as_deref(), Some("never"));
        // 只读探测额外禁用 ssh 交互（缺 key/口令立即失败，而不是挂在终端上等输入）
        let probe = git_probe_env();
        assert!(probe
            .iter()
            .any(|(k, v)| k == "GIT_SSH_COMMAND" && v.contains("BatchMode=yes")));
    }

    /// 用户实测输出：pnpm 供应链策略失败时的建议必须是「放行 minimumReleaseAgeExclude」，
    /// 不能再是 allowBuilds（dsh 对 github: 规格的失败一律会打印那句，具有误导性）
    #[test]
    fn release_age_failure_gets_policy_hint() {
        let out = "? Verifying lockfile against supply-chain policies (100 entries)...\n\
                   Progress: resolved 1, reused 0, downloaded 0, added 0\n\
                   ✗ Lockfile failed supply-chain policy check (100 entries in 3.1s)\n\
                   [ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION] 1 lockfile entries failed verification:\n\
                     dshmarket@1.50.0 was published at 2026-09-20T02:58:05.000Z, within the minimumReleaseAge cutoff (2026-09-19T07:29:27.854Z)\n\
                   The lockfile contains entries that the active policies reject.\n\
                   Alternatively, relax the policy that flagged them.\n\
                   dsh: pnpm failed in profile directory /home/u/.dsh/profiles/web-test\n\
                   dsh: git-hosted plugins build on install via their prepare script, which pnpm blocks until allowed — add the exact key pnpm printed above under allowBuilds in /home/u/.dsh/profiles/web-test/pnpm-workspace.yaml, then re-run";
        let hint = failure_hint(
            "dsh plugin",
            out,
            Some(Path::new("/home/u/.dsh/profiles/web-test")),
        )
        .expect("应给出建议");
        // 结论要指向真正的修复动作
        assert!(hint.contains("minimumReleaseAgeExclude"), "hint={hint}");
        assert!(hint.contains("dshmarket@1.50.0"), "hint={hint}");
        assert!(hint.contains("/home/u/.dsh/profiles/web-test/pnpm-workspace.yaml"), "hint={hint}");
        assert!(hint.contains("pnpm clean --lockfile"), "hint={hint}");
        // 不能把 allowBuilds 当成结论（只在解释 dsh 那句兜底提示时出现「allowBuilds」）
        assert!(
            !hint.contains("allowBuilds") || hint.contains("与插件本身无关"),
            "hint={hint}"
        );
        // 多条目也要能提取
        let two = "[ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION]\n  a@1.0.0 was published at X\n  @s/b@2.3.4 was published at Y\n";
        assert_eq!(
            release_age_entries(two),
            vec!["a@1.0.0".to_string(), "@s/b@2.3.4".to_string()]
        );
    }

    /// 真正被拦构建脚本时才给 allowBuilds 建议
    #[test]
    fn build_script_block_gets_allowbuilds_hint() {
        let out = "dsh: pnpm failed in profile directory /p\npnpm: Ignored build scripts: esbuild. Run \"pnpm approve-builds\" to allow them.\nadd the exact key pnpm printed above under allowBuilds in /p/pnpm-workspace.yaml";
        let hint = failure_hint("dsh plugin", out, Some(Path::new("/p"))).unwrap();
        assert!(hint.contains("allowBuilds"), "hint={hint}");
        assert!(!hint.contains("minimumReleaseAgeExclude"), "hint={hint}");
    }

    #[test]
    fn other_failures_are_classified() {
        let dir = Path::new("/p");
        // 私有仓库
        let auth = failure_hint(
            "git",
            "致命错误：could not read Username for 'https://github.com': terminal prompts disabled",
            Some(dir),
        )
        .unwrap();
        assert!(auth.contains("暂不支持私有仓库"), "auth={auth}");
        // 包不存在
        let miss = failure_hint("dsh plugin", " ERR_PNPM_NO_MATCHING_VERSION  No matching version found for x@9", Some(dir)).unwrap();
        assert!(miss.contains("找不到"), "miss={miss}");
        // link 目标丢失
        let link = failure_hint("dsh plugin", "ERR_PNPM_LINKED_PKG_DIR_NOT_FOUND  /tmp/gone", Some(dir)).unwrap();
        assert!(link.contains("link 目标目录不存在"), "link={link}");
        // 认不出来就不给建议，避免噪音
        assert!(failure_hint("git", "fatal: destination path 'x' already exists", Some(dir)).is_none());
    }

    #[test]
    fn missing_repo_is_distinguished_from_private() {
        assert!(looks_like_missing_repo(
            "remote: Repository not found.\nfatal: repository 'https://github.com/x/y.git/' not found"
        ));
        assert!(looks_like_missing_repo("致命错误：无法读取远程仓库。"));
        assert!(!looks_like_missing_repo("fatal: Authentication failed"));
        assert!(auth_hint().contains("暂不支持私有仓库"));
        assert!(auth_hint().contains("git clone"));
    }

    /// 纯函数：git 报错 → 拒绝话术（不依赖网络，覆盖真实报错串）
    #[test]
    fn probe_error_hint_maps_real_git_errors() {
        let private = probe_error_hint(
            "致命错误：could not read Username for 'https://github.com': terminal prompts disabled",
        );
        assert!(private.contains("暂不支持私有仓库"), "{private}");
        assert!(!private.contains("Username"), "不该把登录提示透出去：{private}");
        let missing = probe_error_hint(
            "remote: Repository not found.\nfatal: repository 'https://github.com/x/y.git/' not found",
        );
        assert!(missing.contains("远端不存在"), "{missing}");
        let other = probe_error_hint("ssh: connect to host github.com port 22: Connection timed out");
        assert!(other.starts_with("无法访问远端"), "{other}");
    }

    /// 网络用例：仓库可访问性判定（走自实现 ref 发现，0.5s 级、免额度）
    #[test]
    fn repo_access_classifies_private_missing_and_public() {
        use crate::registry::RepoAccess;
        // 私有仓库：401 → NotPublic（不弹登录、不挂起）
        match tauri::async_runtime::block_on(crate::registry::probe_remote_access(
            "https://github.com/Yinxe/dsh-qqbot.git",
            None,
        )) {
            RepoAccess::NotPublic => {}
            RepoAccess::Unknown(r) => eprintln!("跳过（网络不可达）：{r}"),
            RepoAccess::Public => panic!("私有仓库不应判为 Public"),
        }
        // 不存在的仓库：GitHub 对「私有」与「不存在」都要求凭据 → 同样 NotPublic
        match tauri::async_runtime::block_on(crate::registry::probe_remote_access(
            "https://github.com/Yinxe/definitely-not-a-repo-xyz.git",
            None,
        )) {
            RepoAccess::NotPublic => {}
            RepoAccess::Unknown(r) => eprintln!("跳过（网络不可达）：{r}"),
            RepoAccess::Public => panic!("不存在的仓库不应判为 Public"),
        }
        // 公开仓库
        match tauri::async_runtime::block_on(crate::registry::probe_remote_access(
            "https://github.com/Yinxe/dsh-luncher.git",
            None,
        )) {
            RepoAccess::Public => {}
            other => eprintln!("跳过（网络不可达）：{other:?}"),
        }
    }

    #[test]
    fn auth_errors_are_recognised() {
        // 实测报错串（git 2.4x，中文 locale）
        assert!(looks_like_auth_error(
            "致命错误：could not read Username for 'https://github.com': terminal prompts disabled"
        ));
        assert!(looks_like_auth_error("fatal: Authentication failed for 'https://github.com/x/y.git/'"));
        assert!(looks_like_auth_error("git@github.com: Permission denied (publickey)."));
        assert!(looks_like_auth_error("错误：unable to read askpass response from '/bin/false'"));
        assert!(!looks_like_auth_error("fatal: destination path 'x' already exists"));
        assert!(auth_hint().contains("暂不支持私有仓库"));
    }

    /// clone 安装 / pull 升级的步骤编排：clone→(构建)→dsh plugin add link:…
    #[test]
    fn clone_and_pull_steps_are_ordered() {
        let _env = util::DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-clone-steps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_LAUNCHER_HOME", &tmp);

        let input = CloneInstallInput {
            url: "https://github.com/Yinxe/deepseek-harness-plugins.git".into(),
            git_ref: Some("main".into()),
            sub_path: Some("plugins/mcwiki-search".into()),
            build: true,
        };
        let (steps, root) = steps_for_clone_install(&input).unwrap();
        // DSH_LAUNCHER_HOME 覆盖的是 HOME，因此落点仍带 .dsh-launcher 前缀
        assert_eq!(root, tmp.join(".dsh-launcher/git-plugins/Yinxe-deepseek-harness-plugins"));
        // 全新克隆：git clone（浅克隆 + 指定分支）→ 构建提示 → pnpm install → pnpm build → dsh plugin add
        assert_eq!(steps.len(), 5, "steps={steps:?}");
        match &steps[0] {
            Step::Cmd { program, args, .. } => {
                assert_eq!(program, "git");
                assert_eq!(args[0], "clone");
                assert!(args.iter().any(|a| a == "main"));
                assert!(args.iter().any(|a| a == "--depth"));
            }
            other => panic!("首步应为 git clone，实际 {other:?}"),
        }
        match steps.last().unwrap() {
            Step::Dsh { args } => {
                assert_eq!(args[0], "add");
                assert!(
                    args[1].ends_with("/.dsh-launcher/git-plugins/Yinxe-deepseek-harness-plugins/plugins/mcwiki-search"),
                    "link 目标 = {}",
                    args[1]
                );
                assert!(args[1].starts_with("link:"));
            }
            other => panic!("末步应为 dsh plugin add，实际 {other:?}"),
        }

        // 已存在克隆：改为 fetch + pull，不再 clone
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let (steps2, _) = steps_for_clone_install(&input).unwrap();
        assert!(matches!(&steps2[0], Step::Note { .. }), "steps2={steps2:?}");
        assert!(steps2.iter().any(|s| matches!(
            s,
            Step::Cmd { args, .. } if args.iter().any(|a| a == "fetch")
        )));

        // pull 升级：git pull --ff-only → 构建 → 重新 link
        let (steps3, _) = steps_for_pull_update_root(&root, Some("plugins/mcwiki-search"), true).unwrap();
        assert!(matches!(&steps3[0], Step::Note { .. }));
        match &steps3[1] {
            Step::Cmd { args, .. } => {
                assert!(args.iter().any(|a| a == "pull"));
                assert!(args.iter().any(|a| a == "--ff-only"));
            }
            other => panic!("pull 升级第 2 步应为 git pull，实际 {other:?}"),
        }
        // 非 git 目录必须被拒
        assert!(steps_for_pull_update_root(&tmp, None, false).is_err());

        std::env::remove_var("DSH_LAUNCHER_HOME");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 真起进程 + 真流式事件：stdout/stderr 都进日志缓冲，退出码与状态正确
    #[cfg(unix)]
    #[test]
    fn runner_streams_stdout_and_stderr() {
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let state = PluginJobState::default();
        let req = JobRequest {
            profile: "web".into(),
            kind: "install".into(),
            label: "流式测试".into(),
            steps: vec![Step::Cmd {
                program: "sh".into(),
                args: vec![
                    "-c".into(),
                    "echo 第一行; echo 第二行; echo 错误行 1>&2; printf '进度条\\r完成\\n'; exit 0".into(),
                ],
                cwd: None,
                label: "sh -c …".into(),
                soft: false,
            }],
        };
        let id = start_job(handle, &state, Settings::default(), req).unwrap();
        let job = wait_job(&state, id);
        assert_eq!(job.ok, Some(true), "job={job:?}");
        assert_eq!(job.exit_code, Some(0));
        assert!(!job.running && !job.cancelled);
        let texts: Vec<String> = job.lines.iter().map(|l| l.text.clone()).collect();
        assert!(texts.iter().any(|t| t == "第一行"), "lines={texts:?}");
        assert!(texts.iter().any(|t| t == "错误行"), "lines={texts:?}");
        // \r 进度重绘被拆成两行，不会糊在一起
        assert!(texts.iter().any(|t| t == "进度条") && texts.iter().any(|t| t == "完成"), "lines={texts:?}");
        // 首行是命令行回显，末行是结束状态
        assert!(texts[0].contains("$"), "lines={texts:?}");
        assert!(texts.last().unwrap().contains("任务完成"), "lines={texts:?}");
        // stderr 行被标成 stderr 流
        assert!(job.lines.iter().any(|l| l.stream == "stderr" && l.text == "错误行"));
    }

    /// 失败步骤会中止后续步骤，并如实记录退出码
    #[cfg(unix)]
    #[test]
    fn runner_stops_on_failure() {
        let app = tauri::test::mock_app();
        let state = PluginJobState::default();
        let req = JobRequest {
            profile: "web".into(),
            kind: "install".into(),
            label: "失败测试".into(),
            steps: vec![
                Step::Cmd {
                    program: "sh".into(),
                    args: vec!["-c".into(), "echo 先失败; exit 3".into()],
                    cwd: None,
                    label: "sh fail".into(),
                    soft: false,
                },
                Step::Cmd {
                    program: "sh".into(),
                    args: vec!["-c".into(), "echo 不该执行".into()],
                    cwd: None,
                    label: "sh never".into(),
                    soft: false,
                },
            ],
        };
        let id = start_job(app.handle().clone(), &state, Settings::default(), req).unwrap();
        let job = wait_job(&state, id);
        assert_eq!(job.ok, Some(false));
        assert_eq!(job.exit_code, Some(3));
        let texts: Vec<String> = job.lines.iter().map(|l| l.text.clone()).collect();
        assert!(!texts.iter().any(|t| t.contains("不该执行")), "lines={texts:?}");
        assert!(texts.last().unwrap().contains("任务失败"));
    }

    /// 取消运行中的任务：进程被杀，任务标记为已取消
    #[cfg(unix)]
    #[test]
    fn runner_cancel_kills_process() {
        let app = tauri::test::mock_app();
        let state = PluginJobState::default();
        let req = JobRequest {
            profile: "web".into(),
            kind: "install".into(),
            label: "取消测试".into(),
            steps: vec![Step::Cmd {
                program: "sh".into(),
                args: vec!["-c".into(), "echo 开始; sleep 30; echo 不该到".into()],
                cwd: None,
                label: "sh sleep".into(),
                soft: false,
            }],
        };
        let id = start_job(app.handle().clone(), &state, Settings::default(), req).unwrap();
        // 等它真的跑起来（拿到 child）
        for _ in 0..60 {
            if state
                .inner
                .lock()
                .unwrap()
                .jobs
                .get(&id)
                .map(|h| h.child.lock().unwrap().is_some())
                .unwrap_or(false)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(state.cancel(id), "取消应被接受");
        let job = wait_job(&state, id);
        assert!(!job.running);
        assert!(job.cancelled);
        let texts: Vec<String> = job.lines.iter().map(|l| l.text.clone()).collect();
        assert!(!texts.iter().any(|t| t.contains("不该到")), "lines={texts:?}");
        assert!(texts.iter().any(|t| t.contains("已取消")), "lines={texts:?}");
    }

    /// 同一 profile 不允许并发任务（pnpm 并发写同一目录会损坏 node_modules）
    #[cfg(unix)]
    #[test]
    fn runner_rejects_concurrent_same_profile() {
        let app = tauri::test::mock_app();
        let state = PluginJobState::default();
        let make = |label: &str| JobRequest {
            profile: "web".into(),
            kind: "install".into(),
            label: label.into(),
            steps: vec![Step::Cmd {
                program: "sh".into(),
                args: vec!["-c".into(), "sleep 5".into()],
                cwd: None,
                label: "sh sleep".into(),
                soft: false,
            }],
        };
        let id = start_job(app.handle().clone(), &state, Settings::default(), make("第一个")).unwrap();
        let err = start_job(app.handle().clone(), &state, Settings::default(), make("第二个"))
            .expect_err("并发应被拒绝");
        assert!(err.contains("已有插件任务"), "err={err}");
        state.cancel(id);
        let _ = wait_job(&state, id);
    }

    /// 轮询等待任务结束（最多 20s）
    fn wait_job(state: &PluginJobState, id: u64) -> PluginJob {
        for _ in 0..400 {
            let snap = state.snapshot();
            if let Some(j) = snap.iter().find(|j| j.id == id) {
                if !j.running {
                    return j.clone();
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("任务 {id} 超时未结束");
    }
}
