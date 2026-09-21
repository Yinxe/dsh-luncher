use serde::Serialize;
// update()/finalize() 是 trait 方法，必须在作用域内
use sha2::Digest as _;
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

/// 内置 Node 归档的官方 SHA-256（来自 https://nodejs.org/dist/vNODE_VERSION/SHASUMS256.txt）。
/// 镜像站地址用户可以随便改，完整性不能跟着一起放下：装进 dsh 运行时的 node 必须
/// 先用这里锚定的官方哈希验过才解压。升级 NODE_VERSION 时必须换掉这张表
/// （`embedded_node_hashes_match_builds` 会联网核对，CI 防错）。
pub const NODE_SHA256: [(&str, &str); 6] = [
    ("node-v24.15.0-darwin-arm64.tar.xz", "af5cfaeafe603aaf7599f287fd9d100bb41f16794f49788fa59dd3f25546930f"),
    ("node-v24.15.0-darwin-x64.tar.xz", "5d627245b9f53cb2512cc21b7aa6aad693106affadd91e0c8f42d600fb7ba444"),
    ("node-v24.15.0-linux-arm64.tar.xz", "f3d5a797b5d210ce8e2cb265544c8e482eaedcb8aa409a8b46da7e8595d0dda0"),
    ("node-v24.15.0-linux-x64.tar.xz", "472655581fb851559730c48763e0c9d3bc25975c59d518003fc0849d3e4ba0f6"),
    ("node-v24.15.0-win-arm64.zip", "c9eb7402eda26e2ba7e44b6727fc85a8de56c5095b1f71ebd3062892211aa116"),
    ("node-v24.15.0-win-x64.zip", "cc5149eabd53779ce1e7bdc5401643622d0c7e6800ade18928a767e940bb0e62"),
];

/// 当前平台归档对应的官方哈希；架构不支持时返回 None（上游 install 会先报不支持）。
fn expected_node_sha256(fname: &str) -> Option<&'static str> {
    NODE_SHA256.iter().find(|(n, _)| *n == fname).map(|(_, h)| *h)
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
    // 完整性锚点：内置官方哈希只认文件名，认不出来说明这张表没跟上平台/版本
    // —— 宁可拒绝安装，也绝不解压一份没验过的 node。
    let expected_sha = expected_node_sha256(&fname).ok_or_else(|| {
        format!(
            "内部错误：内置哈希表里没有 {fname} 的官方 SHA-256（升级 NODE_VERSION 时漏改 runtime.rs），已拒绝安装"
        )
    })?;
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
    // 边下边算：50MB 的归档不重复读第二遍
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("下载中断: {e}"))?
    {
        file.write_all(&chunk).map_err(|e| format!("写文件失败: {e}"))?;
        hasher.update(&chunk);
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
    // SHA-256 校验：镜像站地址是用户可改的（设置里随便填），被投毒或被网关替换成
    // 别的载荷时魔数与长度都拦不住 —— 只有和官方锚定值比对才算数。校验失败**删档**：
    // 一份「看着像 node 实际不是」的归档留在磁盘上只会等着被手动解压。
    let got_sha = format!("{:x}", hasher.finalize());
    if !got_sha.eq_ignore_ascii_case(expected_sha) {
        let _ = std::fs::remove_file(&archive_path);
        let msg = format!(
            "SHA-256 校验不通过，已删除该归档：\n  期望（官方）: {expected_sha}\n  实际: {got_sha}\n\
             镜像站给出的文件不是官方 {fname}（被篡改、劫持或版本目录不同步都可能造成）。\
             请在设置里换回默认镜像站（或填 https://nodejs.org/dist）后重试"
        );
        crate::diag::op("runtime", &msg);
        return Err(msg);
    }
    emit(&app, "$ SHA-256 校验通过 ✔");
    match archive_kind(&archive_path) {
        Some(kind) if kind == archive_expected_kind() => {}
        other => {
            // 镜像站挂掉时经常返回 200 + 一个 HTML 错误页，光看后缀名分辨不出来
            let got = match other.as_deref() {
                None => "空文件".to_string(),
                Some("未知") => "认不出的格式（镜像站常见：200 + 一个 HTML 错误页）".to_string(),
                Some(k) => k.to_string(),
            };
            let msg = format!(
                "下载到的不是 {} 压缩包（判定：{got}；文件头 {}）。归档已保留在 {}，请更换镜像站后重试",
                archive_expected_kind(),
                head_preview(&archive_path, 32),
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

/// 从文件头认归档种类：镜像站 404/被网关拦截时常常返回 200 + HTML，只看后缀会误判。
///
/// ⚠️ 必须比**原始字节**：xz 的魔数首字节是 `0xFD`，不是合法 UTF-8 —— 一旦先过
/// `String::from_utf8_lossy`，它就会被替换成 `U+FFFD`，`starts_with` 永远不可能命中，
/// 结果是把一份好端端的 `node-*.tar.xz` 判成「不是 tar.xz」，安装直接失败。
fn archive_kind(path: &Path) -> Option<String> {
    let head = head_raw(path, 8);
    if head.starts_with(b"PK") {
        Some("zip".into())
    } else if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Some("tar.xz".into())
    } else if head.starts_with(&[0x1F, 0x8B]) {
        Some("tar.gz".into())
    } else if head.is_empty() {
        None
    } else {
        Some("未知".into())
    }
}

/// 读文件开头若干字节（原始字节，不做任何编码转换 —— 魔数比较只能用这个）
fn head_raw(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf).unwrap_or(0);
    buf.truncate(read);
    buf
}

/// 给用户看的开头预览：十六进制 + 可打印字符。
///
/// 别把原始字节直接当文本塞进提示里 —— HTML 错误页还算能读，压缩包就是一屏乱码。
fn head_preview(path: &Path, n: usize) -> String {
    let buf = head_raw(path, n);
    if buf.is_empty() {
        return "（读不到内容）".into();
    }
    let hex = buf.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
    let ascii: String = buf
        .iter()
        .map(|b| if (0x20..0x7f).contains(b) { *b as char } else { '.' })
        .collect();
    format!("{hex}  ({ascii})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dsh-runtime-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// 回归：xz 魔数首字节 `0xFD` 不是合法 UTF-8 —— 先做 lossy 字符串化再比较，
    /// 会把一份合法的 `node-*.tar.xz` 判成「不是 tar.xz」，内置 Node 直接装不上。
    #[test]
    fn xz_magic_is_read_from_raw_bytes() {
        let p = sample(
            "node-v24.15.0-linux-x64.tar.xz",
            &[0xFD, b'7', b'z', b'X', b'Z', 0x00, 0x00, 0x04, 0xE6],
        );
        assert_eq!(archive_kind(&p).as_deref(), Some("tar.xz"));

        let preview = head_preview(&p, 8);
        assert!(
            preview.starts_with("fd 37 7a 58 5a 00"),
            "预览应是十六进制：{preview}"
        );
        assert!(
            !preview.contains('\u{fffd}'),
            "预览里不该出现替换字符：{preview}"
        );
    }

    /// 其余形态照样认得：gzip、zip、镜像站的 HTML 错误页、空文件
    #[test]
    fn other_archive_kinds_and_html_are_classified() {
        let gz = sample("a.tar.gz", &[0x1F, 0x8B, 0x08, 0x00]);
        assert_eq!(archive_kind(&gz).as_deref(), Some("tar.gz"));
        let zip = sample("a.zip", b"PK\x03\x04rest");
        assert_eq!(archive_kind(&zip).as_deref(), Some("zip"));
        let html = sample("b.tar.xz", b"<!DOCTYPE html><title>404</title>");
        assert_eq!(archive_kind(&html).as_deref(), Some("未知"));
        let empty = sample("c.tar.xz", b"");
        assert_eq!(archive_kind(&empty), None);
    }

    /// 每个受支持的平台×架构组合都必须有锚定哈希 —— install() 只按文件名查表，
    /// 表里缺一行就等于那个平台永远装不上（宁缺毋滥是故意的，但别无声无息）。
    #[test]
    fn every_supported_build_has_a_pinned_hash() {
        for (os, arch, ext) in [
            ("linux", "x64", "tar.xz"),
            ("linux", "arm64", "tar.xz"),
            ("darwin", "x64", "tar.xz"),
            ("darwin", "arm64", "tar.xz"),
            ("win", "x64", "zip"),
            ("win", "arm64", "zip"),
        ] {
            let fname = format!("node-v{NODE_VERSION}-{os}-{arch}.{ext}");
            let sha = expected_node_sha256(&fname);
            assert!(sha.is_some(), "锚定表缺少 {fname}");
            let sha = sha.unwrap();
            assert_eq!(
                sha.len(), 64,
                "{fname} 的哈希不是 64 个十六进制字符: {sha}"
            );
            assert!(
                sha.chars().all(|c| c.is_ascii_hexdigit()),
                "{fname} 的哈希含非法字符: {sha}"
            );
        }
    }

    /// 与 nodejs.org 官方 SHASUMS256.txt 对账：锚定表抄错一个字符都会在用户
    /// 装机时爆炸，这条测试把它拦在提交阶段。外网不可达时跳过（CI 离线不红）。
    #[test]
    fn embedded_hashes_match_official_shasums() {
        let _guard = crate::util::NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let url = format!("https://nodejs.org/dist/v{NODE_VERSION}/SHASUMS256.txt");
        let body = match tauri::async_runtime::block_on(async {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .ok()?
                .get(&url)
                .send()
                .await
                .ok()?
                .text()
                .await
                .ok()
        }) {
            Some(b) => b,
            None => {
                eprintln!("跳过：拉不到 {url}（离线环境）");
                return;
            }
        };
        for line in body.lines() {
            let mut it = line.split_whitespace();
            let (Some(sha), Some(file)) = (it.next(), it.next()) else { continue };
            if let Some(pinned) = expected_node_sha256(file) {
                assert_eq!(
                    pinned, sha,
                    "锚定表与官方不符：{file} 官方为 {sha}，内置为 {pinned}"
                );
            }
        }
    }
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
