use serde::Serialize;
use std::io::BufRead;
use std::path::PathBuf;
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
    force: bool,
) -> Result<(), String> {
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
        let result = run_install(&app, &settings2, &version2, force, &state2);
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

fn run_install(
    app: &AppHandle,
    settings: &Settings,
    version: &str,
    force: bool,
    state: &InstallState,
) -> Result<String, String> {
    let versions_dir = settings::versions_dir();
    let target = versions_dir.join(version);

    if target.exists() {
        if force {
            std::fs::remove_dir_all(&target).map_err(|e| format!("清理旧目录失败: {e}"))?;
        } else {
            return Err("该版本目录已存在（如需重装请使用重装按钮）".into());
        }
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
    let node = util::find_node(&settings.node_path);
    util::with_node_on_path(&mut cmd, node.as_deref());

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("启动 npm 失败: {e}"))?;
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

    for r in readers {
        let _ = r.join();
    }

    let cancelled = state.cancelled.load(Ordering::SeqCst) && status.is_none();
    let success = matches!(&status, Some(st) if st.success());

    if success {
        let pkg = target.join(crate::installed::PKG_DIR_IN_NODE_MODULES);
        if pkg.join("package.json").is_file() {
            emit_log(app, version, "安装完成 ✔", "info");
            return Ok(format!("已安装到 {}", target.display()));
        }
        let _ = std::fs::remove_dir_all(&target);
        return Err("npm 结束但未找到 @deepseek-ai/dsh 包文件，可能 registry 上没有该版本".into());
    }

    // 失败或取消：清理半成品目录
    let _ = std::fs::remove_dir_all(&target);
    if cancelled {
        return Err("安装已取消".into());
    }
    let tail = stderr_tail.lock().unwrap().join("\n");
    let code = status.and_then(|s| s.code());
    Err(format!(
        "npm 退出码 {:?}{}",
        code,
        if tail.is_empty() {
            String::new()
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
            match line {
                Ok(l) => {
                    let l = l.trim_end();
                    if l.is_empty() {
                        continue;
                    }
                    if let Some(t) = &tail {
                        let mut g = t.lock().unwrap();
                        g.push(l.to_string());
                        let len = g.len();
                        if len > 40 {
                            g.drain(0..len - 40);
                        }
                    }
                    let _ = app.emit(
                        "install-log",
                        InstallLogEvent {
                            version: version.clone(),
                            line: l.to_string(),
                            stream: stream.to_string(),
                        },
                    );
                }
                Err(_) => break,
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
    std::fs::remove_dir_all(&dir).map_err(|e| format!("删除目录失败: {e}"))?;
    Ok(())
}
