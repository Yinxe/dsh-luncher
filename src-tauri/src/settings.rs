use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

/// 持久化在 ~/.dsh-launcher/settings.json 的启动器设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// npm registry 地址，可换成镜像（如 https://registry.npmmirror.com）
    pub registry: String,
    /// 启动器自身更新清单地址（返回 {version, notes, url} 的 JSON）
    pub update_manifest_url: String,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            registry: "https://registry.npmjs.org".into(),
            update_manifest_url: String::new(),
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
    if let Ok(h) = std::env::var("DSH_LAUNCHER_HOME") {
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
pub fn launcher_home() -> PathBuf {
    home_dir()
        .map(|h| h.join(".dsh-launcher"))
        .unwrap_or_else(|| PathBuf::from(".dsh-launcher"))
}

/// 启动器管理的 dsh 版本安装根目录
pub fn versions_dir() -> PathBuf {
    launcher_home().join("versions")
}

pub fn settings_path() -> PathBuf {
    launcher_home().join("settings.json")
}

pub fn load_settings() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("写入设置失败: {e}"))?;
    Ok(())
}
