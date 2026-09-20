use serde::{Deserialize, Serialize};
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

/// profile 的运行 Target：不同 Target 有各自的启动方式，按此扩展。
/// 目前仅实现 Web（启动后从日志识别地址并打开浏览器），其余为预留。
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ProfileTarget {
    /// Web 应用：bundles 含 @deepseek-ai/dsh-web-app，启动方式 = 打开浏览器
    Web,
    /// 桌面应用外壳：bundles 含 desktop 插件，启动方式预留
    Desktop,
    /// 未识别（无 package.json 或 bundles 中无已知 Target 插件）
    Unknown,
}

/// Web 应用 bundle 的完整包名：bundles 数组中完整出现它才算 Web Target
const WEB_APP_BUNDLE: &str = "@deepseek-ai/dsh-web-app";

/// 桌面外壳 bundle 的完整包名（预留：官方定名后在此增补）
const DESKTOP_APP_BUNDLES: &[&str] = &["@deepseek-ai/dsh-desktop-app"];

/// package.json 中 dsh.profile.bundles 的类型化结构，
/// 交给 serde_json 严格反序列化，不做任何文本层面的模糊匹配
#[derive(Deserialize, Default)]
struct ProfilePackageJson {
    #[serde(default)]
    dsh: DshSection,
}

#[derive(Deserialize, Default)]
struct DshSection {
    #[serde(default)]
    profile: ProfileSection,
}

#[derive(Deserialize, Default)]
struct ProfileSection {
    #[serde(default)]
    bundles: Vec<String>,
}

/// dsh 内置保留 profile：改名/删除会破坏 dsh 核心数据，启动器一律禁止。
/// 判断不区分大小写（避免 `Web` 这种同名变体绕过保护）。
pub const RESERVED_PROFILES: &[&str] = &["headless", "web", "desktop"];

/// 是否为 dsh 内置保留 profile
pub fn is_reserved_profile(name: &str) -> bool {
    let n = name.trim();
    RESERVED_PROFILES.iter().any(|r| r.eq_ignore_ascii_case(n))
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// 传给 --profile 的名字
    pub name: String,
    /// dir = profiles/ 下的子目录；file = yaml/json 配置文件（名字去掉扩展名）
    pub kind: String,
    pub path: String,
    /// 由 package.json 的 dsh.profile.bundles 识别出的运行 Target
    pub target: ProfileTarget,
    /// dsh 内置保留 profile（不可改名/删除）
    pub reserved: bool,
}

/// 读 profile 的 dsh.profile.bundles 列表（缺文件/解析失败返回空）
fn read_bundles(dir: &Path) -> Vec<String> {
    let Ok(txt) = std::fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<ProfilePackageJson>(&txt)
        .map(|pkg| pkg.dsh.profile.bundles)
        .unwrap_or_default()
}

/// 按 bundles 识别 Target：完整包名精确匹配（@deepseek-ai/dsh-web-app ⇒ Web）
fn detect_target(dir: &Path) -> ProfileTarget {
    let bundles = read_bundles(dir);
    if bundles.iter().any(|b| b == WEB_APP_BUNDLE) {
        return ProfileTarget::Web;
    }
    if bundles
        .iter()
        .any(|b| DESKTOP_APP_BUNDLES.contains(&b.as_str()))
    {
        return ProfileTarget::Desktop;
    }
    ProfileTarget::Unknown
}

/// 枚举可启动的 profile：profiles/ 下的子目录 + yaml/yml/json 文件（按名字排序）
pub fn scan_profiles() -> Vec<ProfileInfo> {
    scan_profiles_in(&profiles_dir())
}

/// 同 scan_profiles，但目录由调用方给定（便于测试与真实数据验证）
pub fn scan_profiles_in(dir: &Path) -> Vec<ProfileInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
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
            let reserved = is_reserved_profile(&name);
            out.push(ProfileInfo {
                name: name.into_owned(),
                kind: "dir".into(),
                path: path.to_string_lossy().into_owned(),
                target: detect_target(&path),
                reserved,
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
                let reserved = is_reserved_profile(&stem);
                out.push(ProfileInfo {
                    name: stem,
                    kind: "file".into(),
                    path: path.to_string_lossy().into_owned(),
                    target: ProfileTarget::Unknown,
                    reserved,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_target_by_bundles() {
        let tmp = std::env::temp_dir().join(format!("dsh-target-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        // 无 package.json ⇒ Unknown
        assert_eq!(detect_target(&tmp), ProfileTarget::Unknown);

        let pkg = |bundles: &str| {
            format!(r#"{{"dsh":{{"profile":{{"bundles":[{bundles}]}}}}}}"#)
        };
        // 含 web-app 完整包名 ⇒ Web
        std::fs::write(
            tmp.join("package.json"),
            pkg(r#""@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app""#),
        )
        .unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Web);

        // 近似名不算 Web：多后缀 / 缺 scope 都不是完整包名
        for near in [
            r#""@deepseek-ai/dsh-web-app-x""#,
            r#""dsh-web-app""#,
            r#""@deepseek-ai/dsh-web""#,
        ] {
            std::fs::write(tmp.join("package.json"), pkg(near)).unwrap();
            assert_eq!(detect_target(&tmp), ProfileTarget::Unknown, "near={near}");
        }

        // 只出现在 dependencies、bundles 中没有 ⇒ 不算
        std::fs::write(
            tmp.join("package.json"),
            r#"{"dependencies":{"@deepseek-ai/dsh-web-app":"1.0.0"}}"#,
        )
        .unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Unknown);

        // 含桌面外壳完整包名 ⇒ Desktop
        std::fs::write(
            tmp.join("package.json"),
            pkg(r#""@deepseek-ai/dsh-base","@deepseek-ai/dsh-desktop-app""#),
        )
        .unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Desktop);

        // 其他含 desktop 字样的包名不算（无模糊匹配）
        std::fs::write(tmp.join("package.json"), pkg(r#""@vendor/my-desktop-tool""#)).unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Unknown);

        // 无已知 Target 插件（如 headless）⇒ Unknown
        std::fs::write(
            tmp.join("package.json"),
            pkg(r#""@deepseek-ai/dsh-base","@deepseek-ai/dsh-headless""#),
        )
        .unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Unknown);

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 真实数据冒烟验证：逐个扫描 ~/.dsh/profiles，打印识别结果；
    /// 若存在 profiles/web，则必须识别为 Web。
    #[test]
    fn real_home_profiles_targets() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let dir = PathBuf::from(home).join(".dsh/profiles");
        if !dir.is_dir() {
            return;
        }
        let profiles = scan_profiles_in(&dir);
        for p in &profiles {
            println!("{:>14} -> {:?}", p.name, p.target);
        }
        let web = profiles.iter().find(|p| p.name == "web");
        if let Some(web) = web {
            assert_eq!(web.target, ProfileTarget::Web);
        }
    }
}
