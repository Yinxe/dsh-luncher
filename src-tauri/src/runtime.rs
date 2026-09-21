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
    settings::starter_home().join("runtime")
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
    crate::diag::op(
        "runtime",
        &format!(
            "开始安装内置 Node：url={url} 归档={} 运行目录={}",
            runtime_root().join(&fname).display(),
            runtime_dir().display()
        ),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .user_agent(concat!("dsh-starter/", env!("CARGO_PKG_VERSION")))
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
    // Windows 上必须先把写句柄放掉再让解压器读同一个文件（共享模式一旦不允许，
    // Expand-Archive 就会以「文件被占用」失败，而 zip 会留在 runtime/ 里）
    drop(file);
    emit(
        &app,
        &format!("下载完成 {:.1} MB，正在解压…", received as f64 / 1024.0 / 1024.0),
    );
    crate::diag::op(
        "runtime",
        &format!("下载完成：{received}/{total} 字节（{:.1} MB）", received as f64 / 1048576.0),
    );

    if total > 0 && received != total {
        let msg = format!(
            "下载不完整：收到 {received} 字节，服务器声明 {total} 字节。归档已保留在 {}，请重试或更换镜像站",
            archive_path.display()
        );
        crate::diag::op("runtime", &msg);
        return Err(msg);
    }
    match archive_kind(&archive_path) {
        Some(kind) if kind == archive_expected_kind() => {}
        other => {
            // 镜像站挂掉时经常返回 200 + 一个 HTML 错误页，光看后缀名分辨不出来
            let head = head_bytes(&archive_path, 160);
            let msg = format!(
                "下载到的不是 {} 压缩包（文件头检测为 {}，开头内容 {:?}）。归档已保留在 {}，请更换镜像站后重试",
                archive_expected_kind(),
                other.unwrap_or_else(|| "未知".into()),
                head,
                archive_path.display()
            );
            crate::diag::op("runtime", &msg);
            return Err(msg);
        }
    }

    // 解压是重阻塞操作（powershell / tar .output() 对几十 MB 的归档要跑好几秒），
    // 挪到 spawn_blocking，避免卡住 async 命令所在的执行器线程（AGENTS.md 硬性要求）。
    {
        let app_h = app.clone();
        let archive_for_extract = archive_path.clone();
        let archive_for_err = archive_path.clone();
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
            extract(&app_h, &archive_for_extract)
        })
        .await
        .map_err(|e| format!("解压任务失败: {e}"))?
        {
            crate::diag::op("runtime", &format!("解压失败：{e}"));
            crate::diag::op("runtime", &format!("runtime 目录现状：
{}", dir_tree(&runtime_root())));
            return Err(format!(
                "{e}\n归档已保留在 {}（可手动解压，或更换镜像站后重试）。细节见日志 logs/runtime.log",
                archive_for_err.display()
            ));
        }
    }
    if let Err(e) = std::fs::remove_file(&archive_path) {
        crate::diag::op("runtime", &format!("删除归档失败（可忽略）：{e}"));
    }

    // 归档顶层目录是 node-vVER-<plat>，统一改名成 node-vVER
    let extracted = runtime_root().join(format!("node-v{NODE_VERSION}-{plat}"));
    let expect = runtime_dir();
    if extracted != expect && extracted.is_dir() {
        if expect.exists() && runtime_node().is_some() {
            // 目标目录已经能用（如重复安装同一版本）→ 丢掉刚解压的那份
            let _ = std::fs::remove_dir_all(&extracted);
        } else {
            // 目标目录不存在，或存在但里面没有 node 可执行文件（上次装到一半）——
            // 这种情况下必须用新解压的覆盖，否则「解压成功却仍然没有 node」
            if expect.exists() {
                let _ = std::fs::remove_dir_all(&expect);
            }
            std::fs::rename(&extracted, &expect).map_err(|e| {
                crate::diag::op("runtime", &format!("整理目录失败：{e}"));
                format!("整理目录失败: {e}")
            })?;
        }
    }

    if !runtime_installed() {
        let tree = dir_tree(&runtime_root());
        crate::diag::op(
            "runtime",
            &format!("解压完成但没找到 {}；目录现状：\n{tree}", runtime_dir().join(if cfg!(windows) { "node.exe" } else { "bin/node" }).display()),
        );
        let message = format!(
            "解压完成但没找到 node 可执行文件（期望 {}）。目录现状已写入 logs/runtime.log",
            runtime_dir().join(if cfg!(windows) { "node.exe" } else { "bin/node" }).display()
        );
        let _ = app.emit(
            "runtime-finished",
            RuntimeFinishedEvent { ok: false, message: message.clone() },
        );
        return Err(message);
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

/// 期望的归档种类（Windows 是 zip，其它平台是 tar.xz）
fn archive_expected_kind() -> &'static str {
    if cfg!(windows) {
        "zip"
    } else {
        "tar.xz"
    }
}

/// 从文件头认归档种类：镜像站 404/被网关拦截时常常返回 200 + HTML，只看后缀会误判
fn archive_kind(path: &Path) -> Option<String> {
    let head = head_bytes(path, 8);
    let b = head.as_bytes();
    if b.starts_with(b"PK") {
        Some("zip".into())
    } else if b.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Some("tar.xz".into())
    } else if b.starts_with(&[0x1F, 0x8B]) {
        Some("tar.gz".into())
    } else if head.is_empty() {
        None
    } else {
        Some(format!("未知（开头 {head:?}）"))
    }
}

/// 读文件开头若干字节（非 UTF-8 也能看个大概），仅用于留痕与报错
fn head_bytes(path: &Path, n: usize) -> String {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf).unwrap_or(0);
    buf.truncate(read);
    String::from_utf8_lossy(&buf).replace(['\r', '\n'], " ")
}

/// 目录两层快照（写日志用）：出问题时能一眼看出「解压到哪去了 / 顶层目录叫什么」
fn dir_tree(root: &Path) -> String {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return format!("（读不到 {}）", root.display());
    };
    let mut top: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    top.sort();
    for p in top.iter().take(40) {
        let is_dir = p.is_dir();
        out.push(format!(
            "  {}{}",
            p.file_name().unwrap_or_default().to_string_lossy(),
            if is_dir { "/" } else { "" }
        ));
        if is_dir {
            if let Ok(sub) = std::fs::read_dir(p) {
                let mut names: Vec<_> = sub
                    .flatten()
                    .map(|e| {
                        let path = e.path();
                        format!(
                            "    {}{}",
                            e.file_name().to_string_lossy(),
                            if path.is_dir() { "/" } else { "" }
                        )
                    })
                    .collect();
                names.sort();
                out.extend(names.into_iter().take(15));
            }
        }
    }
    if out.is_empty() {
        return format!("（{} 是空目录）", root.display());
    }
    out.join("\n")
}

/// 解压归档。Windows 依次尝试「系统自带 tar（bsdtar，能解 zip 且对长路径更宽容）」
/// →「PowerShell Expand-Archive」，任一步成功即返回；全部失败才报错，并把每次尝试的
/// 命令与 stderr 写进 logs/runtime.log —— 之前只试一条路，失败了用户手上什么线索都没有。
fn extract(app: &tauri::AppHandle, archive: &Path) -> Result<(), String> {
    let root = runtime_root();
    std::fs::create_dir_all(&root).map_err(|e| format!("创建目录失败: {e}"))?;
    let archive_s = archive.to_string_lossy().into_owned();
    let root_s = root.to_string_lossy().into_owned();

    let mut attempts: Vec<(&str, std::process::Command)> = Vec::new();
    if cfg!(windows) {
        let mut tar_cmd = crate::util::hidden_command("tar");
        tar_cmd.args(["-xf", &archive_s, "-C", &root_s]);
        attempts.push(("系统 tar", tar_cmd));

        // Expand-Archive：路径落到单引号字符串里，' 必须转义成 ''，
        // 否则用户名/目录含 ' 会破坏引号并注入任意 PowerShell。
        let psq = |s: &str| s.replace('\'', "''");
        let ps = format!(
            "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
            psq(&archive_s),
            psq(&root_s)
        );
        let mut ps_cmd = crate::util::hidden_command("powershell");
        ps_cmd.args(["-NoProfile", "-Command", &ps]);
        attempts.push(("PowerShell Expand-Archive", ps_cmd));
    } else {
        let mut tar_cmd = crate::util::hidden_command("tar");
        tar_cmd.args(["-xJf", &archive_s, "-C", &root_s]);
        attempts.push(("tar -xJf", tar_cmd));
    }

    let total = attempts.len();
    let mut failures: Vec<String> = Vec::new();
    for (i, (label, mut cmd)) in attempts.into_iter().enumerate() {
        let line = crate::util::cmd_line(&cmd);
        crate::diag::op("runtime", &format!("解压尝试 {}/{total} [{label}]: {line}", i + 1));
        match cmd.output() {
            Ok(out) if out.status.success() => {
                crate::diag::op("runtime", &format!("[{label}] 解压成功"));
                emit(app, &format!("$ {label} 解压完成，校验 node 可执行文件…"));
                return Ok(());
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let err = if err.is_empty() {
                    String::from_utf8_lossy(&out.stdout).trim().to_string()
                } else {
                    err
                };
                crate::diag::op(
                    "runtime",
                    &format!("[{label}] 失败 exit={:?}: {err}", out.status.code()),
                );
                emit(app, &format!("$ {label} 解压失败：{err}"));
                failures.push(format!("{label}（exit={:?}）：{err}", out.status.code()));
            }
            Err(e) => {
                crate::diag::op("runtime", &format!("[{label}] 无法执行：{e}"));
                emit(app, &format!("$ {label} 不可用：{e}"));
                failures.push(format!("{label} 无法执行：{e}"));
            }
        }
    }
    Err(format!("解压失败（{}）：{}", archive.display(), failures.join("；")))
}
