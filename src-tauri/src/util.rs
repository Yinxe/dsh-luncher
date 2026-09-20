use crate::settings::Settings;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 可直接执行的 npm 调用（可能是 npm 本体，也可能是 node + npm-cli.js）
pub struct NpmInvocation {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// DSH_HOME 是进程级环境变量，凡是临时改它的测试都必须串行执行；
/// 各模块测试统一引用这把锁，避免并行互踩。
#[cfg(test)]
pub(crate) static DSH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 通道健康度 / 探测延迟 / API 额度都是**进程级 static**，而网络用例本身又会写它们。
/// 凡是会发网络请求或读这些状态的测试都拿这把锁串行执行，否则用例之间会互相污染
/// （一个用例把某条通道熔断，另一个用例的探测就换了通道）。
#[cfg(test)]
pub(crate) static NET_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    } else {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// 在 PATH 中查找可执行文件
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let path = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let p = dir.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// 构建 Command，Windows 上自动包装 .cmd/.bat
pub fn spawn_command(program: &Path, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let is_script = program
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
            .unwrap_or(false);
        if is_script {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(program);
            for a in args {
                c.arg(a);
            }
            return c;
        }
    }
    let mut c = Command::new(program);
    for a in args {
        c.arg(a);
    }
    c
}

/// 探测 node 可执行文件：覆盖路径 → 按来源解析
/// auto：系统优先，缺失回退内置运行时；system：仅系统；runtime：仅内置
pub fn find_node(settings: &Settings) -> Option<PathBuf> {
    let o = settings.node_path.trim();
    if !o.is_empty() {
        let p = PathBuf::from(o);
        if p.is_file() {
            return Some(p);
        }
    }
    match settings.node_source.as_str() {
        "system" => find_system_node(),
        "runtime" => crate::runtime::runtime_node(),
        _ => find_system_node().or_else(|| crate::runtime::runtime_node()),
    }
}

fn find_system_node() -> Option<PathBuf> {
    if let Some(p) = which("node") {
        return Some(p);
    }
    if cfg!(windows) {
        let candidates = [
            r"C:\Program Files\nodejs\node.exe",
            r"C:\Program Files (x86)\nodejs\node.exe",
        ];
        candidates.iter().map(PathBuf::from).find(|p| p.is_file())
    } else {
        let mut candidates: Vec<PathBuf> = [
            "/usr/local/bin/node",
            "/usr/bin/node",
            "/opt/homebrew/bin/node",
            "/snap/bin/node",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        if let Some(home) = home_dir() {
            let nvm = home.join(".nvm/versions/node");
            if let Ok(rd) = std::fs::read_dir(&nvm) {
                let mut dirs: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path().join("bin/node"))
                    .filter(|p| p.is_file())
                    .collect();
                dirs.sort_by(|a, b| {
                    crate::semver::compare(&file_version(b), &file_version(a))
                });
                if let Some(latest) = dirs.into_iter().next() {
                    candidates.push(latest);
                }
            }
        }
        candidates.into_iter().find(|p| p.is_file())
    }
}

fn file_version(p: &Path) -> String {
    // .../versions/v24.15.0/bin/node → 24.15.0
    p.parent()
        .and_then(|bin| bin.parent())
        .and_then(|vdir| vdir.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("0")
        .trim_start_matches('v')
        .to_string()
}

/// 探测 npm：PATH 中的 npm → node 同目录 → npm-cli.js 通过 node 运行
pub fn find_npm(settings: &Settings) -> Option<NpmInvocation> {
    if let Some(npm) = which("npm") {
        return Some(NpmInvocation {
            program: npm,
            args: vec![],
        });
    }
    let node = find_node(settings)?;
    let bin_dir = node.parent()?;

    if cfg!(windows) {
        let cli = bin_dir.join("node_modules/npm/bin/npm-cli.js");
        if cli.is_file() {
            return Some(NpmInvocation {
                program: node,
                args: vec![cli.to_string_lossy().into_owned()],
            });
        }
        None
    } else {
        let npm_sh = bin_dir.join("npm");
        if npm_sh.is_file() {
            return Some(NpmInvocation {
                program: npm_sh,
                args: vec![],
            });
        }
        // nvm 布局：<版本目录>/lib/node_modules/npm/bin/npm-cli.js
        let cli = bin_dir.join("../lib/node_modules/npm/bin/npm-cli.js");
        if cli.is_file() {
            return Some(NpmInvocation {
                program: node,
                args: vec![cli
                    .canonicalize()
                    .unwrap_or(cli)
                    .to_string_lossy()
                    .into_owned()],
            });
        }
        None
    }
}

/// 运行短命令并捕获 stdout（带超时，超时或失败返回 None）
pub fn run_captured(program: &Path, args: &[String], timeout: Duration) -> Option<String> {
    let mut cmd = spawn_command(program, args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = BufReader::new(stdout).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).trim().to_string()
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return reader.join().ok().filter(|s| !s.is_empty());
            }
            Ok(Some(_)) => return None,
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 运行短命令并捕获 stdout+stderr（可指定工作目录，带超时）。
/// 与非 `_in` 版本不同，这里**不因退出码非 0 而丢弃输出**：git 之类的命令
/// 失败信息本身就是要展示给用户的内容。返回 (是否成功, 合并输出)。
pub fn run_captured_in(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    envs: &[(String, String)],
    timeout: Duration,
) -> Option<(bool, String)> {
    let mut cmd = spawn_command(program, args);
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let stderr = child.stderr.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = BufReader::new(stdout).read_to_end(&mut buf);
        let mut err = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut err);
        (
            String::from_utf8_lossy(&buf).trim().to_string(),
            String::from_utf8_lossy(&err).trim().to_string(),
        )
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let (out, err) = reader.join().ok()?;
                let text = if status.success() || err.is_empty() { out } else { err };
                return Some((status.success(), text));
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 追加 PATH（把 node 目录放进子进程 PATH，保证 npm 生命周期脚本可用）
pub fn with_node_on_path(cmd: &mut Command, node: Option<&Path>) {
    if let Some(dir) = node.and_then(|n| n.parent()) {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let existing = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{}{sep}{existing}", dir.display()));
    }
}

/// Linux：把子进程与启动器生命周期绑定 —— 启动器无论以何种方式死亡（含 SIGKILL），
/// 内核都会立即 SIGKILL 该子进程，避免孤儿 dsh。
#[cfg(target_os = "linux")]
pub fn bind_to_parent_lifetime(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // 竞态兜底：父进程在 prctl 生效前就已退出
            if libc::getppid() == 1 {
                libc::_exit(1);
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub fn bind_to_parent_lifetime(_cmd: &mut Command) {}

/// 校验版本号字符串，防止路径穿越
/// Windows：把 PowerShell 脚本编码为 -EncodedCommand 参数（base64 of UTF-16LE）。
/// 编码命令不经 shell 层解释，彻底规避引号转义问题。
#[cfg(windows)]
pub fn ps_encoded_command(script: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn is_safe_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v != "."
        && v != ".."
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

/// 把参数按 shell 规则加单引号
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        // 空参数若不显式加引号会在拼接时整个消失，导致 argv 错位
        return "''".to_string();
    }
    if s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '/' | '.' | '-' | '_' | '=' | ':' | ',' | '@' | '+')
    }) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::{is_safe_version, shell_quote};

    #[test]
    fn safe_versions() {
        assert!(is_safe_version("0.1.5-rc.2"));
        assert!(!is_safe_version("../etc"));
        assert!(!is_safe_version(""));
        assert!(!is_safe_version("a/b"));
    }

    #[test]
    fn shell_quote_preserves_empty_arg() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("a/b-1.2"), "a/b-1.2");
        assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
    }
}
