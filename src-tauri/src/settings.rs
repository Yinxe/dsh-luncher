use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
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
    /// Profile 启动方式：child=子进程（随启动器退出）| detached=独立进程（后台常驻，默认）
    pub launch_mode: String,
    /// 按 profile 的启动方式覆盖（键=profile 名，值 child|detached）；未覆盖的走全局 launch_mode
    #[serde(default)]
    pub profile_launch_mode: std::collections::HashMap<String, String>,
    /// Web UI 打开方式：window=应用内独立窗口（默认）| browser=系统默认浏览器
    #[serde(default = "default_web_open_mode")]
    pub web_open_mode: String,
    /// 按 profile 的打开方式覆盖（键=profile 名，值 window|browser）；仅 Web 类型 profile 有意义
    #[serde(default)]
    pub profile_web_open_mode: std::collections::HashMap<String, String>,
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
    /// 系统日志级别：debug | info | warn | error（默认 info）。
    /// 保存后立即生效；环境变量 DSH_STARTER_LOG 存在时覆盖此设置。
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_true() -> bool {
    true
}

/// Web UI 默认在应用内独立窗口打开
fn default_web_open_mode() -> String {
    "window".into()
}

/// 日志级别默认 INFO（与 diag.rs 的缺省行为一致）
fn default_log_level() -> String {
    "info".into()
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
            (
                "profile_launch_mode".into(),
                if self.profile_launch_mode.is_empty() { "（无覆盖）".into() } else { format!("{} 项覆盖", self.profile_launch_mode.len()) },
            ),
            ("web_open_mode".into(), self.web_open_mode.clone()),
            (
                "profile_web_open_mode".into(),
                if self.profile_web_open_mode.is_empty() { "（无覆盖）".into() } else { format!("{} 项覆盖", self.profile_web_open_mode.len()) },
            ),
            ("log_level".into(), self.log_level.clone()),
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
            launch_mode: "detached".into(),
            profile_launch_mode: Default::default(),
            web_open_mode: default_web_open_mode(),
            profile_web_open_mode: Default::default(),
            log_level: default_log_level(),
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

/// 启动器数据根目录（区别于 dsh 自身的 ~/.dsh）
///
/// 0.2.0 起叫 `~/.dsh-starter`（旧名 `~/.dsh-launcher`）。**刻意不做旧目录迁移与兼容**：
/// 改名那一版本来就没有用户，留一套搬运/兜底代码只会变成长期包袱 —— 需要旧数据的人
/// 手动把目录改个名即可。
pub fn starter_home() -> PathBuf {
    home_dir()
        .map(|h| h.join(".dsh-starter"))
        .unwrap_or_else(|| PathBuf::from(".dsh-starter"))
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
    let path = settings_path();
    let raw = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Settings::default(), // 不存在：全新环境，正常
    };
    match serde_json::from_str::<Settings>(&raw) {
        Ok(s) => s,
        Err(e) => {
            // 回退默认值会丢 github_token / registry 等全部设置，必须留下痕迹
            crate::diag::warn(
                "app",
                &format!("设置文件解析失败，本次以默认设置运行（保存设置会覆盖）：{}：{e}", path.display()),
            );
            Settings::default()
        }
    }
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
    // 只记路径与字节数：settings.json 里可能存有 github_token
    crate::diag::debug("app", || format!("设置已保存：{}（{} 字节）", path.display(), text.len()));
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

    /// 按 profile 的启动/打开方式覆盖表要能落盘并读回；老 settings.json 没这两个键时默认空表
    #[test]
    fn profile_mode_maps_round_trip_and_default_to_empty() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-settings-modes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        let mut s = Settings::default();
        s.profile_launch_mode
            .insert("web".into(), "child".into());
        s.profile_web_open_mode
            .insert("web-try".into(), "browser".into());
        save_settings(&s).unwrap();
        let back = load_settings();
        assert_eq!(back.profile_launch_mode.get("web").map(String::as_str), Some("child"));
        assert_eq!(
            back.profile_web_open_mode.get("web-try").map(String::as_str),
            Some("browser")
        );

        // 旧版本写出的 settings.json（没有这两个键）必须仍能解析，且得到空表
        let path = settings_path();
        let mut legacy: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("profileLaunchMode");
        legacy.as_object_mut().unwrap().remove("profileWebOpenMode");
        fs::write(&path, legacy.to_string()).unwrap();
        let back = load_settings();
        assert!(back.profile_launch_mode.is_empty());
        assert!(back.profile_web_open_mode.is_empty());
        assert_eq!(back.launch_mode, "detached");

        std::env::remove_var("DSH_STARTER_HOME");
        fs::remove_dir_all(&tmp).ok();
    }

    /// 数据目录就是 `~/.dsh-starter`：改名不做旧目录迁移（有旧数据的人自己改个目录名）
    #[test]
    fn starter_home_is_the_new_dir_and_ignores_the_legacy_one() {
        let tmp = std::env::temp_dir().join(format!("dsh-home-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join(".dsh-launcher")).unwrap();
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("DSH_STARTER_HOME", &tmp);

        assert_eq!(starter_home(), tmp.join(".dsh-starter"));
        assert!(
            tmp.join(".dsh-launcher").exists(),
            "旧目录不该被动过：不搬运、不删除"
        );

        std::env::remove_var("DSH_STARTER_HOME");
        fs::remove_dir_all(&tmp).ok();
    }
}
