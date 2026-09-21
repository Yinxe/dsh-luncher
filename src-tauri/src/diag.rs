//! 分类日志系统。
//!
//! 为什么存在：release 版是 Windows GUI 程序（`windows_subsystem = "windows"`），panic
//! 不会打印到任何地方；后端一旦卡住，用户能看到的只有「白屏 / 未响应 / 装不上」，手上
//! 没有任何可发出来的证据（issue #1 全靠用户手数 `tasklist.exe` 才定位到）。
//!
//! 设计（按子系统分类，一个文件一类问题）：
//!
//! | 文件 | 记什么 |
//! | --- | --- |
//! | `app.log` | 生命周期：启动各阶段、版本、托盘、设置读写、环境探测、自更新 |
//! | `instance.log` | dsh 实例：内嵌/独立/外部的启动、发现、归属判定、停止 |
//! | `install.log` | dsh 版本：安装（npm 命令/退出码/stderr）、卸载、切换 |
//! | `runtime.log` | 内置 Node：下载字节数、解压尝试、目录快照 |
//! | `plugin.log` | 插件：安装/更新/克隆任务的命令与结果 |
//! | `profile.log` | profile：增删改、配置读写 |
//! | `network.log` | 网络：registry / GitHub / 更新清单的请求与失败原因 |
//! | `ui.log` | 前端报错（window.onerror / 未处理的 Promise / 报错 toast） |
//! | `panic.log` | 崩溃：位置、消息、回溯 |
//!
//! 格式统一为
//! `2026-09-21T01:23:45.678Z run=<本次启动标识> pid=<pid> +<距启动毫秒>ms LEVEL 正文`
//! —— 时间可读、`run=` 能区分不同次启动、`LEVEL` 能 grep（`grep ERROR ui.log`）。
//!
//! 级别：默认 INFO（`info/warn/error` 都写）；`debug()` 默认丢弃，把环境变量
//! `DSH_LAUNCHER_LOG=debug` 打开后才会落盘（轮询类的逐次细节走 debug，避免日志被刷爆）。
//!
//! 约束：日志**不能自己变成故障源** —— 写入失败一律静默忽略，单文件超上限就滚动一份
//! `.1`；`debug` 级别关掉时连字符串都不拼。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 单个日志文件上限；超过就滚动一份 `.1`（只留一代，不会无限增长）
const MAX_BYTES: u64 = 1024 * 1024;

/// 目录名 → 用途（README / 诊断包里都引用这张表，避免以后各写一份）
pub const CATEGORIES: &[(&str, &str)] = &[
    ("app", "生命周期：启动阶段、版本、托盘、设置、环境探测、自更新"),
    ("instance", "dsh 实例：内嵌/独立/外部的启动、发现、归属判定、停止"),
    ("install", "dsh 版本：安装（npm 命令/退出码/stderr）、卸载、切换"),
    ("runtime", "内置 Node：下载、解压尝试、目录快照"),
    ("plugin", "插件：安装/更新/克隆任务的命令与结果"),
    ("profile", "profile：增删改与配置读写"),
    ("network", "网络：registry / GitHub / 更新清单请求与失败原因"),
    ("ui", "前端报错"),
    ("panic", "崩溃：位置、消息、回溯"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }

    fn code(self) -> u8 {
        match self {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        }
    }
}

/// 当前级别（默认 INFO；`DSH_LAUNCHER_LOG=debug` 打开细节日志）
fn min_level() -> Level {
    static LEVEL: AtomicU8 = AtomicU8::new(u8::MAX);
    let cached = LEVEL.load(Ordering::Relaxed);
    if cached != u8::MAX {
        return match cached {
            0 => Level::Debug,
            2 => Level::Warn,
            _ => Level::Info,
        };
    }
    let parsed = match std::env::var("DSH_LAUNCHER_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "debug" | "trace" => Level::Debug,
        "warn" => Level::Warn,
        "error" => Level::Error,
        _ => Level::Info,
    };
    LEVEL.store(parsed.code(), Ordering::Relaxed);
    parsed
}

/// 当前级别名（写进启动横幅，排查时先确认「是不是没开 debug」）
pub fn level_label() -> &'static str {
    min_level().label()
}

/// 日志目录（`~/.dsh-launcher/logs/`）：设置里「打开日志目录」按钮指向它
pub fn logs_dir() -> PathBuf {
    crate::settings::launcher_home().join("logs")
}

/// 本次启动的标识（秒级 epoch）：同一个日志文件里混着多次启动的记录，
/// 排查时先按 `run=` 分组，才能把「哪一次启动出的问题」分开看
pub fn run_id() -> u128 {
    static RUN: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    *RUN.get_or_init(|| now_ms() / 1000)
}

fn pid() -> u32 {
    std::process::id()
}

/// 进程启动至今的毫秒数：排查卡顿时，各阶段之间的间隔比绝对时间更有用
fn since_start() -> u128 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// UTC 时间戳（ISO-8601，毫秒）：不引 chrono，就自己算公历
fn utc_iso(ms: u128) -> String {
    let secs = (ms / 1000) as i64;
    let millis = (ms % 1000) as u32;
    // days_from_civil 的逆运算（Howard Hinnant 算法）
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

fn line(level: Level, msg: &str) -> String {
    format!(
        "{} run={} pid={} +{}ms {:<5} {}",
        utc_iso(now_ms()),
        run_id(),
        pid(),
        since_start(),
        level.label(),
        msg
    )
}

/// 追加一行到 `logs/<cat>.log`。多线程可能同时写（panic hook 也在内），统一加锁。
fn append(cat: &str, text: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_append(&logs_dir(), cat, text);
}

/// 可指定目录的追加（诊断包等场景复用；测试也用它避免污染真实日志目录）
fn write_append(dir: &Path, cat: &str, text: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join(format!("{cat}.log"));
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join(format!("{cat}.log.1")));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{text}");
    }
}

/// 主入口：按类别记一条日志。
///
/// 类别用 [`CATEGORIES`] 里的名字（`app` / `instance` / `install` / `runtime` /
/// `plugin` / `profile` / `network` / `ui`），未知类别会落到 `app.log`，
/// 免得手滑写错文件名把日志散到别处。
pub fn log(cat: &str, level: Level, msg: &str) {
    if level < min_level() {
        return;
    }
    let cat = normalize_cat(cat);
    append(cat, &line(level, msg));
}

fn normalize_cat(cat: &str) -> &str {
    CATEGORIES
        .iter()
        .map(|(name, _)| *name)
        .find(|name| *name == cat)
        .unwrap_or("app")
}

pub fn info(cat: &str, msg: &str) {
    log(cat, Level::Info, msg);
}

pub fn warn(cat: &str, msg: &str) {
    log(cat, Level::Warn, msg);
}

pub fn error(cat: &str, msg: &str) {
    log(cat, Level::Error, msg);
}

/// 细节日志：默认丢弃。轮询类（实例状态、端口归属）逐次记录会刷爆日志，
/// 需要时用 `DSH_LAUNCHER_LOG=debug` 打开。
pub fn debug(cat: &str, msg: impl FnOnce() -> String) {
    if min_level() > Level::Debug {
        return;
    }
    log(cat, Level::Debug, &msg());
}

/// 兼容旧调用点：等同于 `info`
pub fn op(cat: &str, msg: &str) {
    info(cat, msg);
}

/// 启动阶段留痕（`app.log`），保留 `[+Nms]` 便于横向对比各阶段耗时
pub fn mark(label: &str) {
    info("app", label);
}

/// 把 panic 落到 `logs/panic.log` —— release 构建里这是唯一的崩溃线索。
/// 仍然调用原来的 hook，所以 debug / 终端里运行时行为完全不变。
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "未知位置".into());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "（无消息）".into());
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("未命名").to_string();
        // 回溯：release 里没有它就只剩一句「某个地方炸了」。取前 40 行够定位，
        // 避免整段符号表塞满日志。
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let bt: String = bt.lines().take(40).collect::<Vec<_>>().join("\n    ");
        append(
            "panic",
            &format!(
                "{}\n    线程: {name}\n    回溯:\n    {bt}",
                line(Level::Error, &format!("panic @ {loc}: {msg}"))
            ),
        );
        prev(info);
    }));
}

/// 生成「诊断包」正文：环境摘要 + 各分类日志尾部 + 实例状态。
///
/// 用户报问题时只要发这一个文件，不必逐个问「你的 node 在哪」「npm 装了吗」
/// 「装的哪个版本」。**绝不包含凭据**：settings 走脱敏输出。
pub fn diagnostics(app_version: &str, settings: &crate::settings::Settings) -> String {
    let mut out = String::new();
    out.push_str("==== DSH Launcher 诊断包 ====\n");
    out.push_str(&format!("生成时间: {}\n", utc_iso(now_ms())));
    out.push_str(&format!(
        "本次启动: run={} pid={} 已运行 +{}ms\n",
        run_id(),
        pid(),
        since_start()
    ));
    out.push_str(&format!("启动器版本: {app_version}\n"));
    out.push_str(&format!(
        "系统: {} {} / 架构 {}\n",
        std::env::consts::OS,
        std::env::consts::FAMILY,
        std::env::consts::ARCH
    ));
    out.push_str(&format!("日志目录: {}\n", logs_dir().display()));
    out.push_str(&format!("PATH: {}\n", std::env::var("PATH").unwrap_or_default()));

    out.push_str("\n---- 环境 ----\n");
    out.push_str(&format!(
        "node: {}\n",
        crate::util::find_node(settings)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "（未找到）".into())
    ));
    out.push_str(&format!(
        "npm:  {}\n",
        crate::util::find_npm(settings)
            .map(|i| {
                let mut s = i.program.display().to_string();
                for a in &i.args {
                    s.push(' ');
                    s.push_str(a);
                }
                s
            })
            .unwrap_or_else(|| "（未找到）".into())
    ));
    out.push_str(&format!(
        "内置 Node: {}（{}）\n",
        if crate::runtime::runtime_installed() {
            "已安装"
        } else {
            "未安装"
        },
        crate::runtime::runtime_dir().display()
    ));
    out.push_str(&format!(
        "dsh 数据目录: {}\nprofile 目录: {}\n版本目录: {}\n",
        crate::profiles::dsh_native_home().display(),
        crate::profiles::profiles_dir().display(),
        crate::settings::versions_dir().display()
    ));
    out.push_str(&format!(
        "已装版本: {}\n",
        crate::installed::collect_installed(settings)
            .iter()
            .map(|v| format!("{}（{}）", v.version, v.source))
            .collect::<Vec<_>>()
            .join("、")
    ));
    out.push_str(&format!("profile: {}\n", {
        let names: Vec<String> = crate::profiles::scan_profiles()
            .into_iter()
            .map(|p| p.name)
            .collect();
        if names.is_empty() {
            "（无）".into()
        } else {
            names.join("、")
        }
    }));

    out.push_str("\n---- 设置（凭据已脱敏）----\n");
    for (k, v) in settings
        .redacted_summary()
        .iter()
    {
        out.push_str(&format!("{k} = {v}\n"));
    }

    out.push_str("\n---- 日志 ----\n");
    for (cat, desc) in CATEGORIES {
        out.push_str(&format!("\n===== {cat}.log（{desc}）=====\n"));
        match read_tail(&logs_dir().join(format!("{cat}.log")), 64 * 1024) {
            Some((text, truncated)) => {
                if truncated {
                    out.push_str("（仅显示末尾 64KB）\n");
                }
                out.push_str(&text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str("（无）\n"),
        }
    }
    out
}

/// 读文件尾部（供诊断包使用；日志可能已滚过一代，两个都读）
fn read_tail(path: &Path, max: u64) -> Option<(String, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut text = String::new();
    let mut truncated = false;
    for p in [path.with_extension("log.1"), path.to_path_buf()] {
        if !p.is_file() {
            continue;
        }
        if let Ok(mut f) = std::fs::File::open(&p) {
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            if len > max {
                truncated = true;
                let _ = f.seek(SeekFrom::Start(len - max));
            }
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                text.push_str(&String::from_utf8_lossy(&buf));
            }
        }
    }
    (!text.is_empty()).then_some((text, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_iso_matches_known_instants() {
        assert_eq!(utc_iso(0), "1970-01-01T00:00:00.000Z");
        // 2026-09-21T01:23:45.678Z
        let ms = 1_789_953_825_678u128;
        assert_eq!(utc_iso(ms), "2026-09-21T01:23:45.678Z");
        // 闰年 2 月 29 日
        assert_eq!(utc_iso(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn unknown_category_falls_back_to_app() {
        assert_eq!(normalize_cat("install"), "install");
        assert_eq!(normalize_cat("../evil"), "app");
        assert_eq!(normalize_cat(""), "app");
    }

    #[test]
    fn log_line_carries_run_pid_level() {
        let l = line(Level::Warn, "hello");
        assert!(l.contains("run="), "{l}");
        assert!(l.contains("pid="), "{l}");
        assert!(l.contains("WARN"), "{l}");
        assert!(l.ends_with("hello"), "{l}");
    }

    /// 诊断包：必须带够排查信息，且**绝不能泄露凭据**（它会经由聊天工具发给别人）
    #[test]
    fn diagnostics_carries_context_without_secrets() {
        let _env = crate::util::DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-diag-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_LAUNCHER_HOME", &tmp);

        info("install", "npm 命令：node npm-cli.js install --prefix X");
        error("ui", "渲染报错：Cannot read properties of undefined");

        let mut settings = crate::settings::Settings::default();
        settings.github_token = "ghp_supersecret".into();
        settings.registry = "https://user:tok@registry.example.com".into();

        let body = diagnostics("9.9.9", &settings);
        assert!(body.contains("==== DSH Launcher 诊断包 ===="), "缺标题");
        assert!(body.contains("npm 命令：node npm-cli.js install"), "缺 install 日志");
        assert!(body.contains("渲染报错"), "缺 ui 日志");
        assert!(body.contains("PATH:"), "缺 PATH");
        // 凭据：一个字都不能出现
        assert!(!body.contains("ghp_supersecret"), "泄露了 GitHub Token");
        assert!(!body.contains("tok@registry"), "泄露了 registry 凭据");
        assert!(body.contains("github_token = （已设置，长度 15）"), "脱敏摘要不对");
        assert!(body.contains("***@registry.example.com"), "registry 应脱敏");

        std::env::remove_var("DSH_LAUNCHER_HOME");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn writes_and_rotates_per_category() {
        let dir = std::env::temp_dir().join(format!("dsh-logtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_append(&dir, "ui", "first");
        write_append(&dir, "ui", "second");
        let text = std::fs::read_to_string(dir.join("ui.log")).unwrap();
        assert!(text.contains("first") && text.contains("second"), "{text}");
        // 其它分类互不影响
        assert!(!dir.join("install.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
