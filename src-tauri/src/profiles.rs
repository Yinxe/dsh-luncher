use serde::Serialize;
use std::path::PathBuf;

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

/// dsh 的 profile 目录：$DSH_HOME/profile
pub fn profiles_dir() -> PathBuf {
    dsh_native_home().join("profile")
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// 传给 --profile 的名字
    pub name: String,
    /// dir = profile/ 下的子目录；file = yaml/json 配置文件（名字去掉扩展名）
    pub kind: String,
    pub path: String,
}

/// 枚举可启动的 profile：profile/ 下的子目录 + yaml/yml/json 文件（按名字排序）
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
        if name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            out.push(ProfileInfo {
                name: name.into_owned(),
                kind: "dir".into(),
                path: path.to_string_lossy().into_owned(),
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
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
