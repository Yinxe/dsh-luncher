//! pnpm 兼容层：把 `dsh plugin` 背后 pnpm 的失败翻译成「一个可操作的原因 + 一次性重试」。
//!
//! 处理方案对齐同生态的 dshmarket（`src/pnpm-compat.ts` + `src/install.ts` 的
//! `withHoistRecovery`），那几个常量与重试策略是在大量真实 issue 上磨出来的：
//!
//! - **一次性重试，而不是改用户配置**：pnpm 11 在做任何 add/remove 之前都会校验
//!   整个锁定文件，一个「刚发布不久」的版本会挡住**所有**操作（连卸载别的插件也失败，
//!   见 dshmarket #39）。恢复方式是对**这一条命令**放行一次，而不是永久改策略。
//! - `--config.minimum-release-age=0` 必须写成**短横线**：pnpm ≥12.3 会**静默忽略**
//!   camelCase 写法（#600），重试就会和白跑一次没区别。
//! - 每个 `--config.<key>` 还要**同时**以 `PNPM_CONFIG_<KEY>` 环境变量再给一遍：
//!   pnpm 12 会忽略部分 CLI 覆盖（fetchTimeout / auto-install-peers，#615），而
//!   `PNPM_CONFIG_*` 在 11 与 12 上都被读取。
//! - `-w` 只在 profile 确实是 workspace 根（存在 pnpm-workspace.yaml）时注入：
//!   pnpm 9 在 workspace 根 add 不加 `-w` 会报 ERR_PNPM_ADDING_TO_ROOT，
//!   而所有大版本在**非** workspace 目录加 `-w` 都会失败。

use std::path::Path;

/// 一次性的「新版本安全等待期」放行（短横线拼写，见模块注释）
pub const RELEASE_AGE_OVERRIDE: &str = "--config.minimum-release-age=0";
/// 一次性的更长单请求超时（github 源会下载整仓，默认 60s 常常不够）
pub const FETCH_TIMEOUT_OVERRIDE: &str = "--config.fetchTimeout=600000";
/// 一次性的关闭 peer 自动安装（宿主注入的 `@deepseek-ai/*` 在 npm 上并不存在）
pub const AUTO_INSTALL_PEERS_OFF: &str = "--config.auto-install-peers=false";
/// dsh 运行时注入、npm 上不发布的宿主命名空间
pub const HOST_NAMESPACE: &str = "@deepseek-ai/";

/// 归好类的 pnpm 失败
#[derive(Debug, Clone, PartialEq)]
pub enum PnpmFailure {
    /// 锁定文件里有「发布未满 minimumReleaseAge」的版本，挡住一切改动
    ReleaseAge { entries: Vec<String> },
    /// 解析不到某个宿主 peer（`@deepseek-ai/*`）：运行时自带，npm 上没有
    HostPeer { pkg: Option<String> },
    /// 网络临时抖动（安装会重放整棵依赖树，任何既有依赖抖动都会中断）
    TransientNetwork,
    /// pnpm 单请求超时（大 tarball / 慢网络）
    FetchTimeout,
    /// 依赖需要执行构建脚本，被 pnpm 默认拦截
    IgnoredBuilds,
    /// git 插件需要在安装时构建，被 pnpm 默认拦截
    GitPrepareNotAllowed,
    /// git 插件自己的构建脚本失败（最常见是仓库自带 lockfile 与当前 registry 不匹配）
    GitPrepareFailed { pkg: Option<String> },
    /// lockfile 里的 tarball 地址与当前 registry 元数据不一致（常见于镜像 registry）
    TarballUrlMismatch { pkgs: Vec<String> },
    /// lockfile 条目缺 integrity：pnpm 因此拒绝该 profile 的所有操作
    MissingIntegrity { pkgs: Vec<String> },
    /// node_modules 链接到的 store 与当前 pnpm 默认 store 不同
    UnexpectedStore { linked: Option<String>, wanted: Option<String> },
    /// node_modules 由另一个 pnpm 大版本创建，布局不兼容
    HoistDiff,
    /// 依赖在 registry 上不存在（幽灵依赖 / 私有包）
    Fetch404 { pkg: Option<String> },
    /// `file:` 依赖指向的本地路径已不存在
    MissingLocalDep { path: Option<String> },
    /// Windows：node_modules 里的文件被占用，重命名/删除失败（EPERM / EBUSY）
    FileLocked,
    /// PATH 上有 pnpm 但起不来（9009 / EACCES / ENOENT）
    PnpmUnusable,
    /// PATH 上没有 pnpm
    PnpmMissing,
}

impl PnpmFailure {
    pub fn code(&self) -> &'static str {
        match self {
            PnpmFailure::ReleaseAge { .. } => "release-age-violation",
            PnpmFailure::HostPeer { .. } => "host-peer",
            PnpmFailure::TransientNetwork => "transient-network",
            PnpmFailure::FetchTimeout => "fetch-timeout",
            PnpmFailure::IgnoredBuilds => "ignored-builds",
            PnpmFailure::GitPrepareNotAllowed => "git-prepare-not-allowed",
            PnpmFailure::GitPrepareFailed { .. } => "git-prepare-failed",
            PnpmFailure::TarballUrlMismatch { .. } => "tarball-url-mismatch",
            PnpmFailure::MissingIntegrity { .. } => "missing-tarball-integrity",
            PnpmFailure::UnexpectedStore { .. } => "unexpected-store",
            PnpmFailure::HoistDiff => "hoist-pattern-diff",
            PnpmFailure::Fetch404 { .. } => "fetch-404",
            PnpmFailure::MissingLocalDep { .. } => "missing-local-dependency",
            PnpmFailure::FileLocked => "file-locked",
            PnpmFailure::PnpmUnusable => "pnpm-unusable",
            PnpmFailure::PnpmMissing => "pnpm-missing",
        }
    }

    /// 一次性重试要追加的覆盖参数；None = 这类失败不做自动重试
    pub fn retry_override(&self) -> Option<&'static str> {
        match self {
            PnpmFailure::ReleaseAge { .. } => Some(RELEASE_AGE_OVERRIDE),
            PnpmFailure::HostPeer { .. } => Some(AUTO_INSTALL_PEERS_OFF),
            PnpmFailure::FetchTimeout => Some(FETCH_TIMEOUT_OVERRIDE),
            // 网络抖动重跑一次同样的命令即可，不需要覆盖参数
            PnpmFailure::TransientNetwork => Some(""),
            _ => None,
        }
    }

    /// 是否需要先 `pnpm install --no-frozen-lockfile` 重建 node_modules 再重试
    pub fn needs_relink(&self) -> bool {
        matches!(self, PnpmFailure::HoistDiff | PnpmFailure::UnexpectedStore { .. })
    }

    /// 给用户看的原因与下一步（中文，可操作）
    pub fn message(&self, profile_dir: &Path, profile: &str) -> String {
        let dir = profile_dir.display();
        // 提示语一律给 dsh plugin 形式：启动器不鼓励绕过官方命令直接敲 pnpm
        let dsh = |args: &str| format!("dsh plugin --profile {profile} {args}");
        match self {
            PnpmFailure::ReleaseAge { entries } => {
                let who = if entries.is_empty() {
                    String::new()
                } else {
                    format!("\n被拦下的条目：{}", entries.join("、"))
                };
                format!(
                    "这个 profile 的锁定文件里有一个「发布未满安全等待期」的版本，\
                     pnpm 在做任何改动前都会先校验整个锁定文件，所以连卸载别的插件也会失败（与本次操作的插件无关）。{who}\n\
                     已自动用 `{RELEASE_AGE_OVERRIDE}` 放行重试一次；若仍失败，\
                     可执行 `{}` 重建锁定文件，\
                     或在 {dir}/pnpm-workspace.yaml 里按上面条目补 minimumReleaseAgeExclude。"
                    ,
                    dsh("clean --lockfile")
                )
            }
            PnpmFailure::HostPeer { pkg } => {
                let who = pkg.clone().unwrap_or_else(|| "宿主包".into());
                format!(
                    "插件声明的依赖 {who} 是 dsh 运行时注入的宿主包，npm 上并不存在，\
                     pnpm 却去下载它。已自动用 `{AUTO_INSTALL_PEERS_OFF}` 关闭 peer 自动安装重试一次。"
                )
            }
            PnpmFailure::TransientNetwork => format!(
                "拉取依赖时网络临时失败（不一定是你正在装的插件——安装会重放整棵依赖树）。\
                 已自动重试一次；若仍失败请稍后再试。"
            ),
            PnpmFailure::FetchTimeout => format!(
                "下载超时（日志里的 `error (23)` 就是它）：github: 规格会让 pnpm 去 codeload.github.com\
                 下载**整仓 tar.gz**（带 #path: 子目录也一样，整仓下完再取子目录），pnpm 默认单请求 60 秒常常不够。\n\
                 已自动用 `{FETCH_TIMEOUT_OVERRIDE}`（并以 PNPM_CONFIG_FETCH_TIMEOUT 再给一遍）重试一次。\n\
                 如果反复超时：改用「Clone 仓库」安装——一次 git clone 到本地后按 link: 安装，\
                 整仓只下载一次、后续更新走 git pull，完全不经过 codeload。"
            ),
            PnpmFailure::IgnoredBuilds | PnpmFailure::GitPrepareNotAllowed => format!(
                "有依赖需要在安装时执行构建脚本，被 pnpm 默认拦截（pnpm 已打印被拦的包名）。\n\
                 点下方「允许构建脚本并重试」即可：启动器会把这些包写进 {dir}/pnpm-workspace.yaml 的 allowBuilds\
                 （合并已有条目、保留注释与行尾、写前备份），然后用同样的参数重跑一次。\n\
                 构建脚本会执行第三方代码——只有你确认这些包可信时才放行。"
            ),
            PnpmFailure::GitPrepareFailed { pkg } => {
                let who = pkg.clone().map(|p| format!("（{p}）")).unwrap_or_default();
                format!(
                    "git 插件{who}在安装时要跑它自己仓库里的构建（pnpm install），但构建失败了。\
                     最常见的原因是仓库自带的 pnpm-lock.yaml 与你的 registry 不兼容\
                     （lockfile 里的 tarball 指向 registry.npmjs.org，而你在用镜像站），pnpm 11+ 的供应链检查会拒绝整个 lockfile；\
                     也可能是仓库构建脚本本身出错或网络问题。\
                     可以把该依赖固定到已知可用的提交（github:owner/repo#<commit>）后再动其他插件。"
                )
            }
            PnpmFailure::TarballUrlMismatch { pkgs } => {
                let who = list(pkgs);
                format!(
                    "pnpm-lock.yaml 里有些条目的 tarball 地址与当前 registry 发布的元数据不一致{who}，\
                     pnpm 的供应链检查因此拒绝这个 profile 里的**所有**安装与卸载。\
                     常见于镜像 registry（如 npmmirror）遇上用 npmjs.org 生成的锁定文件。\
                     按 pnpm 的提示执行 `{}` 重建锁定后重装，或把 registry 切回生成该锁定文件的源。\
                     （启动器不会替你改锁定文件：那等于替你做供应链决定。）",
                    dsh("clean --lockfile")
                )
            }
            PnpmFailure::MissingIntegrity { pkgs } => {
                let who = list(pkgs);
                format!(
                    "profile 的 pnpm-lock.yaml 里有条目{who}缺少 integrity，pnpm 因此拒绝这个 profile 里的所有安装和卸载——\
                     包括卸载它自己，所以装不回来也删不掉。\
                     请在 pnpm-lock.yaml 里删掉点名的那条依赖记录后重试（**不要**删整个锁定文件，那会让其余插件全部重新解析版本）。\
                     启动器不会自动为未经验证的字节生成校验值。"
                )
            }
            PnpmFailure::UnexpectedStore { linked, wanted } => {
                let detail = match (linked, wanted) {
                    (Some(l), Some(w)) => format!("\n  node_modules → {l}\n  pnpm 现在想用 → {w}"),
                    _ => String::new(),
                };
                format!(
                    "这个 profile 的 node_modules 链接到的 pnpm store 与当前 pnpm 默认使用的不是同一个，pnpm 因此拒绝所有安装与卸载。{detail}\n\
                     执行一次 `{}` 重新链接即可（必要时先退出 dsh）。",
                    dsh("install --store-dir <上面第一个路径>")
                )
            }
            PnpmFailure::HoistDiff => format!(
                "这个 profile 的 node_modules 是旧版 pnpm 建的，与当前 pnpm 的默认布局不兼容，需要重建后重试。\
                 启动器已自动执行 `{}` 尝试重建。",
                dsh("install --no-frozen-lockfile")
            ),
            PnpmFailure::Fetch404 { pkg } => {
                let who = pkg.clone().map(|p| format!("（{p}）")).unwrap_or_default();
                format!(
                    "有一个依赖在 registry 上不存在{who}，pnpm 因此拒绝任何安装操作。\
                     它可能是之前失败的操作残留在 profile package.json 里的幽灵依赖（手动删掉那一行即可），\
                     也可能是需要登录的私有包。"
                )
            }
            PnpmFailure::MissingLocalDep { path } => {
                let who = path.clone().map(|p| format!("（{p}）")).unwrap_or_default();
                format!(
                    "profile 里有一个从本地路径安装的插件，而那个路径已经不在了{who}。\
                     pnpm 在做任何改动前都会重新解析全部直接依赖，所以这一条会挡住所有安装和卸载。\
                     请在 profile 的 package.json 里删掉值为该路径的那一行依赖，然后重试。"
                )
            }
            PnpmFailure::FileLocked => format!(
                "文件被别的进程占着，pnpm 没法替换/删除它（Windows 上常是 EPERM/EBUSY）。\
                 多半是这个 profile 的 dsh 还在运行、编辑器/终端正开着这个目录，或杀毒软件在扫描。\n\
                 请先退出这个实例（托盘里停掉对应 dsh），关掉可能占用目录的程序，再重试安装；\n\
                 仍然失败就重启系统后再装。目录：{dir}"
            ),
            PnpmFailure::PnpmUnusable => "系统里有一个 pnpm 但起不来，插件没有任何改动。\n\
                 在系统终端里执行一次 `pnpm --version`：那里也失败说明要修的是这台机器上的 pnpm（权限/安装）；\
                 那里正常说明是启动方式带来的环境差异，试试直接用终端启动 dsh。"
                .to_string(),
            PnpmFailure::PnpmMissing => {
                "找不到 pnpm，请先安装 pnpm（dsh 的插件管理是 pnpm 的转发器）。".to_string()
            }
        }
    }
}

fn list(pkgs: &[String]) -> String {
    if pkgs.is_empty() {
        String::new()
    } else {
        format!("（{}）", pkgs.join("、"))
    }
}

/// 从 `[ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION]` 的条目行里取出 `name@version`
pub fn release_age_entries(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        if !line.contains("was published at") && !line.contains("does not meet the minimumReleaseAge") {
            continue;
        }
        let first = line.trim().split_whitespace().next().unwrap_or("");
        if first.contains('@') && !first.starts_with('[') {
            let t = first.trim().to_string();
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// 从 `GET https://…/<name>:` 这类行里取包名（%2F 还原为 /）
fn pkg_from_get(output: &str) -> Option<String> {
    for line in output.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("GET ") else { continue };
        let Some(idx) = rest.rfind('/') else { continue };
        let tail = rest[idx + 1..].trim_end_matches(':');
        if tail.is_empty() || tail.contains(' ') {
            continue;
        }
        return Some(tail.replace("%2F", "/").replace("%2f", "/"));
    }
    None
}

/// 从 `No matching version found for <name>@<range>` 里取包名
fn pkg_from_no_matching(output: &str) -> Option<String> {
    let idx = output.find("No matching version found for ")?;
    let rest = &output[idx + "No matching version found for ".len()..];
    let token = rest.split_whitespace().next()?;
    let at = token.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some(token[..at].to_string())
}

fn quoted_after<'a>(hay: &'a str, needle: &str) -> Option<String> {
    let i = hay.find(needle)?;
    let rest = &hay[i + needle.len()..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].to_string())
}

/// 分类一次失败的 pnpm 输出
pub fn classify(output: &str, exit_code: Option<i32>) -> Option<PnpmFailure> {
    let low = output.to_lowercase();

    if output.contains("ERR_PNPM_PUBLIC_HOIST_PATTERN_DIFF")
        || output.contains("ERR_PNPM_VIRTUAL_STORE_DIR_MAX_LENGTH_DIFF")
    {
        return Some(PnpmFailure::HoistDiff);
    }
    if output.contains("ERR_PNPM_UNEXPECTED_STORE") {
        return Some(PnpmFailure::UnexpectedStore {
            linked: quoted_after(output, "currently linked from the store at"),
            wanted: quoted_after(output, "wants to use the store at"),
        });
    }
    if output.contains("ERR_PNPM_MISSING_TARBALL_INTEGRITY") {
        let mut pkgs: Vec<String> = Vec::new();
        for line in output.lines() {
            if !line.contains("has no \"integrity\" field") {
                continue;
            }
            let first = line.trim().split_whitespace().next().unwrap_or("");
            if let Some(name) = first.split('@').next() {
                if !name.is_empty() && !pkgs.contains(&name.to_string()) {
                    pkgs.push(name.to_string());
                }
            }
        }
        return Some(PnpmFailure::MissingIntegrity { pkgs });
    }
    if output.contains("ERR_PNPM_TARBALL_URL_MISMATCH") {
        let mut pkgs: Vec<String> = Vec::new();
        for line in output.lines() {
            if !line.contains("has a tarball URL") {
                continue;
            }
            let first = line.trim().split_whitespace().next().unwrap_or("");
            let name = first.split('@').next().unwrap_or("");
            if !name.is_empty() && !pkgs.contains(&name.to_string()) {
                pkgs.push(name.to_string());
            }
        }
        return Some(PnpmFailure::TarballUrlMismatch { pkgs });
    }
    // 新版本安全等待期：校验整个锁定文件，挡住一切改动（dshmarket #39）
    if output.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION")
        || output.contains("ERR_PNPM_NO_MATURE_MATCHING_VERSION")
    {
        return Some(PnpmFailure::ReleaseAge {
            entries: release_age_entries(output),
        });
    }
    if output.contains("ERR_PNPM_IGNORED_BUILDS") {
        return Some(PnpmFailure::IgnoredBuilds);
    }
    if output.contains("ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED") {
        return Some(PnpmFailure::GitPrepareNotAllowed);
    }
    if output.contains("ERR_PNPM_PREPARE_PACKAGE") {
        let pkg = {
            let needle = "Failed to prepare git-hosted package fetched from";
            quoted_after(output, needle).and_then(|url| {
                url.rsplit('/').next().map(|s| s.to_string())
            })
        };
        return Some(PnpmFailure::GitPrepareFailed { pkg });
    }
    if output.contains("ERR_PNPM_FETCH_404") {
        return Some(PnpmFailure::Fetch404 {
            pkg: pkg_from_get(output),
        });
    }
    if output.contains("ERR_PNPM_NO_MATCHING_VERSION") {
        let pkg = pkg_from_no_matching(output).or_else(|| pkg_from_get(output));
        // 宿主 peer 与普通「版本不存在」要分开：前者重试一次即可绕过
        let host = pkg.as_deref().map(|p| p.starts_with(HOST_NAMESPACE)).unwrap_or(false);
        if host {
            return Some(PnpmFailure::HostPeer { pkg });
        }
        return Some(PnpmFailure::Fetch404 { pkg });
    }
    if output.contains("ERR_PNPM_ADDING_TO_ROOT")
        || output.contains("--workspace-root may only be used inside a workspace")
    {
        // 我们自己控制 -w 的注入，出现即说明注入判断错了
        return Some(PnpmFailure::HoistDiff);
    }
    // 本地 file:/link: 依赖已消失
    if output.contains("while installing a direct dependency") && low.contains("enoent") {
        let path = quoted_after(output, "ENOENT: no such file or directory, open");
        return Some(PnpmFailure::MissingLocalDep { path });
    }
    // Windows 上替换/删除被占用的文件失败（杀毒实时防护、编辑器、残留的 dsh 进程
    // 攥着 node_modules）。过去并进 FetchTimeout：自动重试白给一个 600 秒超时参数，
    // 消息还在讲「整仓 tar.gz 下载」，用户照着换 clone 装了照样失败 —— 单独归类。
    if low.contains("err_pnpm_eperm")
        || low.contains("err_pnpm_ebusy")
        || low.contains("operation not permitted, rename")
        || low.contains("resource busy or locked")
        || low.contains("failed to remove existing directory")
    {
        return Some(PnpmFailure::FileLocked);
    }
    if is_transient(output) {
        return Some(PnpmFailure::TransientNetwork);
    }
    if is_fetch_timeout(output) {
        return Some(PnpmFailure::FetchTimeout);
    }
    if output.contains("pnpm not found on PATH") {
        return Some(PnpmFailure::PnpmMissing);
    }
    let cmd_not_found = exit_code == Some(9009)
        || output.contains("is not recognized as an internal or external command")
        || output.contains("不是内部或外部命令");
    if cmd_not_found || output.contains("spawnSync pnpm") {
        return Some(PnpmFailure::PnpmUnusable);
    }
    None
}

/// 瞬态网络失败：值得自动重试一次
pub fn is_transient(output: &str) -> bool {
    let re = regex::Regex::new(
        r"ERR_PNPM_FETCH_5\d\d|ERR_PNPM_META_FETCH_FAIL|FetchError|ECONNRESET|ETIMEDOUT|EAI_AGAIN|ENETUNREACH|socket hang up|network timeout",
    )
    .expect("静态正则");
    re.is_match(output)
}

/// pnpm 单请求超时（undici 的 abort 在日志里长这样）
pub fn is_fetch_timeout(output: &str) -> bool {
    let re = regex::Regex::new(r"operation was aborted due to timeout|TimeoutError|error \(23\)")
        .expect("静态正则");
    re.is_match(output)
}

/// `dsh plugin add|remove` 的参数：profile 是 workspace 根时注入 `-w`
pub fn plugin_args_for(profile_dir: &Path, args: &[String]) -> Vec<String> {
    let first = args.first().map(String::as_str).unwrap_or("");
    if first != "add" && first != "remove" {
        return args.to_vec();
    }
    if !profile_dir.join("pnpm-workspace.yaml").is_file() {
        return args.to_vec();
    }
    let mut out = vec![first.to_string(), "-w".to_string()];
    out.extend(args.iter().skip(1).cloned());
    out
}

/// 把 argv 里的 `--config.<key>=<value>` 再以 `PNPM_CONFIG_<KEY>` 环境变量给一遍。
///
/// pnpm 12 会静默忽略部分 CLI 覆盖（fetchTimeout / auto-install-peers），
/// 而 `PNPM_CONFIG_*` 在 11 与 12 上都被读取；只对携带覆盖参数的那一次运行生效。
pub fn config_env_for(args: &[String]) -> Vec<(String, String)> {
    let re = regex::Regex::new(r"^--config\.([A-Za-z][A-Za-z0-9-]*)=(\S+)$").expect("静态正则");
    let mut out = Vec::new();
    for a in args {
        let Some(c) = re.captures(a) else { continue };
        let key = c[1]
            .chars()
            .enumerate()
            .fold(String::new(), |mut acc, (i, ch)| {
                if i > 0 && ch.is_ascii_uppercase() {
                    acc.push('_');
                }
                acc.push(ch.to_ascii_uppercase());
                acc
            })
            .replace('-', "_");
        out.push((format!("PNPM_CONFIG_{key}"), c[2].to_string()));
    }
    out
}

/// pnpm 报出的「构建脚本被拦」的包名（裸名，已去掉 @version 与句末句号）。
///
/// pnpm 原句：`Ignored build scripts: esbuild, koffi.` —— 条目可能带版本后缀。
pub fn parse_ignored_builds(output: &str) -> Vec<String> {
    let re = regex::Regex::new(r"(?i)Ignored build scripts:?\s*([^\n]+)").expect("静态正则");
    let Some(c) = re.captures(output) else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for chunk in c[1].split(',') {
        let trimmed = chunk.trim().trim_end_matches('.');
        if trimmed.is_empty() {
            continue;
        }
        let name = match trimmed.rfind('@') {
            Some(at) if at > 0 => &trimmed[..at],
            _ => trimmed,
        };
        if !name.is_empty() && !found.iter().any(|x| x == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// git 插件被 pnpm 的 fetcher 拦下时点名的包
/// （`The git-hosted package "name@2.8.0" needs to execute build scripts but is not in the "allowBuilds" allowlist.`）
///
/// 注意：市场类调用常用 `--reporter=ndjson`，这句话会以**转义引号**（`\"`）到达，
/// 因此先还原再匹配，否则生产路径上永远匹配不到。
pub fn parse_prepare_not_allowed(output: &str) -> Option<String> {
    let text = output.replace("\\\"", "\"");
    let re = regex::Regex::new(r#"git-hosted package "([^"]+)" needs to execute build scripts"#)
        .expect("静态正则");
    let raw = re.captures(&text)?.get(1)?.as_str().trim().to_string();
    // 去掉尾部的 @version：名字本身可能带 scope（@scope/pkg）
    let at = raw.rfind('@')?;
    Some(if at > 0 { raw[..at].to_string() } else { raw })
}

/// 该包是否是「宿主注入、npm 上不存在」的 peer（且不是 profile 的直接依赖）
pub fn is_unpublished_host_peer(pkg: &str, profile_dir: &Path) -> bool {
    if !pkg.starts_with(HOST_NAMESPACE) {
        return false;
    }
    let Ok(raw) = std::fs::read_to_string(profile_dir.join("package.json")) else {
        return true;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return true;
    };
    !v.get("dependencies")
        .and_then(|d| d.as_object())
        .map(|d| d.contains_key(pkg))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用真实措辞做固定用例（与参考项目 tests/pnpm-compat.spec.ts 对齐）
    #[test]
    fn classifies_release_age_violation() {
        let out = "? Verifying lockfile against supply-chain policies (100 entries)...\n\
                   ✗ Lockfile failed supply-chain policy check (100 entries in 1.9s)\n\
                   [ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION] 1 lockfile entries failed verification:\n\
                     dshmarket@1.50.0 was published at 2026-09-20T02:58:05.000Z, within the minimumReleaseAge cutoff\n\
                   dsh: pnpm failed in profile directory /home/u/.dsh/profiles/web-test";
        let f = classify(out, Some(1)).expect("应分类");
        assert_eq!(f.code(), "release-age-violation");
        assert_eq!(f.retry_override(), Some(RELEASE_AGE_OVERRIDE));
        match &f {
            PnpmFailure::ReleaseAge { entries } => assert_eq!(entries, &vec!["dshmarket@1.50.0".to_string()]),
            other => panic!("{other:?}"),
        }
        // 短横线拼写：pnpm ≥12.3 会静默忽略 camelCase
        assert!(RELEASE_AGE_OVERRIDE.contains("minimum-release-age"));
        assert!(!RELEASE_AGE_OVERRIDE.contains("minimumReleaseAge"));
    }

    #[test]
    fn classifies_host_peer_and_network_and_timeout() {
        let host = " ERR_PNPM_NO_MATCHING_VERSION  No matching version found for @deepseek-ai/dsh-tools@^0.1.0-rc.1";
        let f = classify(host, Some(1)).unwrap();
        assert_eq!(f.code(), "host-peer");
        assert_eq!(f.retry_override(), Some(AUTO_INSTALL_PEERS_OFF));

        let net = "ERR_PNPM_FETCH_500  GET https://registry.npmjs.org/x: Internal Server Error";
        assert_eq!(classify(net, Some(1)).unwrap().code(), "transient-network");

        let t = "GET https://codeload.github.com/o/r/tar.gz/abc error (23)";
        assert_eq!(classify(t, Some(1)).unwrap().code(), "fetch-timeout");

        // 文件被占用的 EPERM/EBUSY 不是下载超时：给的是「关掉占用程序」的话术，
        // 也不会白挂一个 600 秒超时参数当重试
        let eperm = "ERR_PNPM_EPERM  operation not permitted, rename '.../node_modules/.pnpm/x' -> '...'";
        let f = classify(eperm, Some(1)).unwrap();
        assert_eq!(f.code(), "file-locked");
        assert_eq!(f.retry_override(), None);
        assert!(f.message(Path::new("/p"), "web").contains("占用"));
        let busy = "ERR_PNPM_EBUSY  resource busy or locked, rmdir '.../node_modules/x'";
        assert_eq!(classify(busy, Some(1)).unwrap().code(), "file-locked");
    }

    #[test]
    fn classifies_blocking_lockfile_problems() {
        let integrity = "ERR_PNPM_MISSING_TARBALL_INTEGRITY  Cannot install package \"is-odd@https://x/y.tgz\": its lockfile entry has no \"integrity\" field";
        let f = classify(integrity, Some(1)).unwrap();
        assert_eq!(f.code(), "missing-tarball-integrity");
        assert!(f.message(Path::new("/p"), "web").contains("不要"));

        let mismatch = "ERR_PNPM_TARBALL_URL_MISMATCH  is-odd@3.0.1 has a tarball URL that does not match the registry";
        assert_eq!(classify(mismatch, Some(1)).unwrap().code(), "tarball-url-mismatch");

        let store = "ERR_PNPM_UNEXPECTED_STORE  currently linked from the store at \"/a/store\" but pnpm wants to use the store at \"/b/store\"";
        assert_eq!(
            classify(store, Some(1)).unwrap(),
            PnpmFailure::UnexpectedStore {
                linked: Some("/a/store".into()),
                wanted: Some("/b/store".into())
            }
        );
    }

    /// 用本次实测输出固定「被拦构建脚本」的解析
    #[test]
    fn parses_ignored_build_scripts_from_real_output() {
        let out = "Progress: resolved 127, reused 127, downloaded 0, added 137, done\n\
                   [ERR_PNPM_IGNORED_BUILDS] Ignored build scripts: supreium-headless-gl@8.3.0\n\
                   Run \"pnpm approve-builds\" to pick which dependencies should be allowed to run scripts.\n";
        assert_eq!(parse_ignored_builds(out), vec!["supreium-headless-gl".to_string()]);
        // 多个条目 + 句末句号 + scope 包名
        let multi = "Ignored build scripts: esbuild, @scope/native-thing@1.2.3, koffi.";
        assert_eq!(
            parse_ignored_builds(multi),
            vec![
                "esbuild".to_string(),
                "@scope/native-thing".to_string(),
                "koffi".to_string()
            ]
        );
        assert!(parse_ignored_builds("no such line").is_empty());
        assert_eq!(
            parse_prepare_not_allowed(
                "The git-hosted package \"@o/p@2.8.0\" needs to execute build scripts but is not in the \"allowBuilds\" allowlist."
            )
            .as_deref(),
            Some("@o/p")
        );
        // ndjson 里引号是转义的
        assert_eq!(
            parse_prepare_not_allowed(
                r#"{"message":"git-hosted package \"pkg@1.0.0\" needs to execute build scripts"}"#
            )
            .as_deref(),
            Some("pkg")
        );
    }

    #[test]
    fn classifies_build_script_blocks() {
        assert_eq!(classify("ERR_PNPM_IGNORED_BUILDS", Some(1)).unwrap().code(), "ignored-builds");
        assert_eq!(
            classify("ERR_PNPM_GIT_DEP_PREPARE_NOT_ALLOWED", Some(1)).unwrap().code(),
            "git-prepare-not-allowed"
        );
        let msg = classify("ERR_PNPM_IGNORED_BUILDS", Some(1))
            .unwrap()
            .message(Path::new("/p"), "web");
        assert!(msg.contains("allowBuilds"), "{msg}");
    }

    #[test]
    fn classifies_pnpm_unusable_and_unknown() {
        assert_eq!(classify("whatever", Some(9009)).unwrap().code(), "pnpm-unusable");
        assert_eq!(classify("pnpm not found on PATH — install pnpm", Some(127)).unwrap().code(), "pnpm-missing");
        assert!(classify("fatal: destination path 'x' already exists", Some(1)).is_none());
    }

    #[test]
    fn injects_workspace_flag_only_for_workspace_roots() {
        let tmp = std::env::temp_dir().join(format!("dsh-pnpm-args-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let args = vec!["add".to_string(), "github:o/r#main".to_string()];
        // 没有 pnpm-workspace.yaml → 不注入（所有大版本加 -w 都会失败）
        assert_eq!(plugin_args_for(&tmp, &args), args);
        std::fs::write(tmp.join("pnpm-workspace.yaml"), "packages:\n  - .\n").unwrap();
        assert_eq!(
            plugin_args_for(&tmp, &args),
            vec!["add".to_string(), "-w".to_string(), "github:o/r#main".to_string()]
        );
        // 非 add/remove 不注入
        let other = vec!["update".to_string()];
        assert_eq!(plugin_args_for(&tmp, &other), other);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn repeats_config_overrides_as_env() {
        let args = vec![
            "add".to_string(),
            RELEASE_AGE_OVERRIDE.to_string(),
            FETCH_TIMEOUT_OVERRIDE.to_string(),
            AUTO_INSTALL_PEERS_OFF.to_string(),
        ];
        let env = config_env_for(&args);
        let get = |k: &str| env.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
        assert_eq!(get("PNPM_CONFIG_MINIMUM_RELEASE_AGE").as_deref(), Some("0"));
        assert_eq!(get("PNPM_CONFIG_FETCH_TIMEOUT").as_deref(), Some("600000"));
        assert_eq!(get("PNPM_CONFIG_AUTO_INSTALL_PEERS").as_deref(), Some("false"));
        assert!(config_env_for(&["add".into(), "pkg".into()]).is_empty());
    }
}
