use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 持久化在 ~/.dsh-starter/settings.json 的启动器设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// npm registry 地址，可换成镜像（如 https://registry.npmmirror.com）
    pub registry: String,
    /// 启动器自身更新清单地址（返回 {version, notes, url} 的 JSON，只能跳转下载）
    pub update_manifest_url: String,
    /// 更新下载源，二选一：
    /// - `r2`（默认）：自建 Cloudflare R2 源，地址写死在代码里（见 update_check::R2_BASE），
    ///   国内下载快；不可用时自动回落 GitHub；
    /// - `github`：官方 GitHub 源。
    /// 刻意不做「自由填写地址」——填错的后果是更新不了，而用户没有任何排查手段。
    pub update_source: String,
    /// 启动 dsh 时附加的默认参数
    pub default_args: String,
    /// 默认启动的 profile（空 = 不带 --profile，走 dsh 默认）
    pub default_profile: String,
    /// 当前使用的 dsh 版本：所有 profile 都基于该版本运行
    pub active_version: String,
    /// 终端模拟器覆盖（"auto" 或可执行文件路径）
    pub terminal: String,
    /// 启动时自动检查启动器更新
    pub auto_check_update: bool,
    /// 发现启动器新版本后直接静默下载安装并重启（仅内置 updater 模式生效）
    pub auto_install_update: bool,
    /// 启动时自动刷新版本列表
    pub auto_check_versions: bool,
    /// Node 可执行文件覆盖路径（留空自动探测）
    pub node_path: String,
    /// Node 来源：auto（系统优先，缺失回退内置）| system | runtime
    pub node_source: String,
    /// 内置 Node 运行时下载镜像站
    pub node_mirror: String,
    /// 点击关闭按钮时隐藏到托盘而不是退出
    pub close_to_tray: bool,
    /// Profile 启动方式：child=子进程（随启动器退出）| detached=独立进程（后台常驻）
    pub launch_mode: String,
    /// 可选的 GitHub Token：只用于提高 api.github.com 额度（匿名 60/小时 → 5000/小时）。
    /// 探测与更新检测走免额度通道（jsDelivr / git），留空也能正常用。
    pub github_token: String,
    /// GitHub 加速：用前缀代理加快 clone / 下载（只对 github 域名生效）
    #[serde(default = "default_true")]
    pub github_accel: bool,
    /// 固定使用哪个代理前缀；留空 = 自动（测速最快的一个）
    #[serde(default)]
    pub github_proxy: String,
    /// 额外候选前缀（逗号 / 换行分隔），与内置清单一起参与测速
    #[serde(default)]
    pub github_proxy_extra: String,
}

fn default_true() -> bool {
    true
}

/// 默认走自建 R2 源（快）；R2 不可用时更新器会自动落到 GitHub
fn default_update_source() -> String {
    "r2".into()
}

/// 把设置里的更新源收敛到 r2 / github（未知值按 r2 处理）
pub fn normalize_update_source(v: &str) -> &'static str {
    if v.trim().eq_ignore_ascii_case("github") {
        "github"
    } else {
        "r2"
    }
}

impl Settings {
    /// 诊断用的设置摘要：**绝不包含凭据**。
    /// registry / 更新清单里的 userinfo 会被抹掉，GitHub Token 只报「是否设置」。
    /// 这份内容会进入诊断包（用户主动导出并发给别人），所以新增字段时要想想能不能公开。
    pub fn redacted_summary(&self) -> Vec<(String, String)> {
        let b = |v: bool| if v { "开" } else { "关" }.to_string();
        vec![
            ("registry".into(), crate::registry::scrub_url(&self.registry)),
            (
                "update_manifest_url".into(),
                crate::registry::scrub_url(&self.update_manifest_url),
            ),
            (
                "update_source".into(),
                if normalize_update_source(&self.update_source) == "github" {
                    "github（官方源）".into()
                } else {
                    "r2（自建源）".into()
                },
            ),
            ("active_version".into(), self.active_version.clone()),
            ("default_profile".into(), self.default_profile.clone()),
            ("default_args".into(), self.default_args.clone()),
            ("node_source".into(), self.node_source.clone()),
            ("node_path".into(), self.node_path.clone()),
            ("node_mirror".into(), self.node_mirror.clone()),
            ("launch_mode".into(), self.launch_mode.clone()),
            ("terminal".into(), self.terminal.clone()),
            ("close_to_tray".into(), b(self.close_to_tray)),
            ("auto_check_update".into(), b(self.auto_check_update)),
            ("auto_install_update".into(), b(self.auto_install_update)),
            ("auto_check_versions".into(), b(self.auto_check_versions)),
            ("github_accel".into(), b(self.github_accel)),
            ("github_proxy".into(), self.github_proxy.clone()),
            ("github_proxy_extra".into(), self.github_proxy_extra.clone()),
            (
                "github_token".into(),
                if self.github_token.trim().is_empty() {
                    "（未设置）".into()
                } else {
                    format!("（已设置，长度 {}）", self.github_token.trim().len())
                },
            ),
        ]
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            registry: "https://registry.npmjs.org".into(),
            update_manifest_url: String::new(),
            update_source: default_update_source(),
            default_args: String::new(),
            default_profile: String::new(),
            active_version: String::new(),
            terminal: "auto".into(),
            auto_check_update: true,
            auto_install_update: false,
            auto_check_versions: true,
            node_path: String::new(),
            node_source: "auto".into(),
            node_mirror: "https://npmmirror.com/mirrors/node".into(),
            close_to_tray: true,
            launch_mode: "child".into(),
            github_token: String::new(),
            github_accel: true,
            github_proxy: String::new(),
            github_proxy_extra: String::new(),
        }
    }
}

#[derive(Default)]
pub struct AppState {
    pub settings: Mutex<Settings>,
}

pub fn home_dir() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("DSH_STARTER_HOME") {
        let p = PathBuf::from(h);
        if p.is_dir() {
            return Some(p);
        }
    }
    if cfg!(windows) {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    } else {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// 数据根目录名（0.2.0 起）。`LEGACY_HOME_DIR` 只在首次启动搬运旧数据时用。
const HOME_DIR: &str = ".dsh-starter";
const LEGACY_HOME_DIR: &str = ".dsh-launcher";

/// 启动器数据根目录（区别于 dsh 自身的 ~/.dsh）
///
/// 0.2.0 把目录从 `~/.dsh-launcher` 改名成 `~/.dsh-starter`：首次调用时把旧目录整体
/// `rename` 过来，已装版本 / profile / 设置原样保留（同卷 rename 不复制字节，再大也是瞬时）。
/// 搬不动（跨设备挂载、Windows 上目录被占用）就**继续用旧目录** —— 名字不一致只是不好看，
/// 让用户以为数据丢了才是事故。
///
/// ⚠️ 这个函数里不能调 `diag::*`：它算日志目录时会回调本函数，直接无限递归。
pub fn starter_home() -> PathBuf {
    match home_dir() {
        Some(home) => migrate_home(&home),
        None => PathBuf::from(HOME_DIR),
    }
}

/// 首次调用把 `~/.dsh-launcher` 搬到 `~/.dsh-starter`，返回真正该用的那个目录。
fn migrate_home(home: &Path) -> PathBuf {
    let new = home.join(HOME_DIR);
    let old = home.join(LEGACY_HOME_DIR);
    if new.exists() || !old.is_dir() {
        return new;
    }
    match std::fs::rename(&old, &new) {
        Ok(()) => new,
        Err(e) => {
            eprintln!(
                "数据目录改名失败（{} → {}）：{e}；继续使用旧目录，数据未丢失",
                old.display(),
                new.display()
            );
            old
        }
    }
}

/// 启动器管理的 dsh 版本安装根目录
pub fn versions_dir() -> PathBuf {
    starter_home().join("versions")
}

const SETTINGS_FILE: &str = "settings.json";

pub fn settings_path() -> PathBuf {
    starter_home().join(SETTINGS_FILE)
}

pub fn load_settings() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    let parent = path
        .parent()
        .ok_or_else(|| "设置路径缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // 原子写：先写同目录临时文件、再 rename 覆盖。直接 fs::write 会先截断目标文件，
    // 中途崩溃 / 断电会留下半截 JSON，下次启动解析失败会静默回退到默认设置，
    // 再保存时就把 github_token、registry 等全部覆盖掉。
    let tmp = parent.join(format!("{SETTINGS_FILE}.tmp"));
    fs::write(&tmp, &text).map_err(|e| format!("写入设置失败: {e}"))?;
    #[cfg(unix)]
    {
        // settings.json 可能存有 github_token，落位前收紧到仅本人可读写
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("写入设置失败: {e}")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_round_trips_token_and_leaves_no_temp_file() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        let mut s = Settings::default();
        s.github_token = "ghp_secret_value".into();
        s.registry = "https://registry.npmmirror.com".into();
        save_settings(&s).unwrap();

        let path = settings_path();
        let stale = starter_home().join(format!("{SETTINGS_FILE}.tmp"));
        assert!(path.exists(), "settings.json 应已落位");
        assert!(!stale.exists(), "原子写不应残留临时文件");

        let back = load_settings();
        assert_eq!(back.github_token, "ghp_secret_value");
        assert_eq!(back.registry, "https://registry.npmmirror.com");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "含 token 的 settings.json 应仅本人可读写");
        }

        std::env::remove_var("DSH_STARTER_HOME");
        fs::remove_dir_all(&tmp).ok();
    }

    /// 改名搬家：老用户的 `~/.dsh-launcher` 要被整体搬到 `~/.dsh-starter`，内容一份不少；
    /// 新目录已存在时绝不覆盖（避免第二次启动把新数据盖掉）。
    #[test]
    fn legacy_home_is_migrated_once_and_never_overwrites_new() {
        let tmp = std::env::temp_dir().join(format!("dsh-home-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let old = tmp.join(LEGACY_HOME_DIR);
        fs::create_dir_all(old.join("versions")).unwrap();
        fs::write(old.join(SETTINGS_FILE), "{\"registry\":\"x\"}").unwrap();

        assert_eq!(migrate_home(&tmp), tmp.join(HOME_DIR));
        assert!(tmp.join(HOME_DIR).join(SETTINGS_FILE).is_file(), "设置要跟着搬");
        assert!(tmp.join(HOME_DIR).join("versions").is_dir(), "已装版本要跟着搬");
        assert!(!old.exists(), "旧目录应已搬走");

        // 幂等：再调一次还是新目录
        assert_eq!(migrate_home(&tmp), tmp.join(HOME_DIR));

        // 新目录已存在、旧目录又冒出来 → 保持新目录，不覆盖
        fs::create_dir_all(&old).unwrap();
        fs::write(tmp.join(HOME_DIR).join("keep.txt"), "new").unwrap();
        assert_eq!(migrate_home(&tmp), tmp.join(HOME_DIR));
        assert!(tmp.join(HOME_DIR).join("keep.txt").is_file());

        fs::remove_dir_all(&tmp).ok();
    }
}
