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

/// 在 PATH 中查找可执行文件。
///
/// Windows 上的顺序很关键：官方 Node.js for Windows 会在同一个目录里同时放
/// `npm`（**给 git-bash 用的 POSIX shell 脚本**）、`npm.cmd`、`npm.ps1`，pnpm/npx
/// 同理。若先命中无扩展名的那个，CreateProcess 会报
/// 「不是有效的 Win32 应用程序 (os error 193)」——表现为「npm 显示未装」「安装 dsh 失败」。
/// 所以：.exe / .cmd / .bat 排前面，无扩展名的候选必须是真正的 PE 可执行文件才认。
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    let path = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            let p = dir.join(format!("{name}{ext}"));
            if !p.is_file() {
                continue;
            }
            if ext.is_empty() && !is_directly_executable(&p) {
                continue;
            }
            return Some(p);
        }
    }
    None
}

/// 这个文件能不能直接交给 CreateProcess：只有真 PE（MZ 头）才行。
/// 无扩展名的文本脚本（git-bash 用的 npm / pnpm / npx）一律不算。
/// 非 Windows 上恒为 true（有无扩展名都能 exec）。
pub fn is_directly_executable(p: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::io::Read;
        match std::fs::File::open(p) {
            Ok(mut f) => {
                let mut magic = [0u8; 2];
                f.read_exact(&mut magic).is_ok() && &magic == b"MZ"
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        true
    }
}

/// 隐藏控制台窗口（Windows）。
///
/// **所有后台命令都必须走这个标志**：启动器是 GUI 进程，起控制台子进程（git / npm /
/// node / powershell / tar / taskkill…）时不设 `CREATE_NO_WINDOW`，每一个都会在屏幕上
/// 弹一个黑框。进插件页、探渠道、测加速时一次会起十几个 git，用户看到的就是
/// 「一进页面一堆黑窗口」。
///
/// 唯一的例外是**用户明确要一个终端窗口**的场景（「在新终端里启动 dsh」，
/// 见 `launcher::spawn_terminal`）—— 那里必须保持可见，别顺手加进来。
pub fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// 后台命令的统一构造入口：Windows 上默认隐藏控制台窗口。
/// 直接写 `Command::new` 会漏掉 `CREATE_NO_WINDOW`，新增代码请用这个。
pub fn hidden_command<P: AsRef<std::ffi::OsStr>>(program: P) -> Command {
    let mut c = Command::new(program);
    hide_window(&mut c);
    c
}

/// Windows：把「无扩展名的 POSIX 脚本」纠正到同目录的 .cmd / .bat / .exe。
///
/// 调用方可能从设置、缓存或旧日志里拿到 `<dir>\npm` 这种路径（真正能用的是
/// `npm.cmd`）。放任不管就是 os error 193，且报错信息对用户毫无指导意义。
#[cfg(windows)]
fn resolve_windows_program(program: &Path) -> PathBuf {
    if program.extension().is_some() || is_directly_executable(program) {
        return program.to_path_buf();
    }
    for ext in ["cmd", "bat", "exe"] {
        let cand = program.with_extension(ext);
        if cand.is_file() {
            crate::diag::op(
                "app",
                &format!(
                    "{} 不是可执行文件（多半是 git-bash 用的脚本），改用它旁边的 {}",
                    program.display(),
                    cand.display()
                ),
            );
            return cand;
        }
    }
    program.to_path_buf()
}

/// 把一条命令拼成可读的一行（写入日志用；Command 没有公开的 Display）
pub fn cmd_line(cmd: &Command) -> String {
    let mut s = cmd.get_program().to_string_lossy().into_owned();
    for a in cmd.get_args() {
        s.push(' ');
        let a = a.to_string_lossy();
        if a.contains(' ') {
            s.push('"');
            s.push_str(&a);
            s.push('"');
        } else {
            s.push_str(&a);
        }
    }
    s
}

/// 构建 Command，Windows 上自动包装 .cmd/.bat，并隐藏控制台窗口。
/// 只用于**后台**命令；要让用户看到终端窗口请直接用 `Command::new`（见 `hide_window`）。
pub fn spawn_command(program: &Path, args: &[String]) -> Command {
    #[cfg(windows)]
    let program = &resolve_windows_program(program);
    #[cfg(windows)]
    {
        let is_script = program
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
            .unwrap_or(false);
        if is_script {
            let mut c = hidden_command("cmd");
            c.arg("/C").arg(program);
            for a in args {
                c.arg(a);
            }
            return c;
        }
    }
    let mut c = hidden_command(program);
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
    let node = find_node(settings);
    // Windows 优先走 `node <npm-cli.js>`：一次绕开 npm / npm.cmd / npm.ps1 三种包装。
    // 无扩展名的 `npm` 是给 git-bash 的 POSIX 脚本，直接 CreateProcess 会报
    // 「不是有效的 Win32 应用程序 (os error 193)」，界面表现为「npm 未装」+ 安装必失败。
    // 官方 msi 与 zip 发行版都把 npm 放在 <node 目录>/node_modules/npm/bin/npm-cli.js。
    if cfg!(windows) {
        if let Some((n, cli)) = node.as_ref().and_then(|n| {
            let cli = n.parent()?.join("node_modules/npm/bin/npm-cli.js");
            cli.is_file().then(|| (n.clone(), cli))
        }) {
            return Some(NpmInvocation {
                program: n,
                args: vec![cli.to_string_lossy().into_owned()],
            });
        }
    }
    if let Some(npm) = which("npm") {
        return Some(NpmInvocation {
            program: npm,
            args: vec![],
        });
    }
    let node = node?;
    let bin_dir = node.parent()?;

    if cfg!(windows) {
        // 兜底：npm-cli.js 就在 node 目录下（正常布局；上面的优先分支已覆盖）
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

/// 把用户在一行里输入的启动参数切成 argv：按空白分组，但尊重单 / 双引号，
/// 让带空格的参数值（如 `--title "a b"`）能作为**单个**参数交给 dsh，而不是被
/// `split_whitespace` 拆成 `"a` / `b"` 两段并把引号原样带进去。
///
/// 与真正的 shell 不同：不做变量 / 命令 / 通配展开，只是「引号保护的空白切分」。
/// 未闭合的引号按已输入内容宽容处理（不报错），以免误伤手打漏一个引号的用户。
pub fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    // 是否已经开始一个 token（用引号括出的空串 `""` 也算一个空参数）
    let mut started = false;
    // None = 引号外；Some('\'') / Some('"') = 处于对应引号内
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    cur.push(c); // 单引号内一切字面量，反斜杠也不例外
                }
            }
            Some('"') => {
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    match chars.peek() {
                        // 双引号内只把 \" 与 \\ 当转义，其余反斜杠原样保留
                        Some(n) if *n == '"' || *n == '\\' => {
                            cur.push(*n);
                            chars.next();
                        }
                        _ => cur.push('\\'),
                    }
                } else {
                    cur.push(c);
                }
            }
            // 只可能是 None（quote 只会被置为 '\'' / '"' 或清空）；用 `_` 覆盖，
            // 编译器无法据 char 取值证明穷尽
            _ => {
                if c.is_whitespace() {
                    if started {
                        out.push(std::mem::take(&mut cur));
                        started = false;
                    }
                } else if c == '\'' || c == '"' {
                    quote = Some(c);
                    started = true;
                } else if c == '\\' {
                    // 引号外的反斜杠转义下一个字符
                    match chars.next() {
                        Some(n) => {
                            cur.push(n);
                            started = true;
                        }
                        None => {
                            cur.push('\\');
                            started = true;
                        }
                    }
                } else {
                    cur.push(c);
                    started = true;
                }
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{is_safe_version, shell_quote, split_args};

    /// 反例防护：除白名单外，生产代码里不得直接 `Command::new`。
    ///
    /// 启动器是 GUI 进程，Windows 上起控制台子进程时只要没设 `CREATE_NO_WINDOW`，就会
    /// 在屏幕上弹一个黑框 —— 进插件页 / 探渠道 / 测加速时一次十几个 git，用户看到的就是
    /// 「一进页面一堆黑窗口」（GitHub issue #1 报告里的第二段）。新增后台命令请走
    /// `util::hidden_command` / `util::spawn_command`。
    #[test]
    fn production_spawns_go_through_hidden_command() {
        // 白名单只有两个：util 是统一出口本身；launcher 那个是用户**明确要的**可见终端窗口
        fn violation(file: &str, line: &str) -> bool {
            if file == "util.rs" || file == "launcher.rs" {
                return false;
            }
            line.contains("Command::new(")
        }
        // 匹配器自检：抓不到反例的话这个测试就是假绿
        assert!(violation("plugin.rs", r#"let c = Command::new("git");"#));
        assert!(!violation("launcher.rs", r#"Command::new("cmd")"#));

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("读不到 src 目录").flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let file = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            // 只扫生产代码：各文件的测试模块都写在 `#[cfg(test)]` 之后
            let prod = text.split("#[cfg(test)]").next().unwrap_or("");
            for (i, line) in prod.lines().enumerate() {
                if violation(&file, line) {
                    offenders.push(format!("{file}:{} {}", i + 1, line.trim()));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "下面这些地方在 Windows 上会弹控制台黑框，请改用 util::hidden_command / spawn_command：\n{}",
            offenders.join("\n")
        );
    }

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

    #[test]
    fn split_args_respects_quotes_and_escapes() {
        // 双引号内的空格并入同一个参数
        assert_eq!(
            split_args(r#"--profile web --title "a b""#),
            vec!["--profile", "web", "--title", "a b"]
        );
        // 单引号同理，且内部字面量
        assert_eq!(split_args("--msg 'hello world'"), vec!["--msg", "hello world"]);
        // 多空格折叠、首尾空白忽略
        assert_eq!(split_args("   a    b   "), vec!["a", "b"]);
        // 引号外反斜杠转义空格
        assert_eq!(split_args(r#"x\ y"#), vec!["x y"]);
        // 相邻无空白的片段拼成一个 token
        assert_eq!(split_args(r#"a"b c"d"#), vec!["ab cd"]);
        // 空引号 → 一个空参数（与 shell 一致）
        assert_eq!(split_args(r#"--flag """#), vec!["--flag", ""]);
        // 未闭合引号宽容吞掉剩余内容，不 panic
        assert_eq!(split_args(r#"--x "y z"#), vec!["--x", "y z"]);
        // 空输入 → 无参数
        assert!(split_args("   ").is_empty());
    }
}
