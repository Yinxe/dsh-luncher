use serde::Serialize;
use std::path::{Path, PathBuf};

/// dsh 自身的数据目录：$DSH_HOME，缺省 ~/.dsh
pub fn dsh_native_home() -> PathBuf {
    if let Ok(h) = std::env::var("DSH_HOME") {
        let h = h.trim();
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    crate::util::home_dir()
        .map(|h| h.join(".dsh"))
        .unwrap_or_else(|| PathBuf::from(".dsh"))
}

/// dsh 的 profile 目录：$DSH_HOME/profiles（旧版/兼容单数 profile）
pub fn profiles_dir() -> PathBuf {
    let home = dsh_native_home();
    let plural = home.join("profiles");
    if plural.is_dir() {
        return plural;
    }
    home.join("profile")
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// 传给 --profile 的名字
    pub name: String,
    /// dir = profiles/ 下的子目录；file = yaml/json 配置文件（名字去掉扩展名）
    pub kind: String,
    pub path: String,
    /// bundles 中包含 @deepseek-ai/dsh-web-app 即为 web 类型
    pub web_type: bool,
}

/// bundles 含 @deepseek-ai/dsh-web-app ⇒ web 类型 profile
fn is_web_type(dir: &Path) -> bool {
    let Ok(txt) = std::fs::read_to_string(dir.join("package.json")) else {
        return false;
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(&txt) else {
        return false;
    };
    let Some(bundles) = j
        .get("dsh")
        .and_then(|d| d.get("profile"))
        .and_then(|p| p.get("bundles"))
        .and_then(|b| b.as_array())
    else {
        return false;
    };
    bundles
        .iter()
        .any(|v| v.as_str() == Some("@deepseek-ai/dsh-web-app"))
}

/// 枚举可启动的 profile：profiles/ 下的子目录 + yaml/yml/json 文件（按名字排序）
pub fn scan_profiles() -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    let dir = profiles_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let name_os = entry.file_name();
        let name = name_os.to_string_lossy();
        // 跳过隐藏项与依赖目录
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if ft.is_dir() {
            out.push(ProfileInfo {
                name: name.into_owned(),
                kind: "dir".into(),
                path: path.to_string_lossy().into_owned(),
                web_type: is_web_type(&path),
            });
        } else if ft.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            if matches!(ext.as_str(), "yaml" | "yml" | "json") {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.into_owned());
                out.push(ProfileInfo {
                    name: stem,
                    kind: "file".into(),
                    path: path.to_string_lossy().into_owned(),
                    web_type: false,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
