use serde::Serialize;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::settings::{self, Settings};
use crate::util;

/// 单个安装任务的状态（字段用 Arc 以便克隆进工作线程）
#[derive(Clone, Default)]
pub struct InstallState {
    pub job_version: Arc<Mutex<Option<String>>>,
    pub child: Arc<Mutex<Option<Child>>>,
    pub cancelled: Arc<AtomicBool>,
}

impl InstallState {
    pub fn running_version(&self) -> Option<String> {
        self.job_version.lock().unwrap().clone()
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallLogEvent {
    pub version: String,
    pub line: String,
    /// stdout | stderr | info
    pub stream: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallFinishedEvent {
    pub version: String,
    pub success: bool,
    pub message: String,
}

fn emit_log(app: &AppHandle, version: &str, line: &str, stream: &str) {
    let _ = app.emit(
        "install-log",
        InstallLogEvent {
            version: version.to_string(),
            line: line.to_string(),
            stream: stream.to_string(),
        },
    );
}

/// 启动后台安装线程；同一时间只允许一个安装任务
pub fn start_install(
    app: AppHandle,
    state: &InstallState,
    settings: &Settings,
    version: &str,
) -> Result<(), String> {
    if !util::is_safe_version(version) {
        return Err("非法版本号".into());
    }
    {
        let mut jv = state.job_version.lock().unwrap();
        if jv.is_some() {
            return Err("已有安装任务正在进行，请等待完成或取消".into());
        }
        *jv = Some(version.to_string());
    }
    state.cancelled.store(false, Ordering::SeqCst);

    let state2 = state.clone();
    let settings2 = settings.clone();
    let version2 = version.to_string();
    std::thread::spawn(move || {
        let result = run_install(&app, &settings2, &version2, &state2);
        let (success, message) = match result {
            Ok(m) => (true, m),
            Err(e) => (false, e),
        };
        let _ = app.emit(
            "install-finished",
            InstallFinishedEvent {
                version: version2.clone(),
                success,
                message: message.clone(),
            },
        );
        state2.child.lock().unwrap().take();
        *state2.job_version.lock().unwrap() = None;
    });
    Ok(())
}

/// 删除启动器管理目录内的版本目录。
/// 删除前拒绝符号链接：防止 versions 下被放入指向启动器目录之外
/// （如全局 node_modules）的链接时，remove_dir_all 误删链接目标。
fn remove_version_dir(dir: &Path) -> Result<(), String> {
    if dir.is_symlink() {
        return Err(format!(
            "{} 是符号链接，拒绝删除；请手动检查 ~/.dsh-starter/versions",
            dir.display()
        ));
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("删除目录失败: {e}"))
}

fn run_install(
    app: &AppHandle,
    settings: &Settings,
    version: &str,
    state: &InstallState,
) -> Result<String, String> {
    let versions_dir = settings::versions_dir();
    let target = versions_dir.join(version);

    // 残留目录（上次安装失败/中途退出留下的半成品）不拦截安装：
    // 此时前端往往把该版本显示为"未安装"，没有重装按钮可点，
    // 报"目录已存在"会让安装入口永久卡死，所以一律清理后重装
    if target.exists() {
        remove_version_dir(&target)?;
        emit_log(app, version, "检测到残留的版本目录，已自动清理", "info");
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("创建目录失败: {e}"))?;
    emit_log(
        app,
        version,
        &format!("目标目录: {}", target.display()),
        "info",
    );

    let npm = util::find_npm(settings).ok_or_else(|| {
        "未找到 npm。请先安装 Node.js（自带 npm），或在设置中指定 Node 路径".to_string()
    })?;
    emit_log(
        app,
        version,
        &format!("npm: {} {}", npm.program.display(), npm.args.join(" ")),
        "info",
    );

    let mut cmd = util::spawn_command(&npm.program, &npm.args);
    cmd.arg("install")
        .arg("--prefix")
        .arg(&target)
        .arg(format!("@deepseek-ai/dsh@{version}"))
        .arg("--no-fund")
        .arg("--no-audit")
        // info 级别能看到每个包的 fetch / resolve / extract 过程
        .arg("--loglevel")
        .arg("info")
        .arg("--foreground-scripts");
    let registry = settings.registry.trim();
    if !registry.is_empty() {
        cmd.arg("--registry").arg(registry);
    }
    // 不加 CI/NO_COLOR 等静默环境变量，保留 npm 真实的过程输出
    cmd.env("npm_config_update_notifier", "false");
    // 启动器死亡时中断安装，避免留下孤儿 npm 进程
    util::bind_to_parent_lifetime(&mut cmd);
    let node = util::find_node(settings);
    util::with_node_on_path(&mut cmd, node.as_deref());

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // 把「实际执行的命令 + PATH」写进日志：npm 起不来时（os error 193 这类）
    // 光看一句报错根本定位不了，日志里能直接看到解析到哪个 npm
    let line = util::cmd_line(&cmd);
    crate::diag::op(
        "install",
        &format!(
            "安装 dsh {version}\n  命令: {line}\n  目标: {}\n  node: {}\n  PATH: {}",
            target.display(),
            node.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "（未找到）".into()),
            std::env::var("PATH").unwrap_or_default()
        ),
    );
    let mut child = cmd.spawn().map_err(|e| {
        crate::diag::op("install", &format!("启动 npm 失败：{line}\n  错误: {e}"));
        // 193 = ERROR_BAD_EXE_FORMAT：把脚本当成可执行文件了（Windows 上的经典坑）
        let hint = if e.raw_os_error() == Some(193) {
            "\n  这通常是把没有扩展名的脚本（git-bash 用的 npm / pnpm）当成程序执行了；\n  请确认 Node.js 安装完整（目录里应有 npm.cmd），或在设置里指定 Node 路径"
        } else {
            ""
        };
        format!(
            "启动 npm 失败：{}{hint}\n  命令与 PATH 已写入 logs/install.log（设置 → 打开日志目录）",
            npm.program.display()
        )
    })?;
    emit_log(app, version, "npm 安装中，依赖较多可能需要几分钟…", "info");

    // 两个管道各起一个读取线程，逐行推送日志；stderr 尾部保留用于失败信息
    let stderr_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_pipe_reader(
            app.clone(),
            version.to_string(),
            out,
            "stdout",
            None,
        ));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_pipe_reader(
            app.clone(),
            version.to_string(),
            err,
            "stderr",
            Some(stderr_tail.clone()),
        ));
    }

    // 把 child 交给状态以便取消
    *state.child.lock().unwrap() = Some(child);

    let status: Option<std::process::ExitStatus> = loop {
        {
            let mut guard = state.child.lock().unwrap();
            match guard.as_mut() {
                Some(c) => match c.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => {}
                    Err(e) => {
                        *guard = None;
                        return Err(format!("等待 npm 进程失败: {e}"));
                    }
                },
                // cancel() 已把 child 拿走并 kill
                None => break None,
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    };

    // 排空两个日志管道。取消时只 kill 了直接子进程，--foreground-scripts 起的孙进程
    // （runscript→sh→node）可能仍持有管道写端，无限 join 会永远卡住 → job_version 不清，
    // 之后所有安装都被「已有安装任务正在进行」挡死。给排空设上限，超时即继续（读线程自然游离）。
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for r in readers {
                let _ = r.join();
            }
            let _ = tx.send(());
        });
        let drained = if state.cancelled.load(Ordering::SeqCst) {
            rx.recv_timeout(std::time::Duration::from_secs(3))
        } else {
            // 正常退出：子进程已死，管道很快 EOF，给足时间但仍设硬上限以防万一
            rx.recv_timeout(std::time::Duration::from_secs(60))
        };
        if drained.is_err() {
            emit_log(app, version, "日志管道排空超时（可能有后台脚本仍在收尾）", "info");
        }
    }

    let cancelled = state.cancelled.load(Ordering::SeqCst) && status.is_none();
    let success = matches!(&status, Some(st) if st.success());

    if success {
        let pkg = target.join(crate::installed::PKG_DIR_IN_NODE_MODULES);
        if pkg.join("package.json").is_file() {
            emit_log(app, version, "安装完成 ✔", "info");
            return Ok(format!("已安装到 {}", target.display()));
        }
        let _ = remove_version_dir(&target);
        crate::diag::op(
            "install",
            &format!("npm 退出码 0 但未找到包文件：{}", target.display()),
        );
        return Err(format!(
            "npm 结束但未找到 @deepseek-ai/dsh 包文件，可能 registry 上没有该版本（细节见 logs/install.log）"
        ));
    }

    // 失败或取消：清理半成品目录
    let _ = remove_version_dir(&target);
    if cancelled {
        return Err("安装已取消".into());
    }
    let tail = stderr_tail.lock().unwrap().join("\n");
    let code = status.and_then(|s| s.code());
    crate::diag::op(
        "install",
        &format!("npm 退出码 {code:?}\n  命令: {line}\n  stderr 尾部:\n{tail}"),
    );
    Err(format!(
        "npm 退出码 {:?}{}",
        code,
        if tail.is_empty() {
            format!("\n  细节见 logs/install.log（设置 → 打开日志目录）")
        } else {
            format!("\n{tail}")
        }
    ))
}

fn spawn_pipe_reader<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    version: String,
    pipe: R,
    stream: &'static str,
    tail: Option<Arc<Mutex<Vec<String>>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(pipe);
        for line in reader.lines() {
            let Ok(l) = line else { break };
            // 下载/构建脚本的进度条用 \r 原地重绘，不换行；
            // 按 \r 拆成独立日志行，否则前端会糊成一大段
            for seg in l.split('\r') {
                let seg = seg.trim_end();
                if seg.is_empty() {
                    continue;
                }
                if let Some(t) = &tail {
                    let mut g = t.lock().unwrap();
                    g.push(seg.to_string());
                    let len = g.len();
                    if len > 40 {
                        g.drain(0..len - 40);
                    }
                }
                let _ = app.emit(
                    "install-log",
                    InstallLogEvent {
                        version: version.clone(),
                        line: seg.to_string(),
                        stream: stream.to_string(),
                    },
                );
            }
        }
    })
}

/// 取消当前安装
pub fn cancel(state: &InstallState) {
    state.cancelled.store(true, Ordering::SeqCst);
    if let Some(mut c) = state.child.lock().unwrap().take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// 卸载启动器管理的版本
pub fn uninstall_managed(version: &str) -> Result<(), String> {
    if !util::is_safe_version(version) {
        return Err("非法版本号".into());
    }
    let dir: PathBuf = settings::versions_dir().join(version);
    if !dir.is_dir() {
        return Err("未找到该版本的安装目录（全局/PATH 安装请在终端里自行卸载）".into());
    }
    remove_version_dir(&dir)?;
    Ok(())
}
