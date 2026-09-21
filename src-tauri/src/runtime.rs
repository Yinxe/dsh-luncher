use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

use crate::settings::{self, Settings};

/// 内置 Node 运行时版本（LTS）。
/// 不得低于 v24.2.0：dsh 的 bin.js 入口判断 `if (import.meta.main)` 依赖该 API，
/// 更老的 Node 下 dsh 会静默退出（exit 0、无输出、无子进程）。
pub const NODE_VERSION: &str = "24.15.0";

pub fn runtime_root() -> PathBuf {
    settings::launcher_home().join("runtime")
}

pub fn runtime_dir() -> PathBuf {
    runtime_root().join(format!("node-v{NODE_VERSION}"))
}

/// 内置 node 可执行文件
pub fn runtime_node() -> Option<PathBuf> {
    let rel = if cfg!(windows) { "node.exe" } else { "bin/node" };
    let p = runtime_dir().join(rel);
    p.is_file().then_some(p)
}

pub fn runtime_installed() -> bool {
    runtime_node().is_some()
}

fn arch_key() -> Result<&'static str, String> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        other => Err(format!("不支持的 CPU 架构: {other}")),
    }
}

fn platform_key() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "win",
        other => return Err(format!("不支持的系统: {other}")),
    };
    Ok(format!("{}-{}", os, arch_key()?))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProgressEvent {
    pub received: u64,
    pub total: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeFinishedEvent {
    pub ok: bool,
    pub message: String,
}

fn emit(app: &tauri::AppHandle, line: &str) {
    let _ = app.emit("runtime-log", line.to_string());
}

/// 下载并安装内置 Node 运行时；已存在则直接返回
pub async fn install(app: tauri::AppHandle, settings: &Settings) -> Result<String, String> {
    if runtime_installed() {
        return Ok(format!("内置 Node v{NODE_VERSION} 已就绪"));
    }
    let mirror = {
        let m = settings.node_mirror.trim().trim_end_matches('/');
        if m.is_empty() {
            "https://npmmirror.com/mirrors/node".to_string()
        } else {
            m.to_string()
        }
    };
    let plat = platform_key()?;
    let ext = if cfg!(windows) { "zip" } else { "tar.xz" };
    let fname = format!("node-v{NODE_VERSION}-{plat}.{ext}");
    let url = format!("{mirror}/v{NODE_VERSION}/{fname}");
    emit(&app, &format!("$ 镜像站: {mirror}"));
    emit(&app, &format!("$ 平台: {plat} · 目标: runtime/node-v{NODE_VERSION}"));
    emit(&app, &format!("$ 下载 {fname}"));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .user_agent(concat!("dsh-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败（{url}）: {e}，可在设置中更换 Node 镜像站"))?;
    if !resp.status().is_success() {
        return Err(format!("下载返回 {}（{url}），可在设置中更换 Node 镜像站", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);

    let archive_path = runtime_root().join(&fname);
    std::fs::create_dir_all(runtime_root()).map_err(|e| format!("创建目录失败: {e}"))?;
    let mut file = std::fs::File::create(&archive_path).map_err(|e| format!("写文件失败: {e}"))?;

    let mut received: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut last_pct: u64 = 0;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("下载中断: {e}"))?
    {
        file.write_all(&chunk).map_err(|e| format!("写文件失败: {e}"))?;
        received += chunk.len() as u64;
        if received - last_emit >= 512 * 1024 || received == total {
            let _ = app.emit(
                "runtime-progress",
                RuntimeProgressEvent { received, total },
            );
            if total > 0 {
                let pct = received * 100 / total;
                if pct >= last_pct + 10 || received == total {
                    emit(
                        &app,
                        &format!(
                            "⬇ 下载中 {pct:>3}% ({:.1} / {:.1} MB)",
                            received as f64 / 1048576.0,
                            total as f64 / 1048576.0
                        ),
                    );
                    last_pct = pct;
                }
            }
            last_emit = received;
        }
    }
    let _ = file.flush();
    emit(
        &app,
        &format!("下载完成 {:.1} MB，正在解压…", received as f64 / 1024.0 / 1024.0),
    );

    // 解压是重阻塞操作（powershell / tar .output() 对几十 MB 的归档要跑好几秒），
    // 挪到 spawn_blocking，避免卡住 async 命令所在的执行器线程（AGENTS.md 硬性要求）。
    {
        let app_h = app.clone();
        let archive_for_extract = archive_path.clone();
        tauri::async_runtime::spawn_blocking(move || extract(&app_h, &archive_for_extract))
            .await
            .map_err(|e| format!("解压任务失败: {e}"))??;
    }
    let _ = std::fs::remove_file(&archive_path);

    // 归档顶层目录是 node-vVER-<plat>，统一改名成 node-vVER
    let extracted = runtime_root().join(format!("node-v{NODE_VERSION}-{plat}"));
    let expect = runtime_dir();
    if extracted != expect && extracted.is_dir() {
        if expect.exists() {
            let _ = std::fs::remove_dir_all(&extracted);
        } else {
            std::fs::rename(&extracted, &expect).map_err(|e| format!("整理目录失败: {e}"))?;
        }
    }

    if !runtime_installed() {
        let _ = app.emit(
            "runtime-finished",
            RuntimeFinishedEvent {
                ok: false,
                message: "解压完成但未找到 node 可执行文件".into(),
            },
        );
        return Err("解压完成但未找到 node 可执行文件".into());
    }

    // 版本升级后清理遗留的旧版运行时目录（如 node-v22.14.0），避免长期占用磁盘
    if let Ok(rd) = std::fs::read_dir(runtime_root()) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir()
                && p != runtime_dir()
                && e.file_name().to_string_lossy().starts_with("node-v")
            {
                let _ = std::fs::remove_dir_all(&p);
                emit(&app, &format!("$ 已清理旧版运行时: {}", e.file_name().to_string_lossy()));
            }
        }
    }

    let msg = format!("内置 Node v{NODE_VERSION} 安装完成：{}", runtime_dir().display());
    emit(&app, &msg);
    let _ = app.emit(
        "runtime-finished",
        RuntimeFinishedEvent { ok: true, message: msg.clone() },
    );
    Ok(msg)
}

fn extract(app: &tauri::AppHandle, archive: &Path) -> Result<(), String> {
    let root = runtime_root();
    std::fs::create_dir_all(&root).map_err(|e| format!("创建目录失败: {e}"))?;
    if cfg!(windows) {
        // zip → PowerShell 展开。路径落到单引号字符串里，' 必须转义成 ''，
        // 否则用户名/目录含 ' 会破坏引号并注入任意 PowerShell。
        let psq = |s: &str| s.replace('\'', "''");
        let ps = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            psq(&archive.to_string_lossy()),
            psq(&root.to_string_lossy())
        );
        let out = crate::util::hidden_command("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .map_err(|e| format!("调用 PowerShell 失败: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "解压失败: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    } else {
        let out = crate::util::hidden_command("tar")
            .args(["-xJf", &archive.to_string_lossy(), "-C", &root.to_string_lossy()])
            .output()
            .map_err(|e| format!("调用 tar 失败（需要 xz 支持）: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "解压失败: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    emit(app, "$ 解压完成，校验 node 可执行文件…");
    Ok(())
}
