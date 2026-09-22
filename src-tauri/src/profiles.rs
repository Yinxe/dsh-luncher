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
    /// 桌面应用外壳：package.json 的 name 为桌面运行时包名，或 bundles 含 desktop 插件；启动方式预留
    Desktop,
    /// 未识别（无 package.json 或 name/bundles 中无已知 Target）
    Unknown,
}

/// Web 应用 bundle 的完整包名：bundles 数组中完整出现它才算 Web Target
const WEB_APP_BUNDLE: &str = "@deepseek-ai/dsh-web-app";

/// 桌面外壳 bundle 的完整包名（预留：官方定名后在此增补）
const DESKTOP_APP_BUNDLES: &[&str] = &["@deepseek-ai/dsh-desktop-app"];

/// 桌面运行时 profile 的 package.json name。官方 desktop profile 的 bundles 里同样带
/// `@deepseek-ai/dsh-web-app`（桌面外壳内嵌 Web 应用），只按 bundles 会被误判成 Web，
/// 因此这个精确包名要先于 bundles 判定。
const DESKTOP_RUNTIME_PACKAGES: &[&str] = &["@deepseek-ai/dsh-desktop-runtime"];

/// package.json 中 Target 识别用到的字段（dsh.profile.bundles 与顶层 name），
/// 交给 serde_json 严格反序列化，不做任何文本层面的模糊匹配
#[derive(Deserialize, Default)]
struct ProfilePackageJson {
    #[serde(default)]
    name: String,
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
    /// 由 package.json 的 name 与 dsh.profile.bundles 识别出的运行 Target
    pub target: ProfileTarget,
    /// dsh 内置保留 profile（不可改名/删除）
    pub reserved: bool,
}

/// 读 profile 的 package.json 中 Target 识别所需字段（缺文件/解析失败返回默认值）
fn read_profile_package(dir: &Path) -> ProfilePackageJson {
    let Ok(txt) = std::fs::read_to_string(dir.join("package.json")) else {
        return ProfilePackageJson::default();
    };
    serde_json::from_str::<ProfilePackageJson>(&txt).unwrap_or_default()
}

/// 识别 Target：先看 package.json 的 name（桌面运行时内嵌 web-app，必须优先判），
/// 再看 bundles 里的完整包名（@deepseek-ai/dsh-web-app ⇒ Web）
fn detect_target(dir: &Path) -> ProfileTarget {
    let pkg = read_profile_package(dir);
    if DESKTOP_RUNTIME_PACKAGES.contains(&pkg.name.as_str()) {
        return ProfileTarget::Desktop;
    }
    let bundles = &pkg.dsh.profile.bundles;
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
        // DirEntry::file_type() 不跟随符号链接：软链到目录 / yaml 的 profile 若不解析目标，
        // 既不是 dir 也不是 file 会被整个跳过，而 dsh 其实能透过软链运行它。这里解析后再判定；
        // 指向不存在目标的坏软链 is_dir / is_file 都为 false，会自然跳过。
        let is_dir = ft.is_dir() || (ft.is_symlink() && path.is_dir());
        let is_file = ft.is_file() || (ft.is_symlink() && path.is_file());
        if is_dir {
            let reserved = is_reserved_profile(&name);
            out.push(ProfileInfo {
                name: name.into_owned(),
                kind: "dir".into(),
                path: path.to_string_lossy().into_owned(),
                target: detect_target(&path),
                reserved,
            });
        } else if is_file {
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

    /// 首次使用引导的判据：全新机器上 `$DSH_HOME`（乃至 profiles 目录）都不存在，
    /// 扫描结果必须为空 —— 前端据此显示「初始化 dsh」引导卡，而不是让用户对着
    /// 空下拉框猜。dsh 第一次运行（`dsh web`）之后同样这个函数才会返回 web profile。
    #[test]
    fn fresh_home_has_no_profiles() {
        let tmp = std::env::temp_dir().join(format!("dsh-nohome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(
            scan_profiles_in(&tmp).is_empty(),
            "不存在的目录不应扫出任何 profile"
        );
        // 目录存在但没有 profile 也一样（用户把 profile 全删了）
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::create_dir_all(tmp.join("node_modules")).unwrap();
        assert!(scan_profiles_in(&tmp).is_empty(), "node_modules 不算 profile");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn scan_lists_symlinked_profile_dir_skips_broken_link() {
        let base = std::env::temp_dir().join(format!("dsh-symprof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("profiles");
        std::fs::create_dir_all(&root).unwrap();

        // root 之外的真实目录（含 package.json），软链进 root
        let real = base.join("real-profile");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("package.json"), r#"{"name":"p"}"#).unwrap();
        std::os::unix::fs::symlink(&real, root.join("linked")).unwrap();
        // 指向不存在目标的坏软链：应跳过、且不 panic
        std::os::unix::fs::symlink(base.join("missing"), root.join("broken")).unwrap();

        let names: Vec<String> = scan_profiles_in(&root).into_iter().map(|p| p.name).collect();
        assert!(
            names.contains(&"linked".to_string()),
            "软链目录应被识别为 profile：{names:?}"
        );
        assert!(
            !names.contains(&"broken".to_string()),
            "坏软链应被跳过：{names:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

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

        // name 为桌面运行时包名 ⇒ Desktop，即使 bundles 里带着 dsh-web-app
        // （官方 desktop profile 就是这种形态：桌面外壳内嵌 Web 应用）
        std::fs::write(
            tmp.join("package.json"),
            r#"{"name":"@deepseek-ai/dsh-desktop-runtime","dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Desktop);

        // name 近似（缺 scope / 多后缀）不算桌面运行时，仍按 bundles 判成 Web
        for near in ["dsh-desktop-runtime", "@deepseek-ai/dsh-desktop-runtime-x"] {
            let json = r#"{"name":"__NAME__","dsh":{"profile":{"bundles":["@deepseek-ai/dsh-web-app"]}}}"#
                .replace("__NAME__", near);
            std::fs::write(tmp.join("package.json"), json).unwrap();
            assert_eq!(detect_target(&tmp), ProfileTarget::Web, "near name={near}");
        }

        // 只有 name、没有 dsh 段 ⇒ 不崩，按 Unknown
        std::fs::write(tmp.join("package.json"), r#"{"name":"some-app"}"#).unwrap();
        assert_eq!(detect_target(&tmp), ProfileTarget::Unknown);

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 真实数据冒烟验证：逐个扫描 ~/.dsh/profiles，打印识别结果；
    /// 若存在 profiles/web / profiles/desktop，则必须分别识别为 Web / Desktop。
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
        let desktop = profiles.iter().find(|p| p.name == "desktop");
        if let Some(desktop) = desktop {
            assert_eq!(desktop.target, ProfileTarget::Desktop);
        }
    }
}
