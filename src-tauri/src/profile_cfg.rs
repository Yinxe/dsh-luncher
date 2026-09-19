use tauri::Emitter;
use crate::settings::Settings;
use crate::util;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::profiles;

/// profile 内插件 bundle（package.json 的 dsh.profile.bundles）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BundleInfo {
    pub name: String,
    pub version: Option<String>,
    /// npm（registry 依赖）/ link（本地链接）
    pub source: String,
    pub enabled: bool,
}

/// cordis.patch.yml 顶层条目摘要（只读展示；编辑走原始文本保注释）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PatchEntryInfo {
    pub index: usize,
    /// config（按 id 覆盖/禁用）| insert（插入插件实例）
    pub kind: String,
    pub id: Option<String>,
    pub disabled: bool,
    /// insert 条目内的插件实例
    pub items: Vec<PatchItemInfo>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PatchItemInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub disabled: bool,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetail {
    pub profile: String,
    pub exists: bool,
    pub bundles: Vec<BundleInfo>,
    pub package_raw: String,
    pub patch_raw: String,
    pub patch_entries: Vec<PatchEntryInfo>,
}

fn profile_dir(profile: &str) -> Result<PathBuf, String> {
    let name = profile.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err("非法 profile 名称".into());
    }
    Ok(profiles::profiles_dir().join(name))
}

fn source_of(version_value: Option<&str>) -> &'static str {
    match version_value {
        Some(v) if v.starts_with("link:") => "link",
        Some(v) if v.starts_with("workspace:") => "workspace",
        Some(v) if v.starts_with("file:") => "file",
        _ => "npm",
    }
}

/// 读取 profile 的插件与配置详情
pub fn read_detail(profile: &str) -> Result<ProfileDetail, String> {
    let dir = profile_dir(profile)?;
    let pkg_path = dir.join("package.json");
    let patch_path = dir.join("cordis.patch.yml");

    let package_raw = std::fs::read_to_string(&pkg_path).unwrap_or_default();
    let patch_raw = std::fs::read_to_string(&patch_path).unwrap_or_default();
    let exists = pkg_path.is_file();

    let patch_disabled = patch_disabled_ids(profile);
    let mut bundles = Vec::new();
    if !package_raw.is_empty() {
        let pkg: serde_json::Value =
            serde_json::from_str(&package_raw).map_err(|e| format!("package.json 解析失败: {e}"))?;
        let deps = pkg.get("dependencies").and_then(|v| v.as_object());
        if let Some(list) = pkg
            .get("dsh")
            .and_then(|d| d.get("profile"))
            .and_then(|p| p.get("bundles"))
            .and_then(|b| b.as_array())
        {
            for b in list {
                if let Some(name) = b.as_str() {
                    let dep_ver = deps
                        .and_then(|d| d.get(name))
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    bundles.push(BundleInfo {
                        name: name.to_string(),
                        version: dep_ver.clone(),
                        source: source_of(dep_ver.as_deref()).to_string(),
                        enabled: !patch_disabled.contains(name),
                    });
                }
            }
        }
    }

    let mut patch_entries = Vec::new();
    if !patch_raw.is_empty() {
        if let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(&patch_raw)
        {
            for (idx, item) in seq.iter().enumerate() {
                let kind = if item.get("insert").is_some() {
                    "insert"
                } else {
                    "config"
                };
                let disabled = item
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let id = item
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let mut items = Vec::new();
                if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
                    for it in inner {
                        items.push(PatchItemInfo {
                            id: it.get("id").and_then(|v| v.as_str()).map(String::from),
                            name: it.get("name").and_then(|v| v.as_str()).map(String::from),
                            disabled: it
                                .get("disabled")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        });
                    }
                }
                patch_entries.push(PatchEntryInfo {
                    index: idx,
                    kind: kind.into(),
                    id,
                    disabled,
                    items,
                });
            }
        }
    }

    Ok(ProfileDetail {
        profile: profile.to_string(),
        exists,
        bundles,
        package_raw,
        patch_raw,
        patch_entries,
    })
}

/// 用户 cordis.patch.yml 中被禁用的条目 id 集合
fn patch_disabled_ids(profile: &str) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    let Ok(raw) = std::fs::read_to_string(user_patch_path(profile).unwrap_or_default()) else {
        return set;
    };
    if let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
        for item in seq {
            let disabled = item.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
            if !disabled {
                continue;
            }
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                set.insert(id.to_string());
            }
            if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
                for it in inner {
                    if it.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false) {
                        if let Some(id) = it.get("id").and_then(|v| v.as_str()) {
                            set.insert(id.to_string());
                        }
                    }
                }
            }
        }
    }
    set
}

/// 覆写前先备份原文件（.launcher-bak-<时间戳>）
fn backup(path: &Path) -> Result<(), String> {
    if let Ok(content) = std::fs::read(path) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak = path.with_extension(format!("launcher-bak-{ts}"));
        std::fs::write(&bak, content).map_err(|e| format!("写备份失败: {e}"))?;
    }
    Ok(())
}

fn validate_yaml(content: &str) -> Result<(), String> {
    serde_yaml::from_str::<serde_yaml::Value>(content)
        .map(|_| ())
        .map_err(|e| format!("YAML 语法错误：{e}"))
}

fn validate_json(content: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(content)
        .map(|_| ())
        .map_err(|e| format!("JSON 语法错误：{e}"))
}

/// 修改 bundle 启用状态：通过用户 cordis.patch 层 `disabled: true`，
/// 条目保留在 bundles 列表中（禁用 ≠ 移除，卸载才是真移除）
pub fn set_bundle_enabled(profile: &str, name: &str, enabled: bool) -> Result<(), String> {
    set_plugin_disabled(profile, name, !enabled)
}

/// 解析用于插件命令的 dsh 可执行入口（当前版本优先）
pub fn resolve_dsh_bin(settings: &Settings) -> Result<(PathBuf, PathBuf), String> {
    let installed = crate::installed::collect_installed(settings);
    let active = settings.active_version.trim();
    let target = if active.is_empty() {
        None
    } else {
        installed.iter().find(|i| i.version == active)
    };
    let target = match target {
        Some(t) => t.clone(),
        None => crate::installed::pick_latest(&installed)
            .ok_or("未找到已安装的 dsh，无法执行插件命令")?,
    };
    let bin_js = target
        .bin_js
        .clone()
        .ok_or("该版本缺少 bin.js，无法执行插件命令")?;
    let node = util::find_node(settings).ok_or("未找到 Node.js，无法执行插件命令")?;
    Ok((node, PathBuf::from(bin_js)))
}

/// 执行官方插件命令：dsh plugin --profile <p> <pnpm-args...>
/// 安装 = add <pkg>；卸载 = remove <pkg>；升级 = update <pkg>
pub fn run_plugin_cli(
    settings: &Settings,
    profile: &str,
    pnpm_args: &[&str],
) -> Result<String, String> {
    let (node, bin_js) = resolve_dsh_bin(settings)?;
    let dir = profile_dir(profile)?;
    if !dir.join("package.json").is_file() {
        return Err("该 profile 尚未初始化（缺少 package.json），请先启动一次".into());
    }
    let out = std::process::Command::new(&node)
        .arg(&bin_js)
        .arg("plugin")
        .arg("--profile")
        .arg(profile)
        .args(pnpm_args)
        .current_dir(&dir)
        .env(
            "DSH_HOME",
            profiles::dsh_native_home().to_string_lossy().into_owned(),
        )
        .output()
        .map_err(|e| format!("执行 dsh plugin 失败: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        let tail: Vec<&str> = text.lines().rev().take(10).collect();
        return Err(format!(
            "命令失败（dsh plugin {}）:\n{}",
            pnpm_args.join(" "),
            tail.iter().rev().cloned().collect::<Vec<_>>().join("\n")
        ));
    }
    let tail: Vec<&str> = text.lines().rev().take(6).collect();
    Ok(tail.iter().rev().cloned().collect::<Vec<_>>().join("\n"))
}

/// 后台执行官方插件命令（不阻塞），输出逐行以 plugin-log 事件回传
pub fn start_plugin_cli(
    app: tauri::AppHandle,
    settings: &Settings,
    profile: &str,
    pnpm_args: &[&str],
) -> Result<(), String> {
    let (node, bin_js) = resolve_dsh_bin(settings)?;
    let dir = profile_dir(profile)?;
    if !dir.join("package.json").is_file() {
        return Err("该 profile 尚未初始化（缺少 package.json），请先启动一次".into());
    }
    let profile = profile.to_string();
    let args: Vec<String> = pnpm_args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let emit_line = |line: &str, done: bool, ok: bool| {
            let _ = app.emit(
                "plugin-log",
                serde_json::json!({ "profile": profile, "line": line, "done": done, "ok": ok }),
            );
        };
        emit_line(&format!("$ dsh plugin --profile {} {}", profile, args.join(" ")), false, true);
        let dsh_home = profiles::dsh_native_home().to_string_lossy().into_owned();
        let mut child = Command::new(&node);
        child
            .arg(&bin_js)
            .arg("plugin")
            .arg("--profile")
            .arg(&profile)
            .args(&args)
            .current_dir(&dir)
            .env("DSH_HOME", dsh_home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match child.output() {
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                for line in text.lines().filter(|l| !l.trim().is_empty()).take(40) {
                    emit_line(line, false, true);
                }
                let ok = o.status.success();
                emit_line(
                    if ok { "✔ 完成" } else { "✘ 失败" },
                    true,
                    ok,
                );
            }
            Err(e) => emit_line(&format!("✘ 执行失败: {e}"), true, false),
        }
    });
    Ok(())
}

/// 卸载插件：官方命令 dsh plugin --profile <p> remove <pkg>（后台执行）
pub fn uninstall_bundle(
    app: tauri::AppHandle,
    settings: &Settings,
    profile: &str,
    name: &str,
) -> Result<(), String> {
    start_plugin_cli(app, settings, profile, &["remove", name])
}

/// 安装插件：官方命令 dsh plugin --profile <p> add <pkg>（后台执行）
pub fn install_bundle(
    app: tauri::AppHandle,
    settings: &Settings,
    profile: &str,
    name: &str,
) -> Result<(), String> {
    start_plugin_cli(app, settings, profile, &["add", name])
}

/// 读 profile 内的文本文件（仅 cordis.patch.yml —— package.json 只归 dsh plugin 命令管）
pub fn read_profile_file(profile: &str, file: &str) -> Result<String, String> {
    let allowed = ["cordis.patch.yml"];
    if !allowed.contains(&file) {
        return Err("不允许读取该文件".into());
    }
    let path = profile_dir(profile)?.join(file);
    std::fs::read_to_string(path).map_err(|e| format!("读取失败: {e}"))
}

/// 写 profile 内的文本文件（先备份 + 语法校验）
pub fn write_profile_file(profile: &str, file: &str, content: &str) -> Result<(), String> {
    let allowed = ["cordis.patch.yml"];
    if !allowed.contains(&file) {
        return Err("不允许写入该文件".into());
    }
    if file.ends_with(".yml") {
        validate_yaml(content)?;
    } else {
        validate_json(content)?;
    }
    let path = profile_dir(profile)?.join(file);
    backup(&path)?;
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

/// 全局配置 ~/.dsh/settings.yaml
pub fn global_config_path() -> PathBuf {
    profiles::dsh_native_home().join("settings.yaml")
}

pub fn read_global_config() -> Result<String, String> {
    std::fs::read_to_string(global_config_path()).map_err(|e| format!("读取失败: {e}"))
}

pub fn write_global_config(content: &str) -> Result<(), String> {
    validate_yaml(content)?;
    let path = global_config_path();
    backup(&path)?;
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

// ── 基于 cordis.patch 的插件动态启停 ─────────────────────

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntryInfo {
    /// 服务/插件 id（bundle patch 列表或用户 patch 中的 id）
    pub id: String,
    /// 提供该插件的 bundle 包名
    pub bundle: Option<String>,
    /// 用户 patch 层是否禁用了它
    pub disabled: bool,
    /// 该禁用条目是否由启动器管理（可通过开关自动移除）
    pub managed: bool,
    /// true = bundle 插件包；false = bundle 内的服务插件
    pub is_bundle: bool,
}

const MANAGE_MARKER: &str = "# dsh-launcher: disable";

fn user_patch_path(profile: &str) -> Result<PathBuf, String> {
    Ok(profile_dir(profile)?.join("cordis.patch.yml"))
}

/// 收集 profile 可启停的插件清单：
/// 1) 各 bundle 包自带 cordis.patch.yml 中的服务 id；
/// 2) 用户 patch 层中出现的 id（含启动器追加的禁用块）。
pub fn plugin_inventory(profile: &str) -> Result<Vec<PluginEntryInfo>, String> {
    let dir = profile_dir(profile)?;
    let pkg_raw =
        std::fs::read_to_string(dir.join("package.json")).map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let pkg: serde_json::Value =
        serde_json::from_str(&pkg_raw).map_err(|e| format!("package.json 解析失败: {e}"))?;
    let bundles: Vec<String> = pkg
        .get("dsh")
        .and_then(|d| d.get("profile"))
        .and_then(|p| p.get("bundles"))
        .and_then(|b| b.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut out: Vec<PluginEntryInfo> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let patch_disabled = patch_disabled_ids(profile);

    // 0) bundle 包本身也是插件条目（包名 ≠ 服务 id，禁用走 patch 层）
    for bundle in &bundles {
        if seen.insert(bundle.clone()) {
            out.push(PluginEntryInfo {
                id: bundle.clone(),
                bundle: Some(bundle.clone()),
                disabled: patch_disabled.contains(bundle),
                managed: true,
                is_bundle: true,
            });
        }
    }

    // 1) 每个 bundle 自带 patch 列表中的服务 id
    for bundle in &bundles {
        let bundle_patch = dir.join("node_modules").join(bundle).join("cordis.patch.yml");
        let Ok(text) = std::fs::read_to_string(&bundle_patch) else {
            continue;
        };
        if let Ok(serde_yaml::Value::Sequence(seq)) =
            serde_yaml::from_str::<serde_yaml::Value>(&text)
        {
            for item in seq {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if seen.insert(id.to_string()) {
                        out.push(PluginEntryInfo {
                            id: id.to_string(),
                            bundle: Some(bundle.clone()),
                            disabled: patch_disabled.contains(id),
                            managed: false,
                            is_bundle: false,
                        });
                    }
                }
            }
        }
    }

    // 2) 用户 patch 层：解析禁用状态与额外 id
    let patch_path = user_patch_path(profile)?;
    let raw = std::fs::read_to_string(&patch_path).unwrap_or_default();
    if !raw.is_empty() {
        if let Ok(serde_yaml::Value::Sequence(seq)) =
            serde_yaml::from_str::<serde_yaml::Value>(&raw)
        {
            for item in seq {
                let id = item.get("id").and_then(|v| v.as_str()).map(String::from);
                let disabled = item
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if let Some(id) = id.clone() {
                    if let Some(e) = out.iter_mut().find(|e| e.id == id) {
                        if disabled {
                            e.disabled = true;
                            e.managed = raw.contains(&format!("{MANAGE_MARKER} {id}"));
                        }
                    } else if seen.insert(id.clone()) {
                        let managed = raw.contains(&format!("{MANAGE_MARKER} {id}"));
                        out.push(PluginEntryInfo {
                            id,
                            bundle: None,
                            disabled,
                            managed,
                            is_bundle: false,
                        });
                    }
                }
            }
        }
    }
    Ok(out)
}

/// 读取 profile 的 patchReload 生命周期：live（默认）| startup
pub fn patch_reload_mode(profile: &str) -> String {
    let Ok(raw) = std::fs::read_to_string(profile_dir(profile).unwrap_or_default().join("package.json"))
    else {
        return "live".into();
    };
    serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|j| {
            j.get("dsh")?
                .get("profile")?
                .get("patchReload")?
                .as_str()
                .map(String::from)
        })
        .unwrap_or_else(|| "live".into())
}

/// 通过用户 cordis.patch.yml 启停插件：
/// 禁用 = 追加启动器管理的注释标记块；启用 = 按标记移除该块（不动用户手写内容）。
pub fn set_plugin_disabled(profile: &str, id: &str, disabled: bool) -> Result<(), String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 200 || id.contains('\n') || id.contains("..") {
        return Err("非法插件 id".into());
    }
    let path = user_patch_path(profile)?;
    let raw = std::fs::read_to_string(&path).unwrap_or_default();

    if disabled {
        // 已存在禁用（无论谁写的）就不再追加
        if plugin_inventory(profile)?
            .iter()
            .any(|e| e.id == id && e.disabled)
        {
            return Ok(());
        }
        let mut out = raw.clone();
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!(
            "\n{MANAGE_MARKER} {id}（启动器管理：关闭开关会自动移除此块；live 模式即时生效，startup 模式需重启实例）\n- id: {id}\n  disabled: true\n"
        ));
        validate_yaml(&out)?;
        backup(&path)?;
        std::fs::write(&path, out).map_err(|e| format!("写入失败: {e}"))?;
        return Ok(());
    }

    // 启用：移除启动器管理的块（标记行 + 条目行 + 缩进行，直到下一个顶层元素）
    let marker = format!("{MANAGE_MARKER} {id}");
    let mut out_lines: Vec<&str> = Vec::new();
    let mut skipping = false;
    let mut seen_entry_line = false;
    let mut found = false;
    for line in raw.lines() {
        if !skipping {
            if line.trim_start().starts_with(&marker) {
                skipping = true;
                found = true;
            } else {
                out_lines.push(line);
            }
            continue;
        }
        // 管理块内部：条目行（"- id: ..."）与其缩进行都跳过
        if !seen_entry_line {
            if line.starts_with("- ") {
                seen_entry_line = true;
            }
            continue;
        }
        // 条目行之后遇到非空、非缩进的行 => 管理块结束
        if !line.trim().is_empty() && !line.starts_with(' ') {
            skipping = false;
            out_lines.push(line);
        }
    }
    if !found {
        let manual = plugin_inventory(profile)?
            .iter()
            .any(|e| e.id == id && e.disabled);
        return Err(if manual {
            format!("插件 {id} 的禁用条目是手动添加的，请直接编辑 cordis.patch.yml 移除 `disabled: true`")
        } else {
            format!("插件 {id} 本就处于启用状态")
        });
    }
    let mut out = out_lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    validate_yaml(&out)?;
    backup(&path)?;
    std::fs::write(&path, out).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_toggle_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("dsh-cfg-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(prof_dir.join("node_modules/@deepseek-ai/dsh-web-app")).unwrap();
        std::fs::write(
            prof_dir.join("package.json"),
            r#"{"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-web-app"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "# my notes\n- id: webserver\n  config:\n    host: 0.0.0.0\n",
        )
        .unwrap();
        std::fs::write(
            prof_dir.join("node_modules/@deepseek-ai/dsh-web-app/cordis.patch.yml"),
            "- id: webserver\n- id: web-runtime\n- id: connection\n",
        )
        .unwrap();

        // 清单：bundle 服务 id 被收集
        let inv = plugin_inventory("web").unwrap();
        assert!(inv.iter().any(|e| e.id == "webserver" && !e.disabled));
        assert!(inv
            .iter()
            .any(|e| e.id == "web-runtime" && e.bundle.as_deref() == Some("@deepseek-ai/dsh-web-app")));

        // 禁用：追加管理块，用户注释保留
        set_plugin_disabled("web", "web-runtime", true).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"), "用户注释必须保留");
        assert!(raw.contains("dsh-launcher: disable web-runtime"));
        assert!(plugin_inventory("web")
            .unwrap()
            .iter()
            .find(|e| e.id == "web-runtime")
            .unwrap()
            .disabled);

        // 连续禁用不重复追加
        set_plugin_disabled("web", "web-runtime", true).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert_eq!(raw.matches("dsh-launcher: disable").count(), 1);

        // 启用：管理块被移除，注释与 webserver 条目保留
        set_plugin_disabled("web", "web-runtime", false).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"));
        assert!(raw.contains("- id: webserver"));
        assert!(!raw.contains("dsh-launcher: disable"));
        assert!(!raw.contains("disabled: true"));
        assert!(!plugin_inventory("web")
            .unwrap()
            .iter()
            .find(|e| e.id == "web-runtime")
            .unwrap()
            .disabled);

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }
}
