//! 安装 / 卸载的**前后置校验与修复**。
//!
//! 细节对齐同生态的 dshmarket（`src/install.ts:validateAddedPlugins/removeAndReconcile`、
//! `src/profile.ts:hasLoadableEntry/conflictingEntryIds/dropFromManifest`、
//! `src/routes.ts` 的 uninstall 路由）。它踩过的坑都值得直接抄：
//!
//! - **假成功**：pnpm 退出 0 也可能装了个「没有 dsh 清单 / 没有可加载入口」的东西
//!   （源码检出、构建脚本被 allowBuilds 拦住），这会在**下次启动**把整个 profile 弄挂，
//!   所以装完立刻核对，不合格的当场卸掉。
//! - **入口 id 冲突**：cordis 不允许同一 id 有两个 `insert` 条目；把一个 TUI 插件装进
//!   web profile 就会让 dsh **启动不了**，而报错里两个插件都不提。装完必须比对。
//! - **清单残留**：remove 可能「事都做完了但退出码非 0」，也可能退出 0 却没走到清单
//!   和解 —— 两种都会在 `dsh.profile.bundles` 留下一行指向已不存在的包，而 loader
//!   遇到第一行就整体启动失败。以**磁盘事实**为准来修：包没了就删行，包还在就留行
//!   （留着重试才有东西可重试）。
//! - **卸载前的用户补丁检查**：用户自己的 cordis.patch.yml 若还 insert 着这个包的条目，
//!   卸载会让下次启动缺模块；启动器不替用户改他的补丁文件，而是拒绝并指出要删哪几行。
//! - **原生模块**：Node 不会卸载已加载的 `.node`，卸载完不重启进程就重装，在 Windows
//!   上会因为改名撞句柄而失败 —— 这种卸载不能声称「热生效」。
//! - **静默未升级**：pnpm 的 minimumReleaseAge 会**安静地保留旧版本并退出 0**，
//!   所以退出码干净不等于升级成功，必须比对升级前后的版本/提交。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 一个 profile 在某个时刻的「已装插件」快照
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProfileSnapshot {
    /// package.json 的 dependencies
    pub deps: Vec<(String, String)>,
    /// dsh.profile.bundles
    pub bundles: Vec<String>,
    /// 变更前 node_modules 里各依赖的**实际已装版本**（"静默未升级"判定要用它比对）
    pub installed: Vec<(String, Option<String>)>,
}

impl ProfileSnapshot {
    pub fn dep_names(&self) -> Vec<String> {
        self.deps.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn bundle_names(&self) -> Vec<String> {
        self.bundles.clone()
    }

    /// 变更前 node_modules 里的版本
    pub fn installed_version(&self, name: &str) -> Option<String> {
        self.installed
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.clone())
    }
}

fn manifest_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join("package.json")
}

/// 读 profile 清单快照（package.json 的依赖 + bundles）
pub fn snapshot(profile_dir: &Path) -> ProfileSnapshot {
    let Ok(raw) = std::fs::read_to_string(manifest_path(profile_dir)) else {
        return ProfileSnapshot::default();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return ProfileSnapshot::default();
    };
    let mut deps: Vec<(String, String)> = v
        .get("dependencies")
        .and_then(|d| d.as_object())
        .map(|m| {
            m.iter()
                .map(|(k, val)| (k.clone(), val.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    deps.sort();
    let bundles: Vec<String> = v
        .get("dsh")
        .and_then(|d| d.get("profile"))
        .and_then(|p| p.get("bundles"))
        .and_then(|b| b.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let installed = deps
        .iter()
        .map(|(n, _)| (n.clone(), installed_version_of_dir(profile_dir, n)))
        .collect();
    ProfileSnapshot {
        deps,
        bundles,
        installed,
    }
}

/// 备份到 `<file>.launcher-bak`（只在首次生成，避免反复覆盖掉更早的好状态）
fn backup_once(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bak = path.with_file_name(format!("{file}.launcher-bak"));
    if bak.is_file() {
        return Ok(());
    }
    std::fs::copy(path, &bak)
        .map(|_| ())
        .map_err(|e| format!("备份 {} 失败: {e}", path.display()))
}

/// 从清单里删掉某个包的依赖行与 bundle 行（磁盘事实为准的修复手段）。
/// 返回是否有改动。写前备份；用 serde_json::to_string_pretty 回写会丢注释，
/// 但 package.json 本来就没有注释，安全。
pub fn drop_from_manifest(profile_dir: &Path, name: &str) -> Result<bool, String> {
    let file = manifest_path(profile_dir);
    let raw = std::fs::read_to_string(&file).map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let mut v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析 package.json 失败: {e}"))?;
    let mut touched = false;
    if let Some(deps) = v.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
        if deps.remove(name).is_some() {
            touched = true;
        }
    }
    if let Some(bundles) = v
        .get_mut("dsh")
        .and_then(|d| d.get_mut("profile"))
        .and_then(|p| p.get_mut("bundles"))
        .and_then(|b| b.as_array_mut())
    {
        let before = bundles.len();
        bundles.retain(|x| x.as_str() != Some(name));
        if bundles.len() != before {
            touched = true;
        }
    }
    if !touched {
        return Ok(false);
    }
    backup_once(&file)?;
    let mut text = serde_json::to_string_pretty(&v).map_err(|e| format!("序列化失败: {e}"))?;
    text.push('\n');
    std::fs::write(&file, text).map_err(|e| format!("写入 package.json 失败: {e}"))?;
    Ok(true)
}

/// 包是否在 node_modules 里真实存在（判断「包没了 vs 包还在」）
pub fn installed_on_disk(profile_dir: &Path, name: &str) -> bool {
    profile_dir
        .join("node_modules")
        .join(name)
        .join("package.json")
        .is_file()
}

fn installed_version_of_dir(profile_dir: &Path, name: &str) -> Option<String> {
    installed_version(profile_dir, name)
}

/// 已安装版本（node_modules 里的 package.json），用于「静默未升级」判定
pub fn installed_version(profile_dir: &Path, name: &str) -> Option<String> {
    let raw = std::fs::read_to_string(
        profile_dir
            .join("node_modules")
            .join(name)
            .join("package.json"),
    )
    .ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("version").and_then(|x| x.as_str()).map(String::from)
}

/// pnpm 的 minimumReleaseAge 会安静保留旧版本并退出 0 —— 干净退出不等于升级成功
pub fn is_stale_update(before: Option<&str>, after: Option<&str>) -> bool {
    matches!((before, after), (Some(b), Some(a)) if a == b)
}

/// 包是否声明了 dsh 清单（`dsh.bundle` / `dsh.client` 任一）
pub fn has_dsh_manifest(pkg_dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(pkg_dir.join("package.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    v.get("dsh").map(|d| d.is_object() || d.is_string()).unwrap_or(false)
}

/// package.json 声明的入口文件是否存在（main / exports / module / bin 里的第一项）
fn entry_artifact_exists(pkg_dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(pkg_dir.join("package.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let mut candidates: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        if !s.is_empty() && !candidates.iter().any(|x| x == s) {
            candidates.push(s.to_string());
        }
    };
    if let Some(m) = v.get("main").and_then(|x| x.as_str()) {
        push(m);
    }
    if let Some(m) = v.get("module").and_then(|x| x.as_str()) {
        push(m);
    }
    match v.get("exports") {
        Some(serde_json::Value::String(s)) => push(s),
        Some(serde_json::Value::Object(o)) => {
            let root = o.get(".").unwrap_or(&serde_json::Value::Null);
            match root {
                serde_json::Value::String(s) => push(s),
                serde_json::Value::Object(cond) => {
                    for key in ["import", "require", "default", "node"] {
                        if let Some(s) = cond.get(key).and_then(|x| x.as_str()) {
                            push(s);
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    candidates.iter().any(|rel| {
        let p = pkg_dir.join(rel.trim_start_matches("./"));
        p.is_file()
    })
}

/// cordis.patch.yml 里 `insert:` 声明的 id（只有 insert 会**创建**条目，config 行只是配置）
pub fn inserted_ids(pkg_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pkg_dir.join("cordis.patch.yml")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_inserted_ids(&text, &mut out);
    out
}

fn collect_inserted_ids(text: &str, out: &mut Vec<String>) {
    let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(text) else {
        return;
    };
    for item in &seq {
        if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
            for it in inner {
                if let Some(id) = it.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() && !out.iter().any(|x| x == id) {
                        out.push(id.to_string());
                    }
                }
                // insert 里也可以再套 insert
                if let Some(serde_yaml::Value::Sequence(deep)) = it.get("insert") {
                    for d in deep {
                        if let Some(id) = d.get("id").and_then(|v| v.as_str()) {
                            if !id.is_empty() && !out.iter().any(|x| x == id) {
                                out.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 包在 patch 里声明的**全部** id（config 行 + insert 行）——
/// 用户补丁可能引用其中任意一个，卸载前要全查一遍
pub fn declared_ids(pkg_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pkg_dir.join("cordis.patch.yml")) else {
        return Vec::new();
    };
    let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    fn walk(item: &serde_yaml::Value, out: &mut Vec<String>) {
        if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
            if !id.is_empty() && !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        }
        if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
            for it in inner {
                walk(it, out);
            }
        }
    }
    for item in &seq {
        walk(item, &mut out);
    }
    out
}

/// 一次安装/卸载之后应有的**后续动作**（由调用方用 `dsh plugin remove` 执行）
#[derive(Clone, Debug, PartialEq)]
pub struct FollowUp {
    pub name: String,
    pub reason: String,
}

/// 装/卸之后的核对结果
#[derive(Clone, Debug, Default)]
pub struct PostChecks {
    /// 供终端逐行显示
    pub logs: Vec<String>,
    /// 需要调用方接着执行的卸载（不合格的假成功、id 冲突、清单残留）
    pub follow_ups: Vec<FollowUp>,
    pub report: MutationReport,
}

/// 安装/卸载成功后的核对（对应 dshmarket 的 validateAddedPlugins + removeAndReconcile）
///
/// @param before 变更前的清单快照
/// @param argv   本次执行的 dsh plugin 参数（`add|remove …`）
/// @param kind   任务类型（`upgrade` 时才会做「静默未升级」判断）
pub fn post_mutation_checks(
    profile_dir: &Path,
    before: &ProfileSnapshot,
    argv: &[String],
    kind: &str,
) -> PostChecks {
    let after = snapshot(profile_dir);
    let mut out = PostChecks::default();
    let sub = argv.first().map(String::as_str).unwrap_or("");
    let names_in_argv: Vec<String> = argv
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| {
            // 规格可能是 name@ver / github:o/r#ref&path:x / link:/path —— 只留包名部分
            let base = a.split('#').next().unwrap_or(a);
            let base = base.strip_prefix("github:").unwrap_or(base);
            base.split('@').next().unwrap_or(base).to_string()
        })
        .collect();

    if sub == "add" {
        let added: Vec<String> = after
            .dep_names()
            .into_iter()
            .filter(|n| !before.dep_names().contains(n))
            .collect();
        if added.is_empty() {
            // dshmarket #258：命令报成功但 profile 一点没变 = 插件命令通道异常，而不是插件的问题
            out.logs.push(
                "⚠ dsh plugin add 报告成功，但 profile 的依赖没有任何变化（插件命令通道异常？）"
                    .to_string(),
            );
        }
        let others: Vec<String> = after
            .bundle_names()
            .into_iter()
            .filter(|b| !added.contains(b))
            .collect();
        for name in &added {
            if let Some(reason) = is_broken_install(profile_dir, name) {
                out.logs.push(format!(
                    "✘ {name} 装上了但不合格（{reason}）——它会让下次启动加载失败，正在卸掉"
                ));
                out.follow_ups.push(FollowUp {
                    name: name.clone(),
                    reason: "broken-install".into(),
                });
                out.report.removed_broken.push((name.clone(), reason));
                continue;
            }
            let hits = conflicting_inserted_ids(profile_dir, name, &others);
            if !hits.is_empty() {
                let desc = hits
                    .iter()
                    .map(|(id, owner)| format!("{id} ↔ {owner}"))
                    .collect::<Vec<_>>()
                    .join("、");
                out.logs.push(format!(
                    "✘ {name} 的 loader 条目 id 与已装插件冲突（{desc}）——cordis 会因此拒绝启动整棵插件树，正在卸掉 {name}"
                ));
                out.follow_ups.push(FollowUp {
                    name: name.clone(),
                    reason: "entry-id-conflict".into(),
                });
                out.report.conflicts.extend(hits);
                out.report.removed_broken.push((name.clone(), "entry-id-conflict".into()));
                continue;
            }
            out.report.added.push(name.clone());
            out.logs.push(format!("✔ {name} 校验通过（有 dsh 清单 + 可加载入口）"));
        }
        // 静默未升级：pnpm 的 minimumReleaseAge 会保留旧版本并退出 0
        if kind == "upgrade" {
            for name in names_in_argv.iter() {
                let after_v = installed_version(profile_dir, name);
                let before_v = before.installed_version(name);
                if is_stale_update(before_v.as_deref(), after_v.as_deref()) {
                    out.report.stale_updates.push(name.clone());
                    out.logs.push(format!(
                        "⚠ {name} 升级后版本没变（{}）——pnpm 可能因 minimumReleaseAge 保留了旧版本，未真正升级",
                        after_v.unwrap_or_default()
                    ));
                }
            }
        }
    } else if sub == "remove" {
        // 以「我们请求卸载的名字」为准，再以**磁盘事实**收尾（对齐 dshmarket 的 removeAndReconcile）：
        // remove 可能事都做完了却退出非 0，也可能退出 0 却没走到清单和解。
        let mut seen: Vec<String> = Vec::new();
        for name in &names_in_argv {
            if seen.contains(name) {
                continue;
            }
            seen.push(name.clone());
            if installed_on_disk(profile_dir, name) {
                out.report.still_installed.push(name.clone());
                out.logs.push(format!(
                    "⚠ {name} 仍在 node_modules 里（卸载没有真正生效）：清单行保留，请重试卸载"
                ));
                continue;
            }
            // 包没了但清单还留着行 → 按磁盘事实删行，否则下次启动会因解析不到包而整体失败
            if after.dep_names().contains(name) || after.bundle_names().contains(name) {
                out.follow_ups.push(FollowUp {
                    name: name.clone(),
                    reason: "manifest-residue".into(),
                });
                out.report.manifest_repaired.push(name.clone());
                out.logs.push(format!(
                    "✘ {name} 已从磁盘移除，但 package.json 里还留着它的依赖/bundle 行（remove 没走到清单和解）——正在删除这些残留行"
                ));
            }
        }
    }
    out
}

/// patch 里引用的**其它包名**（carrier 场景：自己没入口，靠 patch 挂载别的包）
fn patch_target_names(pkg_dir: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pkg_dir.join("cordis.patch.yml")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
        return out;
    };
    let walk = |item: &serde_yaml::Value, out: &mut Vec<String>| {
        if let Some(n) = item.get("name").and_then(|v| v.as_str()) {
            if !n.is_empty() && !out.iter().any(|x| x == n) {
                out.push(n.to_string());
            }
        }
        if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
            for it in inner {
                if let Some(n) = it.get("name").and_then(|v| v.as_str()) {
                    if !n.is_empty() && !out.iter().any(|x| x == n) {
                        out.push(n.to_string());
                    }
                }
            }
        }
    };
    for item in &seq {
        walk(item, &mut out);
    }
    out
}

/// loader 是否有东西可加载：自己的入口文件，或（carrier 包）它 patch 里挂载的包有入口。
/// 解析位置对齐 pnpm 的 hoisted 布局：profile/node_modules、包内 node_modules、
/// 以及 workspace 根（profiles/node_modules）。
pub fn has_loadable_entry(profile_dir: &Path, name: &str) -> bool {
    let dir = profile_dir.join("node_modules").join(name);
    if entry_artifact_exists(&dir) {
        return true;
    }
    let workspace_root = profile_dir.parent().unwrap_or(profile_dir);
    for target in patch_target_names(&dir) {
        if target == name {
            continue;
        }
        for base in [
            profile_dir.join("node_modules").join(&target),
            dir.join("node_modules").join(&target),
            workspace_root.join("node_modules").join(&target),
        ] {
            if entry_artifact_exists(&base) {
                return true;
            }
        }
    }
    false
}

/// 新装的包与被装插件之间的 **insert id 冲突**（会让下次启动直接失败）
pub fn conflicting_inserted_ids(
    profile_dir: &Path,
    candidate: &str,
    other_bundles: &[String],
) -> Vec<(String, String)> {
    let mine = inserted_ids(&profile_dir.join("node_modules").join(candidate));
    if mine.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(String, String)> = Vec::new();
    for bundle in other_bundles {
        if bundle == candidate {
            continue;
        }
        let theirs = inserted_ids(&profile_dir.join("node_modules").join(bundle));
        for id in &mine {
            if theirs.contains(id) && !hits.iter().any(|(i, _)| i == id) {
                hits.push((id.clone(), bundle.clone()));
            }
        }
    }
    hits
}

/// 装完成败判定：缺 dsh 清单、或 loader 没东西可加载 → 属于「会把下次启动弄挂」的假成功
pub fn is_broken_install(profile_dir: &Path, name: &str) -> Option<String> {
    let dir = profile_dir.join("node_modules").join(name);
    if !dir.is_dir() {
        return Some("包没有出现在 node_modules 里".into());
    }
    if !has_dsh_manifest(&dir) {
        return Some("package.json 里没有 dsh 清单（不是 dsh 插件包）".into());
    }
    if !has_loadable_entry(profile_dir, name) {
        return Some(
            "没有可加载的入口文件（源码检出、或构建脚本被 pnpm allowBuilds 拦住了）".into(),
        );
    }
    None
}

/// 包内（或其自带依赖里）是否有原生模块 `.node`
pub fn holds_native_addon(profile_dir: &Path, name: &str) -> bool {
    fn scan(dir: &Path, depth: usize) -> bool {
        if depth > 4 {
            return false;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if scan(&p, depth + 1) {
                    return true;
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("node") {
                return true;
            }
        }
        false
    }
    let dir = profile_dir.join("node_modules").join(name);
    if !dir.is_dir() {
        return false;
    }
    // 只扫包自身与它自带的 node_modules，别把整棵依赖树走一遍
    scan(&dir, 0)
}

/// 用户自己的 cordis.patch.yml 是否仍在引用这个包（包名或其插件 id）。
/// 返回引用处，空 = 没有引用；None = 补丁无法解析（不确定）。
///
/// 启动器**不会**替用户改他的补丁文件：卸载前先拒绝，并指出要删哪几行。
pub fn user_patch_references(profile_dir: &Path, name: &str, plugin_ids: &[String]) -> Option<Vec<String>> {
    let path = profile_dir.join("cordis.patch.yml");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    if raw.trim().is_empty() {
        return Some(Vec::new());
    }
    let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(&raw) else {
        return None;
    };
    let mut hits: Vec<String> = Vec::new();
    let note = |label: String, hits: &mut Vec<String>| {
        if !hits.iter().any(|x| x == &label) {
            hits.push(label);
        }
    };
    for item in &seq {
        let name_hit = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|n| n == name)
            .unwrap_or(false);
        let id_hit = item
            .get("id")
            .and_then(|v| v.as_str())
            .map(|i| plugin_ids.iter().any(|p| p == i))
            .unwrap_or(false);
        if name_hit || id_hit {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or(name);
            note(id.to_string(), &mut hits);
        }
        if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
            for it in inner {
                let name_hit = it
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| n == name)
                    .unwrap_or(false);
                let id_hit = it
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|i| plugin_ids.iter().any(|p| p == i))
                    .unwrap_or(false);
                if name_hit || id_hit {
                    let id = it
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| it.get("name").and_then(|v| v.as_str()))
                        .unwrap_or(name);
                    note(id.to_string(), &mut hits);
                }
            }
        }
    }
    Some(hits)
}

/// 一次安装/卸载后要报告的结果
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationReport {
    /// 本次新增并保留下来的包
    pub added: Vec<String>,
    /// 因不合格被当场卸掉的包（附原因）
    pub removed_broken: Vec<(String, String)>,
    /// insert id 冲突（id, 冲突方包名）
    pub conflicts: Vec<(String, String)>,
    /// 静默未升级的包（pnpm 因 minimumReleaseAge 保留了旧版本）
    pub stale_updates: Vec<String>,
    /// 以磁盘事实修的清单残留
    pub manifest_repaired: Vec<String>,
    /// 卸载后仍在磁盘上、需要重试的包
    pub still_installed: Vec<String>,
}

// ── pnpm allowBuilds（放行构建脚本） ─────────────────────────
//
// 处理方案移植自 dshmarket 的 setAllowBuilds（`src/profile.ts`），那几个边界都是
// 真实 issue 换来的，不能省：
// - **必须按 `\r?\n` 匹配**：CRLF 的 pnpm-workspace.yaml（Windows 编辑器、git autocrlf）
//   会让旧写法匹配不到已有块而**再追加一个** `allowBuilds:`；两个同名顶层键是非法 YAML，
//   pnpm 之后会拒绝该 profile 的每一次安装，不只是这一次。
// - **合并全部同名块**（而不是只处理第一个）：已经踩坑留下两个块的文件能被修复，
//   且不会丢掉其中任何一个里的授权。
// - **键要按需加引号**：`@scope/pkg` 以 YAML 保留指示符 `@` 开头，不引号会让整个文件
//   对之后所有 pnpm 运行都失效。
// - **只接受三种键形态**（裸名 / `name@git+https://github.com/o/r.git` /
//   `name@https://codeload.github.com/o/r/tar.gz/<40hex>`）：这个列表是"不往 pnpm 会解析的
//   文件里写任意文本"的保证。
// - **丢掉占位值**：pnpm 失败安装的 bug 会写入字面量 "set this to true or false"，
//   留着会让之后所有授权都失效。
// - **沿用文件自己的换行符**，别把 CRLF 文件改成混合行尾。

/// YAML 块映射的键在必要时加引号
fn quote_yaml_key(key: &str) -> String {
    let first = key.chars().next().unwrap_or(' ');
    let reserved = "-?:,[]{}#&*!|>'\"%@`".contains(first);
    let colon_space = key.contains(": ") || key.ends_with(':');
    if reserved || colon_space {
        format!("'{}'", key.replace('\'', "''"))
    } else {
        key.to_string()
    }
}

/// 允许写进 allowBuilds 的键形态（裸包名 / 稳定 git 形态 / codeload 形态）
fn allow_build_key_ok(key: &str) -> bool {
    let bare = regex::Regex::new(r"^[A-Za-z0-9@/_.-]+$").expect("静态正则");
    let git = regex::Regex::new(
        r"^[A-Za-z0-9@/_.-]+@git\+https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.git$",
    )
    .expect("静态正则");
    let codeload = regex::Regex::new(
        r"^[A-Za-z0-9@/_.-]+@https://codeload\.github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/tar\.gz/[0-9a-f]{40}$",
    )
    .expect("静态正则");
    bare.is_match(key) || git.is_match(key) || codeload.is_match(key)
}

/// 解析 yaml 里所有 `allowBuilds:` 块（键 → 值），丢弃占位值与非法键
fn parse_allow_builds_blocks(yaml: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    let mut in_block = false;
    for line in yaml.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_block = line.trim_start().starts_with("allowBuilds:");
            continue;
        }
        if !in_block {
            continue;
        }
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        // 值只认 true/false；pnpm bug 写出的占位值会被这里丢掉
        let Some((k, v)) = t.rsplit_once(':') else { continue };
        let v = v.trim();
        if v != "true" && v != "false" {
            continue;
        }
        let mut key = k.trim().to_string();
        if key.len() >= 2
            && ((key.starts_with('\'') && key.ends_with('\''))
                || (key.starts_with('"') && key.ends_with('"')))
        {
            key = key[1..key.len() - 1].to_string();
        }
        if key.is_empty() || !allow_build_key_ok(&key) {
            continue;
        }
        map.insert(key, v.to_string());
    }
    map
}

/// 把给定包写进 profile 的 `pnpm-workspace.yaml` 的 `allowBuilds`（合并已有条目、
/// 保留文件其余内容与行尾）。返回合并后**全部**被放行的键。
///
/// 这是用户显式点「允许构建脚本」之后才调用的：构建脚本会执行第三方代码，
/// 属于用户的决定，启动器不替用户默认放行。
pub fn set_allow_builds(profile_dir: &Path, packages: &[String]) -> Result<Vec<String>, String> {
    let file = profile_dir.join("pnpm-workspace.yaml");
    let yaml = std::fs::read_to_string(&file).unwrap_or_default();
    let mut map = parse_allow_builds_blocks(&yaml);
    let mut accepted: Vec<String> = Vec::new();
    for p in packages {
        let key = p.trim();
        if key.is_empty() || !allow_build_key_ok(key) {
            continue;
        }
        map.insert(key.to_string(), "true".to_string());
        accepted.push(key.to_string());
    }
    if accepted.is_empty() {
        return Ok(map.keys().cloned().collect());
    }
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let block: Vec<String> = map
        .iter()
        .map(|(k, v)| format!("  {}: {v}", quote_yaml_key(k)))
        .collect();
    let block_text = format!("allowBuilds:{eol}{}{eol}", block.join(eol));

    // 丢掉所有旧的 allowBuilds 块（条目已并入 map），再把合并结果插到原位置
    let mut out: Vec<String> = Vec::new();
    let mut inserted = false;
    let mut in_block = false;
    let mut first_block_line: Option<usize> = None;
    for line in yaml.split('\n') {
        let bare = line.strip_suffix('\r').unwrap_or(line);
        let top_level = !bare.starts_with(' ') && !bare.starts_with('\t');
        if top_level {
            if bare.trim_start().starts_with("allowBuilds:") {
                if !inserted {
                    if first_block_line.is_none() {
                        first_block_line = Some(out.len());
                    }
                    for l in block_text.trim_end_matches(eol).split(eol) {
                        out.push(l.to_string());
                    }
                    inserted = true;
                }
                in_block = true;
                continue;
            }
            in_block = false;
        }
        if in_block {
            continue; // 旧块的内容整体丢弃（已并入 map）
        }
        out.push(line.to_string());
    }
    let mut text = out.join("\n");
    if !inserted {
        // 没有 allowBuilds 块：追加到文件末尾（保住原有换行风格）
        let trimmed = text.trim_end_matches(['\n', '\r']).to_string();
        text = if trimmed.is_empty() {
            block_text.clone()
        } else {
            format!("{trimmed}{eol}{eol}{}", block_text.replace(eol, eol))
        };
    } else if !text.ends_with('\n') {
        text.push('\n');
    }

    backup_once(&file)?;
    std::fs::write(&file, text).map_err(|e| format!("写入 {} 失败: {e}", file.display()))?;
    Ok(map.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dsh-verify-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_pkg(dir: &Path, name: &str, body: &str) {
        let d = dir.join("node_modules").join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("package.json"), body).unwrap();
    }

    #[test]
    fn allow_builds_merges_and_quotes_and_keeps_eol() {
        let dir = tmp("allow");
        std::fs::write(
            dir.join("pnpm-workspace.yaml"),
            "packages:\r\n  - .\r\n\r\nallowBuilds:\r\n  esbuild: true\r\n  cpu-features: false\r\n  broken: set this to true or false\r\n",
        )
        .unwrap();
        let all = set_allow_builds(&dir, &["@scope/native-thing".to_string(), "supreium-headless-gl".to_string()])
            .unwrap();
        // 合并了旧的（含 false 那条）+ 新的；占位值那条被丢掉
        assert!(all.contains(&"esbuild".to_string()));
        assert!(all.contains(&"cpu-features".to_string()));
        assert!(all.contains(&"@scope/native-thing".to_string()));
        assert!(all.contains(&"supreium-headless-gl".to_string()));
        assert!(!all.contains(&"broken".to_string()), "占位值条目应被丢掉");
        let text = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
        // CRLF 保留，且只有一个 allowBuilds 块（多了就是非法 YAML）
        assert!(text.contains("\r\n"), "应保留 CRLF");
        assert_eq!(text.matches("allowBuilds:").count(), 1, "{text}");
        // @scope 键必须加引号，否则整个 yaml 对之后所有 pnpm 运行都失效
        assert!(text.contains("'@scope/native-thing': true"), "{text}");
        // 合并后仍然是合法 YAML 且能读回来
        let v: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        let ab = v.get("allowBuilds").unwrap().as_mapping().unwrap();
        assert!(ab.len() >= 4);
        // 只想放行非法键时不写文件
        let before = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
        set_allow_builds(&dir, &["rm -rf /".to_string()]).unwrap();
        assert_eq!(before, std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 串联：用本次实测的 pnpm 输出 → 解析被拦包名 → 写 allowBuilds → 文件仍合法可用
    #[test]
    fn approve_flow_from_real_pnpm_output() {
        let output = "dependencies:\n\
                      + dsh-plugin-wallpaper-engine https://gh.927223.xyz/https://github.com/e/dsh-wallpaper-engine/releases/download/v0.7.1/x.tgz\n\
                      Packages: +137\n\
                      [ERR_PNPM_IGNORED_BUILDS] Ignored build scripts: supreium-headless-gl@8.3.0\n\
                      Run \"pnpm approve-builds\" to pick which dependencies should be allowed to run scripts.\n";
        // 分类 → 解析（分类器只报"被拦"，包名由解析函数给出）
        let f = crate::pnpm::classify(output, Some(1)).unwrap();
        assert_eq!(f.code(), "ignored-builds");
        let names = crate::pnpm::parse_ignored_builds(output);
        assert_eq!(names, vec!["supreium-headless-gl".to_string()]);

        let dir = tmp("approve-e2e");
        std::fs::write(
            dir.join("pnpm-workspace.yaml"),
            "packages:\n  - .\n\nnodeLinker: hoisted\n",
        )
        .unwrap();
        let all = set_allow_builds(&dir, &names).unwrap();
        assert!(all.contains(&"supreium-headless-gl".to_string()));
        let text = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
        // 原有内容保留 + 新块合法
        assert!(text.contains("packages:") && text.contains("nodeLinker: hoisted"));
        let v: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();
        assert_eq!(
            v.get("allowBuilds")
                .and_then(|m| m.get("supreium-headless-gl"))
                .and_then(|b| b.as_bool()),
            Some(true)
        );
        // 备份在（可回退）
        assert!(dir.join("pnpm-workspace.yaml.launcher-bak").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allow_builds_repairs_doubled_blocks_and_appends_when_missing() {
        let dir = tmp("allow2");
        // 踩过坑的文件：两个 allowBuilds 块（旧写法用 \n 匹配漏了 CRLF 造成的）
        std::fs::write(
            dir.join("pnpm-workspace.yaml"),
            "allowBuilds:\n  a: true\nallowBuilds:\n  b: true\n",
        )
        .unwrap();
        let all = set_allow_builds(&dir, &["c".to_string()]).unwrap();
        assert!(all.contains(&"a".to_string()) && all.contains(&"b".to_string()) && all.contains(&"c".to_string()));
        let text = std::fs::read_to_string(dir.join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(text.matches("allowBuilds:").count(), 1, "{text}");
        serde_yaml::from_str::<serde_yaml::Value>(&text).unwrap();

        // 没有该块时追加（并保住原有内容）
        let dir2 = tmp("allow3");
        std::fs::write(dir2.join("pnpm-workspace.yaml"), "packages:\n  - .\n").unwrap();
        set_allow_builds(&dir2, &["esbuild".to_string()]).unwrap();
        let t2 = std::fs::read_to_string(dir2.join("pnpm-workspace.yaml")).unwrap();
        assert!(t2.contains("packages:") && t2.contains("  esbuild: true"), "{t2}");
        serde_yaml::from_str::<serde_yaml::Value>(&t2).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn snapshot_reads_deps_and_bundles() {
        let dir = tmp("snap");
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"a":"1.0.0","b":"link:/x"},"dsh":{"profile":{"bundles":["a"]}}}"#,
        )
        .unwrap();
        let s = snapshot(&dir);
        assert_eq!(s.dep_names(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.bundle_names(), vec!["a".to_string()]);
        assert_eq!(
            s.deps.iter().find(|(n, _)| n == "b").map(|(_, v)| v.as_str()),
            Some("link:/x")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drop_from_manifest_removes_both_rows() {
        let dir = tmp("drop");
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"a":"1.0.0","b":"2.0.0"},"dsh":{"profile":{"bundles":["a","b"]}}}"#,
        )
        .unwrap();
        assert!(drop_from_manifest(&dir, "a").unwrap());
        let s = snapshot(&dir);
        assert_eq!(s.dep_names(), vec!["b".to_string()]);
        assert_eq!(s.bundle_names(), vec!["b".to_string()]);
        // 备份存在且保留原始内容（可回退）
        assert!(dir.join("package.json.launcher-bak").is_file());
        // 幂等：没这个包时不再改
        assert!(!drop_from_manifest(&dir, "zzz").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_broken_installs_and_loadable_entries() {
        let dir = tmp("broken");
        write_pkg(&dir, "good", r#"{"name":"good","main":"./lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::create_dir_all(dir.join("node_modules/good/lib")).unwrap();
        std::fs::write(dir.join("node_modules/good/lib/index.js"), "").unwrap();
        assert!(is_broken_install(&dir, "good").is_none());

        // 缺 dsh 清单（普通库被当成插件装了）
        write_pkg(&dir, "plain", r#"{"name":"plain","main":"./lib/index.js"}"#);
        std::fs::create_dir_all(dir.join("node_modules/plain/lib")).unwrap();
        std::fs::write(dir.join("node_modules/plain/lib/index.js"), "").unwrap();
        assert!(is_broken_install(&dir, "plain").unwrap().contains("dsh 清单"));

        // 有清单但入口文件不存在（源码检出 / 构建被拦）
        write_pkg(
            &dir,
            "src-only",
            r#"{"name":"src-only","main":"./lib/host.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        );
        let broken = is_broken_install(&dir, "src-only").unwrap();
        assert!(broken.contains("入口文件"), "{broken}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn carrier_bundle_is_loadable_through_what_it_mounts() {
        let dir = tmp("carrier");
        // carrier 自己没有入口，但 patch 里挂载了另一个有入口的包
        write_pkg(
            &dir,
            "carrier",
            r#"{"name":"carrier","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        );
        std::fs::write(
            dir.join("node_modules/carrier/cordis.patch.yml"),
            "- insert:\n    - id: skin\n      name: real-ui\n",
        )
        .unwrap();
        write_pkg(&dir, "real-ui", r#"{"name":"real-ui","main":"./lib/index.js"}"#);
        std::fs::create_dir_all(dir.join("node_modules/real-ui/lib")).unwrap();
        std::fs::write(dir.join("node_modules/real-ui/lib/index.js"), "").unwrap();
        assert!(has_loadable_entry(&dir, "carrier"));
        assert!(is_broken_install(&dir, "carrier").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn detects_inserted_id_conflicts_only() {
        let dir = tmp("conflict");
        write_pkg(&dir, "newbie", r#"{"name":"newbie","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::write(
            dir.join("node_modules/newbie/cordis.patch.yml"),
            "- insert:\n    - id: storage\n",
        )
        .unwrap();
        write_pkg(&dir, "installed", r#"{"name":"installed","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::write(
            dir.join("node_modules/installed/cordis.patch.yml"),
            "- insert:\n    - id: storage\n- id: harmless-config\n",
        )
        .unwrap();
        let hits = conflicting_inserted_ids(&dir, "newbie", &["installed".to_string()]);
        assert_eq!(hits, vec![("storage".to_string(), "installed".to_string())]);
        // 只是 config 行（不 insert）不算冲突
        write_pkg(&dir, "cfg", r#"{"name":"cfg","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::write(dir.join("node_modules/cfg/cordis.patch.yml"), "- id: storage\n").unwrap();
        assert!(conflicting_inserted_ids(&dir, "cfg", &["installed".to_string()]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_patch_reference_blocks_uninstall() {
        let dir = tmp("upatch");
        std::fs::write(
            dir.join("cordis.patch.yml"),
            "# my notes\n- id: keepme\n- insert:\n    - id: mypkg-entry\n      name: '@dshp/mypkg'\n",
        )
        .unwrap();
        let hits = user_patch_references(&dir, "@dshp/mypkg", &[]).unwrap();
        assert_eq!(hits, vec!["mypkg-entry".to_string()]);
        // 按插件 id 也能查到
        let hits2 = user_patch_references(&dir, "@other/pkg", &["mypkg-entry".to_string()]).unwrap();
        assert_eq!(hits2, vec!["mypkg-entry".to_string()]);
        // 无关包不受影响
        assert!(user_patch_references(&dir, "@x/y", &["nope".to_string()]).unwrap().is_empty());
        // 补丁坏了 → None（不确定，交由上层决定是否强制）
        std::fs::write(dir.join("cordis.patch.yml"), "foo: [unclosed\n").unwrap();
        assert!(user_patch_references(&dir, "@dshp/mypkg", &[]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_checks_flag_broken_install_and_id_conflict() {
        let dir = tmp("post-broken");
        // 变更前：已装一个声明 storage 的插件
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"installed":"1.0.0"},"dsh":{"profile":{"bundles":["installed"]}}}"#,
        )
        .unwrap();
        write_pkg(&dir, "installed", r#"{"name":"installed","main":"./lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::create_dir_all(dir.join("node_modules/installed/lib")).unwrap();
        std::fs::write(dir.join("node_modules/installed/lib/index.js"), "").unwrap();
        std::fs::write(dir.join("node_modules/installed/cordis.patch.yml"), "- insert:\n    - id: storage\n").unwrap();
        let before = snapshot(&dir);

        // 变更后：新增一个「没有入口」的源码检出 + 一个 id 冲突的包
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"installed":"1.0.0","src-only":"2.0.0","clash":"3.0.0"},"dsh":{"profile":{"bundles":["installed","src-only","clash"]}}}"#,
        )
        .unwrap();
        write_pkg(&dir, "src-only", r#"{"name":"src-only","main":"./lib/host.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::write(dir.join("node_modules/src-only/cordis.patch.yml"), "- insert:\n    - id: solo\n").unwrap();
        write_pkg(&dir, "clash", r#"{"name":"clash","main":"./lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::create_dir_all(dir.join("node_modules/clash/lib")).unwrap();
        std::fs::write(dir.join("node_modules/clash/lib/index.js"), "").unwrap();
        std::fs::write(dir.join("node_modules/clash/cordis.patch.yml"), "- insert:\n    - id: storage\n").unwrap();

        let checks = post_mutation_checks(&dir, &before, &["add".into(), "src-only@2".into(), "clash@3".into()], "install");
        let names: Vec<&String> = checks.follow_ups.iter().map(|f| &f.name).collect();
        assert!(names.contains(&&"src-only".to_string()), "{:?}", checks.logs);
        assert!(names.contains(&&"clash".to_string()), "{:?}", checks.logs);
        assert_eq!(checks.report.removed_broken.len(), 2);
        assert_eq!(checks.report.conflicts, vec![("storage".to_string(), "installed".to_string())]);
        assert!(checks.logs.iter().any(|l| l.contains("id 与已装插件冲突")), "{:?}", checks.logs);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn post_checks_repair_manifest_residue_and_flag_stale_update() {
        let dir = tmp("post-residue");
        std::fs::write(
            dir.join("package.json"),
            r#"{"dependencies":{"gone":"1.0.0","keep":"2.0.0"},"dsh":{"profile":{"bundles":["gone","keep"]}}}"#,
        )
        .unwrap();
        let before = snapshot(&dir);
        // 「卸载成功」但清单里仍留着 gone 的行，且磁盘上已经没有了（node_modules 里没有 gone）
        let checks = post_mutation_checks(&dir, &before, &["remove".into(), "gone".into()], "uninstall");
        assert_eq!(checks.report.manifest_repaired, vec!["gone".to_string()]);
        assert!(checks.follow_ups.iter().any(|f| f.reason == "manifest-residue"));

        // 升级但版本没变（minimumReleaseAge 静默保留旧版本）
        let dir2 = tmp("post-stale");
        std::fs::write(
            dir2.join("package.json"),
            r#"{"dependencies":{"p":"1.2.3"},"dsh":{"profile":{"bundles":["p"]}}}"#,
        )
        .unwrap();
        write_pkg(&dir2, "p", r#"{"name":"p","version":"1.2.3","main":"./lib/index.js","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::create_dir_all(dir2.join("node_modules/p/lib")).unwrap();
        std::fs::write(dir2.join("node_modules/p/lib/index.js"), "").unwrap();
        let before2 = snapshot(&dir2);
        let checks2 = post_mutation_checks(&dir2, &before2, &["add".into(), "p@latest".into()], "upgrade");
        assert_eq!(checks2.report.stale_updates, vec!["p".to_string()]);
        assert!(checks2.logs.iter().any(|l| l.contains("版本没变")), "{:?}", checks2.logs);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn empty_add_is_reported_as_channel_problem() {
        let dir = tmp("post-noop");
        std::fs::write(dir.join("package.json"), r#"{"dependencies":{}}"#).unwrap();
        let before = snapshot(&dir);
        let checks = post_mutation_checks(&dir, &before, &["add".into(), "x@1".into()], "install");
        assert!(checks.logs.iter().any(|l| l.contains("插件命令通道异常")), "{:?}", checks.logs);
        assert!(checks.report.added.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn native_addon_and_stale_update() {
        let dir = tmp("native");
        write_pkg(&dir, "nat", r#"{"name":"nat","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#);
        std::fs::create_dir_all(dir.join("node_modules/nat/prebuilds")).unwrap();
        assert!(!holds_native_addon(&dir, "nat"));
        std::fs::write(dir.join("node_modules/nat/prebuilds/x.node"), "").unwrap();
        assert!(holds_native_addon(&dir, "nat"));
        // minimumReleaseAge 静默保留旧版本：退出码干净但版本没变
        assert!(is_stale_update(Some("1.2.3"), Some("1.2.3")));
        assert!(!is_stale_update(Some("1.2.3"), Some("1.3.0")));
        assert!(!is_stale_update(None, Some("1.0.0")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
