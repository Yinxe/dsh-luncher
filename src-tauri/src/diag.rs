//! 启动留痕与崩溃日志。
//!
//! 为什么需要：release 构建带 `windows_subsystem = "windows"`，panic 不会打印到任何
//! 地方；而后端一旦在启动路径上卡住，用户能看到的只有「窗口白屏 + 未响应」，没有任何
//! 线索（issue #1 就是靠用户手工数 `tasklist.exe` 才定位到）。这里把启动各阶段耗时
//! 与 panic 落到 `~/.dsh-launcher/logs/`，下次这类问题直接看文件即可。
//!
//! 约束：留痕**不能自己变成故障源** —— 所有写入失败一律静默忽略，文件有大小上限。

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// 单个日志文件上限；超过就滚动一份 `.1`（只留一代，不会无限增长）
const MAX_BYTES: u64 = 256 * 1024;

fn logs_dir() -> PathBuf {
    crate::settings::launcher_home().join("logs")
}

/// 追加一行到 `logs/<file>`。多线程可能同时写（panic hook 也在内），统一加锁。
fn append(file: &str, line: &str) {
    static LOCK: Mutex<()> = Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = logs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(file);
    if std::fs::metadata(&path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join(format!("{file}.1")));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// 进程启动至今的毫秒数：留痕里最有用的是各阶段之间的间隔，而不是绝对时间
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

/// 记一条启动阶段留痕（`logs/startup.log`）
pub fn mark(label: &str) {
    append(
        "startup.log",
        &format!("[{} +{}ms] {label}", now_ms(), since_start()),
    );
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
        // 崩溃时间与「距启动多久」一起记：启动即崩和跑几小时后崩，指向完全不同的问题
        append(
            "panic.log",
            &format!("[{} +{}ms] panic @ {loc}: {msg}", now_ms(), since_start()),
        );
        prev(info);
    }));
}
