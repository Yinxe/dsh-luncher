use tauri::Emitter;
use crate::settings::Settings;
use crate::util;
use serde::{Deserialize, Serialize};
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
    /// 该包通过 patch 层声明的真实插件 id（包名 ≠ 插件 id）
    pub plugin_ids: Vec<String>,
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
pub fn read_detail(settings: &Settings, profile: &str) -> Result<ProfileDetail, String> {
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
        let bundles_list: Vec<String> = pkg
            .get("dsh")
            .and_then(|d| d.get("profile"))
            .and_then(|p| p.get("bundles"))
            .and_then(|b| b.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let ids_map = resolve_bundle_ids(settings, profile, &dir, &bundles_list);
        for name in &bundles_list {
            let dep_ver = deps
                .and_then(|d| d.get(name))
                .and_then(|v| v.as_str())
                .map(String::from);
            let plugin_ids = ids_map.get(name).cloned().unwrap_or_default();
            // 包禁用 = 其声明的全部插件 id 都在用户 patch 层被禁用
            let enabled = plugin_ids.is_empty()
                || !plugin_ids.iter().all(|id| patch_disabled.contains(id));
            bundles.push(BundleInfo {
                name: name.to_string(),
                version: dep_ver.clone(),
                source: source_of(dep_ver.as_deref()).to_string(),
                enabled,
                plugin_ids,
            });
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

/// 用户 cordis.patch.yml 中被禁用的条目 id 集合（顶层 id 条目 + insert 内层子项）
fn patch_disabled_ids(profile: &str) -> std::collections::BTreeSet<String> {
    match std::fs::read_to_string(user_patch_path(profile).unwrap_or_default()) {
        Ok(raw) => disabled_ids_in(&raw),
        Err(_) => Default::default(),
    }
}

fn disabled_ids_in(raw: &str) -> std::collections::BTreeSet<String> {
    let mut set = std::collections::BTreeSet::new();
    if let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(raw) {
        for item in seq {
            collect_disabled_ids(&item, &mut set);
        }
    }
    set
}

fn collect_disabled_ids(item: &serde_yaml::Value, set: &mut std::collections::BTreeSet<String>) {
    if item
        .get("disabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
            set.insert(id.to_string());
        }
    }
    if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
        for it in inner {
            collect_disabled_ids(it, set);
        }
    }
}

/// 覆写前先备份原文件。固定单一备份文件 <名>.launcher-bak，每次覆写覆盖同一份；
/// 同时清理旧版按时间戳堆积的 <名>.launcher-bak-<ts> 备份。
fn backup(path: &Path) -> Result<(), String> {
    let content = match std::fs::read(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // 目标不存在则无从备份
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bak = path.with_file_name(format!("{stem}.launcher-bak"));
    std::fs::write(&bak, content).map_err(|e| format!("写备份失败: {e}"))?;
    if let Some(dir) = path.parent() {
        let legacy_prefix = format!("{stem}.launcher-bak-");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&legacy_prefix) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
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

/// 从 `dsh --profile <p> --dump-config` 输出解析各层声明的插件 id。
/// 分节头 `# == <层名>`（可带 ", patched by X" 后缀），其下顶层 `- id: X` 即该层声明的插件。
fn parse_dump_layers(text: &str) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let mut layer: Option<String> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# == ") {
            let name = rest.split(", patched by").next().unwrap_or(rest).trim();
            layer = Some(name.to_string());
            continue;
        }
        if let (Some(l), Some(id)) = (&layer, line.strip_prefix("- id: ")) {
            let id = id.trim().trim_matches(|c| c == '\'' || c == '"').trim();
            if id.is_empty() {
                continue;
            }
            let e = out.entry(l.clone()).or_default();
            if !e.iter().any(|x| x == id) {
                e.push(id.to_string());
            }
        }
    }
    out
}

/// 兜底：直接读各 bundle 自带 cordis.patch.yml 中的 id（含 insert 子项）。
/// dsh-base 等核心包的 patch 由代码生成、无此文件，解析不到属预期。
fn bundle_ids_from_files(
    dir: &Path,
    bundles: &[String],
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for b in bundles {
        let path = dir.join("node_modules").join(b).join("cordis.patch.yml");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(serde_yaml::Value::Sequence(seq)) =
            serde_yaml::from_str::<serde_yaml::Value>(&text)
        else {
            continue;
        };
        let e = out.entry(b.clone()).or_default();
        for item in seq {
            collect_patch_item_ids(&item, e);
        }
    }
    out
}

fn collect_patch_item_ids(item: &serde_yaml::Value, out: &mut Vec<String>) {
    if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
        if !id.is_empty() && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    }
    if let Some(serde_yaml::Value::Sequence(inner)) = item.get("insert") {
        for it in inner {
            collect_patch_item_ids(it, out);
        }
    }
}

/// bundle 包名 → 其声明的真实插件 id。包名 ≠ 插件 id（如 @dshp/mcwiki-search 的 id 是
/// dshp-mcwiki-search）；优先官方命令 `dsh --profile <p> --dump-config`（权威，含
/// dsh-base 等以代码生成 patch 的核心包），失败再读各 bundle 自带的 cordis.patch.yml。
fn resolve_bundle_ids(
    settings: &Settings,
    profile: &str,
    dir: &Path,
    bundles: &[String],
) -> std::collections::BTreeMap<String, Vec<String>> {
    if let Ok((node, bin_js)) = resolve_dsh_bin(settings) {
        let out = std::process::Command::new(&node)
            .arg(&bin_js)
            .arg("--profile")
            .arg(profile)
            .arg("--dump-config")
            .current_dir(dir)
            .env(
                "DSH_HOME",
                profiles::dsh_native_home().to_string_lossy().into_owned(),
            )
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let mut map = parse_dump_layers(&String::from_utf8_lossy(&o.stdout));
                map.retain(|k, _| bundles.contains(k));
                return map;
            }
        }
    }
    bundle_ids_from_files(dir, bundles)
}

/// 修改 bundle 启用状态：先解析出该包声明的真实插件 id，再通过用户 cordis.patch 层
/// 逐 id 写 `disabled: true`（禁用 ≠ 移除，卸载才是真移除）。
pub fn set_bundle_enabled(
    settings: &Settings,
    profile: &str,
    name: &str,
    enabled: bool,
) -> Result<(), String> {
    let dir = profile_dir(profile)?;
    let pkg_raw = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let pkg: serde_json::Value =
        serde_json::from_str(&pkg_raw).map_err(|e| format!("package.json 解析失败: {e}"))?;
    let bundles: Vec<String> = pkg
        .get("dsh")
        .and_then(|d| d.get("profile"))
        .and_then(|p| p.get("bundles"))
        .and_then(|b| b.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let ids = resolve_bundle_ids(settings, profile, &dir, &bundles)
        .get(name)
        .cloned()
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(format!(
            "无法解析插件包 {name} 声明的插件 ID，无法启停（可卸载该包或手动编辑 cordis.patch.yml）"
        ));
    }
    set_ids_disabled(profile, name, &ids, !enabled)
}

/// id 写入 YAML 条目时的安全标量：`@` 等保留字符必须引号包裹，否则解析报
/// "found character that cannot start any token"。
fn yaml_id_scalar(id: &str) -> String {
    let plain = !id.is_empty()
        && id
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric() || c == '_')
            .unwrap_or(false)
        && id.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._/".contains(c));
    if plain {
        id.to_string()
    } else {
        format!("\"{}\"", id.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// 按真实插件 id 启停一个 bundle 的全部插件。启停只看用户 patch 层的 `disabled`
/// 字段，不区分条目是启动器写入还是手动添加：
/// 禁用 = 逐 id 追加启动器标记块；启用 = 移除该包全部 id 的禁用条目。
fn set_ids_disabled(
    profile: &str,
    bundle: &str,
    ids: &[String],
    disabled: bool,
) -> Result<(), String> {
    for id in ids {
        if id.trim().is_empty() || id.len() > 200 || id.contains('\n') {
            return Err(format!("插件包 {bundle} 含非法插件 id"));
        }
    }
    let path = user_patch_path(profile)?;
    let raw = std::fs::read_to_string(&path).unwrap_or_default();

    if disabled {
        // 已禁用的 id 不重复追加
        let already = disabled_ids_in(&raw);
        let todo: Vec<&String> = ids.iter().filter(|id| !already.contains(*id)).collect();
        if todo.is_empty() {
            return Ok(());
        }
        let mut out = raw.clone();
        if !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        for id in &todo {
            out.push_str(&format!(
                "\n{MANAGE_MARKER} {bundle}/{id}（启动器管理：关闭开关会自动移除此块；live 模式即时生效，startup 模式需重启实例）\n- id: {}\n  disabled: true\n",
                yaml_id_scalar(id)
            ));
        }
        validate_yaml(&out)?;
        backup(&path)?;
        std::fs::write(&path, out).map_err(|e| format!("写入失败: {e}"))?;
        return Ok(());
    }

    // 启用：该包各 id 的禁用条目一律移除——仅含 id+disabled 的条目整块移除
    //（连带紧邻的启动器标记注释）；还带其他配置的条目只删 disabled 行、保留配置。
    let targets: std::collections::BTreeSet<String> = ids.iter().cloned().collect();
    if !ids.iter().any(|id| disabled_ids_in(&raw).contains(id)) {
        return Err(format!("插件包 {bundle} 本就处于启用状态"));
    }

    let lines: Vec<&str> = raw.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("- ") {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        // 顶层条目块：起始行 + 后续缩进行/空行
        let mut j = i + 1;
        while j < lines.len() && (lines[j].starts_with(' ') || lines[j].trim().is_empty()) {
            j += 1;
        }
        let block = &lines[i..j];
        let item = yaml_item_of(&block.join("\n"));
        let hit = item
            .as_ref()
            .map(|it| {
                targets.contains(it.get("id").and_then(|v| v.as_str()).unwrap_or(""))
                    && it.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false)
            })
            .unwrap_or(false);

        if hit && item.as_ref().is_some_and(|it| only_id_disabled(it)) {
            drop_block_with_marker(&mut out);
            changed = true;
            i = j;
            continue;
        }
        if hit {
            // 条目还带其他配置：只删与 id 键同缩进的 disabled 行
            let key_indent = block[0].find("id:").unwrap_or(2);
            for l in strip_disabled_lines(block, key_indent, &mut changed) {
                out.push(l.to_string());
            }
            i = j;
            continue;
        }
        if item
            .as_ref()
            .and_then(|it| it.get("insert"))
            .is_some_and(|v| v.is_sequence())
        {
            // insert 条目：内层子项的禁用同样只看 disabled 字段
            let rebuilt = rebuild_insert_block(block, &targets, &mut changed);
            if rebuilt.is_empty() {
                drop_block_with_marker(&mut out);
                changed = true;
            } else {
                out.extend(rebuilt);
            }
            i = j;
            continue;
        }

        out.extend(block.iter().map(|l| l.to_string()));
        i = j;
    }

    let mut out_text = out.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    // 兜底：flow 写法等无法按行处理的情况，仍有目标 id 被禁用则报错
    if let Some(id) = ids.iter().find(|id| disabled_ids_in(&out_text).contains(*id)) {
        return Err(format!(
            "插件包 {bundle} 的插件 {id} 存在无法按行移除的 disabled 条目，请手动编辑 cordis.patch.yml"
        ));
    }
    validate_yaml(&out_text)?;
    backup(&path)?;
    std::fs::write(&path, out_text).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|&c| c == ' ').count()
}

/// 把顶层条目块文本（单元素 YAML 序列）解析出其中的映射节点
fn yaml_item_of(block_text: &str) -> Option<serde_yaml::Value> {
    match serde_yaml::from_str::<serde_yaml::Value>(block_text) {
        Ok(serde_yaml::Value::Sequence(seq)) => seq.into_iter().next(),
        _ => None,
    }
}

/// 条目是否只含 id / disabled 两个键（可整块移除而不丢用户配置）
fn only_id_disabled(item: &serde_yaml::Value) -> bool {
    item.as_mapping().is_some_and(|m| {
        m.keys()
            .all(|k| k.as_str().is_some_and(|s| s == "id" || s == "disabled"))
    })
}

/// 删除块内与 id 键同缩进的 disabled 行（保留其余配置），返回保留的行
fn strip_disabled_lines<'a>(
    block: &[&'a str],
    key_indent: usize,
    changed: &mut bool,
) -> Vec<&'a str> {
    let mut kept = Vec::with_capacity(block.len());
    for &line in block {
        if leading_spaces(line) == key_indent && line.trim_start().starts_with("disabled:") {
            *changed = true;
        } else {
            kept.push(line);
        }
    }
    kept
}

/// 整块移除时连带清理紧邻的启动器标记注释行（用户自己的注释保留）
fn drop_block_with_marker(out: &mut Vec<String>) {
    let mut last = out.len();
    while last > 0 && out[last - 1].trim().is_empty() {
        last -= 1;
    }
    if last > 0 && out[last - 1].trim_start().starts_with(MANAGE_MARKER) {
        out.truncate(last - 1);
    }
}

/// 处理 insert 条目块中内层子项的禁用，返回替换后的行；全部子项被移除时返回空
fn rebuild_insert_block(
    block: &[&str],
    targets: &std::collections::BTreeSet<String>,
    changed: &mut bool,
) -> Vec<String> {
    enum Action {
        Keep,
        Strip(usize),
        Drop,
    }
    let inner_indent = block
        .iter()
        .skip(1)
        .find(|l| !l.trim().is_empty())
        .map(|l| leading_spaces(l))
        .unwrap_or(0);
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut k = 1;
    while k < block.len() {
        let line = block[k];
        if line.trim().is_empty()
            || leading_spaces(line) != inner_indent
            || !line.trim_start().starts_with("- ")
        {
            k += 1;
            continue;
        }
        let mut e = k + 1;
        while e < block.len()
            && (block[e].trim().is_empty() || leading_spaces(block[e]) > inner_indent)
        {
            e += 1;
        }
        ranges.push((k, e));
        k = e;
    }
    if ranges.is_empty() {
        return block.iter().map(|l| l.to_string()).collect();
    }

    let mut actions: Vec<Action> = Vec::with_capacity(ranges.len());
    for (s, e) in &ranges {
        let item = yaml_item_of(&block[*s..*e].join("\n"));
        let hit = item
            .as_ref()
            .map(|it| {
                targets.contains(it.get("id").and_then(|v| v.as_str()).unwrap_or(""))
                    && it.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false)
            })
            .unwrap_or(false);
        actions.push(if !hit {
            Action::Keep
        } else if item.as_ref().is_some_and(|it| only_id_disabled(it)) {
            Action::Drop
        } else {
            Action::Strip(block[*s].find("id:").unwrap_or(inner_indent + 2))
        });
    }

    if actions.iter().all(|a| matches!(a, Action::Drop)) {
        return Vec::new();
    }
    let mut res: Vec<String> = Vec::new();
    let mut k = 0;
    while k < block.len() {
        if let Some(pos) = ranges.iter().position(|(s, _)| *s == k) {
            let (s, e) = ranges[pos];
            match &actions[pos] {
                Action::Keep => res.extend(block[s..e].iter().map(|l| l.to_string())),
                Action::Strip(indent) => {
                    for line in strip_disabled_lines(&block[s..e], *indent, changed) {
                        res.push(line.to_string());
                    }
                }
                Action::Drop => *changed = true,
            }
            k = e;
            continue;
        }
        res.push(block[k].to_string());
        k += 1;
    }
    res
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

/// 读 profile 内的文本文件（cordis.patch.yml 可编辑；package.json 只读 —— 写入仍归 dsh plugin 命令管）
pub fn read_profile_file(profile: &str, file: &str) -> Result<String, String> {
    let allowed = ["cordis.patch.yml", "package.json"];
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

// ── 基于 cordis.patch 的插件包启停 ─────────────────────

const MANAGE_MARKER: &str = "# dsh-launcher: disable";

fn user_patch_path(profile: &str) -> Result<PathBuf, String> {
    Ok(profile_dir(profile)?.join("cordis.patch.yml"))
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

// ── Web 快捷配置（接管 webserver / web-runtime / connection 三个 patch 条目） ──

const WEB_QUICK_MARKER: &str = "# dsh-launcher: web-quick";

/// web 快捷配置当前值（解析自 cordis.patch.yml；条目不存在 = present=false，键缺失 = None）
#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebQuickConfig {
    pub webserver_present: bool,
    pub host: Option<String>,
    pub port: Option<u64>,
    pub web_runtime_present: bool,
    pub open_browser: Option<bool>,
    pub print_url: Option<bool>,
    pub surface_context: Option<bool>,
    pub connection_present: bool,
    pub cookie_max_age_days: Option<i64>,
}

/// 保存载荷：三个条目由启动器整块生成。patch 条目会整体替换该行 config，
/// 所以每块的键必须成套写全；两条 trustedHosts 固定用 `!!js` 表达式联动
/// webStartup → webRuntime 信任链（见 @deepseek-ai/dsh-web-app 的 cordis.patch.yml 定义）。
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WebQuickConfigInput {
    pub host: String,
    pub port: u64,
    pub open_browser: bool,
    pub print_url: bool,
    pub surface_context: bool,
    pub cookie_max_age_days: i64,
}

/// 解析 patch 文本中三个目标条目的当前配置（纯 disabled 条目不算已配置）
fn web_quick_from_patch(raw: &str) -> WebQuickConfig {
    let mut out = WebQuickConfig::default();
    let Ok(serde_yaml::Value::Sequence(seq)) = serde_yaml::from_str::<serde_yaml::Value>(raw)
    else {
        return out;
    };
    for item in seq {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        if item
            .get("disabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let Some(cfg) = item.get("config") else {
            continue;
        };
        match id {
            "webserver" => {
                out.webserver_present = true;
                out.host = cfg.get("host").and_then(|v| v.as_str()).map(String::from);
                out.port = cfg.get("port").and_then(|v| v.as_u64());
            }
            "web-runtime" => {
                out.web_runtime_present = true;
                out.open_browser = cfg.get("openBrowser").and_then(|v| v.as_bool());
                out.print_url = cfg.get("printUrl").and_then(|v| v.as_bool());
                out.surface_context = cfg.get("surfaceContext").and_then(|v| v.as_bool());
            }
            "connection" => {
                out.connection_present = true;
                out.cookie_max_age_days = cfg.get("cookieMaxAgeDays").and_then(|v| v.as_i64());
            }
            _ => {}
        }
    }
    out
}

pub fn get_web_quick_config(profile: &str) -> Result<WebQuickConfig, String> {
    let raw = std::fs::read_to_string(user_patch_path(profile)?).unwrap_or_default();
    Ok(web_quick_from_patch(&raw))
}

/// YAML 单引号标量：内部单引号翻倍转义
fn yaml_single_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// 生成一个目标条目的整块文本（含启动器标记注释行）
fn web_quick_block(id: &str, input: &WebQuickConfigInput) -> String {
    let body = match id {
        "webserver" => format!(
            "- id: webserver\n  config:\n    host: {}\n    port: {}\n",
            yaml_single_quoted(input.host.trim()),
            input.port
        ),
        "web-runtime" => format!(
            "- id: web-runtime\n  config:\n    openBrowser: {}\n    printUrl: {}\n    surfaceContext: {}\n    trustedHosts: !!js ctx.webStartup.trustedHosts\n",
            input.open_browser, input.print_url, input.surface_context
        ),
        _ => format!(
            "- id: connection\n  config:\n    trustedHosts: !!js ctx.webRuntime.trustedHosts\n    cookieMaxAgeDays: {}\n",
            input.cookie_max_age_days
        ),
    };
    format!("{WEB_QUICK_MARKER} {id}（启动器管理：保存「快捷配置」整块覆盖此条目）\n{body}")
}

/// 整块替换时连带清理紧邻上一行的启动器 web-quick 标记注释（用户注释保留）
fn drop_web_quick_marker(out: &mut Vec<String>) {
    let mut last = out.len();
    while last > 0 && out[last - 1].trim().is_empty() {
        last -= 1;
    }
    if last > 0 && out[last - 1].trim_start().starts_with(WEB_QUICK_MARKER) {
        out.truncate(last - 1);
    }
}

/// 把三个 web 快捷配置条目整块写入用户 cordis.patch.yml：
/// 已有同 id 的 config 条目（非 disabled）就地整块替换（重复条目丢弃）；
/// 缺失的条目插到首个「插件启停」管理块之前（避免落在 disabled 条目之后又把它覆盖回启用），
/// 没有管理块则追加到末尾；空列表 `[]` 占位行移除。用户注释一律保留；写前备份 + YAML 校验。
pub fn set_web_quick_config(profile: &str, input: &WebQuickConfigInput) -> Result<(), String> {
    let host = input.host.trim();
    if host.is_empty() || host.len() > 253 || host.chars().any(|c| c.is_control()) {
        return Err("host 不能为空且不得包含控制字符".into());
    }
    if !(1..=65535).contains(&input.port) {
        return Err("port 需在 1-65535 之间".into());
    }
    if !(1..=3_650_000).contains(&input.cookie_max_age_days) {
        return Err("cookieMaxAgeDays 需在 1-3650000 之间".into());
    }

    let ids = ["webserver", "web-runtime", "connection"];
    let blocks: Vec<String> = ids.iter().map(|id| web_quick_block(id, input)).collect();

    let path = user_patch_path(profile)?;
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = raw.lines().collect();

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 16);
    let mut replaced = [false; 3];
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("- ") {
            // 空 `[]` 占位在补块后非法（flow 与 block 序列不能混用），先行移除
            if lines[i].trim() == "[]" && replaced.iter().any(|r| !r) {
                i += 1;
                continue;
            }
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        // 顶层条目块：起始行 + 后续缩进行/空行
        let mut j = i + 1;
        while j < lines.len() && (lines[j].starts_with(' ') || lines[j].trim().is_empty()) {
            j += 1;
        }
        let item = yaml_item_of(&lines[i..j].join("\n"));
        let id = item
            .as_ref()
            .and_then(|it| it.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string());
        let k = id.as_deref().and_then(|id| ids.iter().position(|x| *x == id));
        let is_config = item.as_ref().is_some_and(|it| {
            it.get("config").is_some()
                && !it
                    .get("disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
        });
        if let (Some(k), true) = (k, is_config) {
            if !replaced[k] {
                drop_web_quick_marker(&mut out);
                out.push(blocks[k].clone());
                replaced[k] = true;
            }
            // 同 id 重复条目直接丢弃，统一由唯一新块接管
            i = j;
            continue;
        }
        out.extend(lines[i..j].iter().map(|l| l.to_string()));
        i = j;
    }

    // 顶层必须是条目列表：是映射则不接管（多半是手写坏的文件，去 patch 编辑器排查）
    if !raw.trim().is_empty() {
        if let Ok(serde_yaml::Value::Mapping(_)) = serde_yaml::from_str::<serde_yaml::Value>(&raw) {
            return Err("cordis.patch.yml 顶层不是条目列表，无法写入快捷配置".into());
        }
    }

    let mut ins: Vec<String> = Vec::new();
    for (k, b) in blocks.iter().enumerate() {
        if replaced[k] {
            continue;
        }
        ins.push(String::new());
        ins.extend(b.trim_end_matches('\n').split('\n').map(String::from));
    }
    if !ins.is_empty() {
        // 插入点：首个插件启停管理标记之前；没有则追加到末尾
        let mut at = out
            .iter()
            .position(|l| l.starts_with(MANAGE_MARKER))
            .unwrap_or(out.len());
        while at > 0 && out[at - 1].trim().is_empty() {
            out.remove(at - 1);
            at -= 1;
        }
        if at < out.len() {
            ins.push(String::new());
        }
        out.splice(at..at, ins);
    }

    let mut out_text = out.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    validate_yaml(&out_text)?;
    backup(&path)?;
    std::fs::write(&path, out_text).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DSH_HOME 是进程级环境变量，用到它的测试必须串行执行
    static DSH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn parse_dump_layers_extracts_bundle_ids() {
        let text = "# == @deepseek-ai/dsh-base\n\
                    - id: timer\n\
                    - id: hmr\n\
                    # == @deepseek-ai/dsh-base, patched by @deepseek-ai/dsh-web-app\n\
                    - id: webserver\n\
                    # == @dshp/mcwiki-search\n\
                    - id: 'dshp-mcwiki-search'\n\
                    # == /home/u/.dsh/profiles/web/cordis.patch.yml\n\
                    - id: user-entry\n";
        let m = parse_dump_layers(text);
        assert_eq!(
            m.get("@deepseek-ai/dsh-base").unwrap(),
            &["timer".to_string(), "hmr".to_string(), "webserver".to_string()]
        );
        assert_eq!(
            m.get("@dshp/mcwiki-search").unwrap(),
            &["dshp-mcwiki-search".to_string()]
        );
        // 用户 patch 层按路径分节单独存在，resolve 阶段按 bundle 名过滤掉
        assert!(m.contains_key("/home/u/.dsh/profiles/web/cordis.patch.yml"));
    }

    #[test]
    fn bundle_ids_from_files_reads_patch_and_insert() {
        let tmp = std::env::temp_dir().join(format!("dsh-bid-test-{}", std::process::id()));
        let pkg = tmp.join("node_modules/@x/pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("cordis.patch.yml"),
            "- id: alpha\n- insert:\n  - id: beta\n  - id: gamma\n",
        )
        .unwrap();
        let m = bundle_ids_from_files(&tmp, &["@x/pkg".to_string()]);
        assert_eq!(
            m.get("@x/pkg").unwrap(),
            &["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn bundle_toggle_roundtrip() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-cfg-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "# my notes\n- id: webserver\n  config:\n    host: 0.0.0.0\n",
        )
        .unwrap();

        let bundle = "@dshp/mcwiki-search";
        let ids = vec!["dshp-mcwiki-search".to_string(), "@weird id".to_string()];

        // 禁用：按真实插件 id 追加管理块，`@` 等保留字符被安全引用，用户注释保留
        set_ids_disabled("web", bundle, &ids, true).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"), "用户注释必须保留");
        assert!(raw.contains("- id: dshp-mcwiki-search"));
        assert!(raw.contains("- id: \"@weird id\""));
        assert!(raw.contains("disabled: true"));
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();
        let disabled = patch_disabled_ids("web");
        assert!(disabled.contains("dshp-mcwiki-search") && disabled.contains("@weird id"));

        // 幂等：已全部禁用再禁用不追加
        set_ids_disabled("web", bundle, &ids, true).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert_eq!(raw.matches("dsh-launcher: disable").count(), 2);

        // 启用：该包管理块整体移除，用户内容保留
        set_ids_disabled("web", bundle, &ids, false).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"));
        assert!(raw.contains("- id: webserver"));
        assert!(!raw.contains("dsh-launcher: disable"));
        assert!(!raw.contains("disabled: true"));
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        // 再次启用应报“本就启用”
        assert!(set_ids_disabled("web", bundle, &ids, false).is_err());

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn backup_keeps_single_file_and_cleans_legacy() {
        let tmp = std::env::temp_dir().join(format!("dsh-bak-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("cordis.patch.yml");
        std::fs::write(&f, "v1").unwrap();
        // 旧版按时间戳堆积的备份应被清理
        std::fs::write(tmp.join("cordis.patch.launcher-bak-1700000000"), "old").unwrap();

        backup(&f).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join("cordis.patch.launcher-bak")).unwrap(),
            "v1"
        );

        std::fs::write(&f, "v2").unwrap();
        backup(&f).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join("cordis.patch.launcher-bak")).unwrap(),
            "v2"
        );

        let baks: Vec<String> = std::fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("launcher-bak"))
            .collect();
        assert_eq!(baks, vec!["cordis.patch.launcher-bak".to_string()]);

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn enable_removes_manual_disabled_entries() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 启停只看 disabled 字段：无启动器标记的手写禁用条目同样被开关移除。
        // 仅含 id+disabled 的条目整块移除；带其他配置的条目只删 disabled 行。
        let tmp = std::env::temp_dir().join(format!("dsh-manual-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "# my notes\n\
             - id: keepme\n  config:\n    host: 1\n\
             - id: plain\n  disabled: true\n\
             - id: withcfg\n  disabled: true\n  config:\n    port: 80\n\
             - insert:\n  - id: inner-plain\n    disabled: true\n  - id: inner-cfg\n    disabled: true\n    config:\n      a: 1\n",
        )
        .unwrap();

        let ids: Vec<String> = ["plain", "withcfg", "inner-plain", "inner-cfg"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        set_ids_disabled("web", "@x/pkg", &ids, false).unwrap();

        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"), "用户注释必须保留");
        assert!(raw.contains("- id: keepme") && raw.contains("host: 1"));
        assert!(raw.contains("- id: withcfg") && raw.contains("port: 80"));
        assert!(raw.contains("- id: inner-cfg") && raw.contains("a: 1"));
        assert!(!raw.contains("disabled: true"), "所有禁用应被移除:\n{raw}");
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();
        assert!(disabled_ids_in(&raw).is_empty());

        // 已无禁用条目时再启用应报“本就处于启用状态”
        assert!(set_ids_disabled("web", "@x/pkg", &["plain".to_string()], false).is_err());

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn web_quick_replaces_config_entries_in_place() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-webq-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        // 初始：用户注释 + webserver 配置 + 一个无关的插件禁用管理块
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "# user notes\n\
             - id: webserver\n  config:\n    host: '0.0.0.0'\n    port: 3080\n\
             \n\
             # dsh-launcher: disable x/y（启动器管理：关闭开关会自动移除此块）\n\
             - id: other\n  disabled: true\n",
        )
        .unwrap();

        let input = WebQuickConfigInput {
            host: "127.0.0.1".into(),
            port: 3081,
            open_browser: true,
            print_url: false,
            surface_context: true,
            cookie_max_age_days: 36500,
        };
        set_web_quick_config("web", &input).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();

        // 用户注释保留；webserver 就地替换；web-runtime/connection 整块写入且键成套
        assert!(raw.contains("# user notes"), "用户注释必须保留:\n{raw}");
        assert!(raw.contains("host: '127.0.0.1'") && raw.contains("port: 3081"));
        assert!(!raw.contains("0.0.0.0"), "旧配置应被整块替换:\n{raw}");
        assert!(raw.contains("openBrowser: true"));
        assert!(raw.contains("printUrl: false"));
        assert!(raw.contains("surfaceContext: true"));
        assert!(raw.contains("trustedHosts: !!js ctx.webStartup.trustedHosts"));
        assert!(raw.contains("trustedHosts: !!js ctx.webRuntime.trustedHosts"));
        assert!(raw.contains("cookieMaxAgeDays: 36500"));
        // 无关的禁用管理块保留，且新条目都在它之前（否则 disabled 会被覆盖回启用）
        assert!(raw.contains("- id: other") && raw.contains("disabled: true"));
        let disable_pos = raw.find("# dsh-launcher: disable").unwrap();
        for id in ["webserver", "web-runtime", "connection"] {
            assert!(
                raw.find(&format!("- id: {id}")).unwrap() < disable_pos,
                "{id} 应在管理禁用块之前:\n{raw}"
            );
        }
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        // 回读
        let cfg = get_web_quick_config("web").unwrap();
        assert!(cfg.webserver_present && cfg.web_runtime_present && cfg.connection_present);
        assert_eq!(cfg.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(cfg.port, Some(3081));
        assert_eq!(cfg.open_browser, Some(true));
        assert_eq!(cfg.print_url, Some(false));
        assert_eq!(cfg.surface_context, Some(true));
        assert_eq!(cfg.cookie_max_age_days, Some(36500));

        // 再次保存：幂等（块唯一、标记不堆积）
        set_web_quick_config("web", &input).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert_eq!(raw.matches("- id: webserver\n").count(), 1, "webserver 块唯一:\n{raw}");
        assert_eq!(raw.matches("- id: web-runtime\n").count(), 1);
        assert_eq!(raw.matches("- id: connection\n").count(), 1);
        assert_eq!(raw.matches(WEB_QUICK_MARKER).count(), 3);
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn web_quick_appends_to_empty_patch() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-webq2-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        // 空列表占位（headless 的初始形态）：补块后 `[]` 必须移除，注释保留
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "# header comment\n[]\n",
        )
        .unwrap();

        let input = WebQuickConfigInput {
            host: "0.0.0.0".into(),
            port: 3080,
            open_browser: false,
            print_url: true,
            surface_context: true,
            cookie_max_age_days: 30,
        };
        set_web_quick_config("web", &input).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# header comment"));
        assert!(!raw.contains("[]"), "`[]` 占位应被移除:\n{raw}");
        assert!(raw.contains("host: '0.0.0.0'"));
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        let cfg = get_web_quick_config("web").unwrap();
        assert!(cfg.webserver_present && cfg.web_runtime_present && cfg.connection_present);

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn web_quick_missing_entries_report_absent() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-webq3-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        // 纯禁用条目 / 无 config 条目 / 缺键：都不算已接管
        std::fs::write(
            prof_dir.join("cordis.patch.yml"),
            "- id: webserver\n  disabled: true\n\
             - id: web-runtime\n  other: 1\n",
        )
        .unwrap();
        let cfg = get_web_quick_config("web").unwrap();
        assert!(!cfg.webserver_present);
        assert!(!cfg.web_runtime_present);
        assert!(!cfg.connection_present);

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn web_quick_rejects_invalid_input() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-webq4-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        std::fs::write(prof_dir.join("cordis.patch.yml"), "[]\n").unwrap();

        let base = |host: &str, port: u64, days: i64| WebQuickConfigInput {
            host: host.into(),
            port,
            open_browser: true,
            print_url: true,
            surface_context: true,
            cookie_max_age_days: days,
        };
        assert!(set_web_quick_config("web", &base("", 3080, 30)).is_err());
        assert!(set_web_quick_config("web", &base("0.0.0.0", 0, 30)).is_err());
        assert!(set_web_quick_config("web", &base("0.0.0.0", 70000, 30)).is_err());
        assert!(set_web_quick_config("web", &base("0.0.0.0", 3080, 0)).is_err());
        // 全部非法时不应动原文件
        assert_eq!(
            std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap(),
            "[]\n"
        );

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 真实数据冒烟：把 ~/.dsh/profiles/web/cordis.patch.yml（含 !!js、insert、大量管理禁用块）
    /// 拷进临时 DSH_HOME 后完整跑一遍 get/set，只读真实文件、写入全部发生在临时目录。
    #[test]
    fn web_quick_real_home_smoke() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let real = PathBuf::from(home).join(".dsh/profiles/web/cordis.patch.yml");
        let Ok(real_raw) = std::fs::read_to_string(&real) else {
            return;
        };
        let tmp = std::env::temp_dir().join(format!("dsh-webq-real-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        std::fs::write(prof_dir.join("cordis.patch.yml"), &real_raw).unwrap();

        let before = get_web_quick_config("web").unwrap();
        let input = WebQuickConfigInput {
            host: before.host.clone().unwrap_or_else(|| "127.0.0.1".into()),
            port: before.port.unwrap_or(3080),
            open_browser: before.open_browser.unwrap_or(true),
            print_url: before.print_url.unwrap_or(true),
            surface_context: before.surface_context.unwrap_or(true),
            cookie_max_age_days: before.cookie_max_age_days.unwrap_or(36500),
        };
        set_web_quick_config("web", &input).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();

        // 结构不变量：YAML 合法；块唯一；用户条目/insert/管理禁用块保留
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();
        assert_eq!(raw.matches("- id: webserver\n").count(), 1);
        assert_eq!(raw.matches("- id: web-runtime\n").count(), 1);
        assert_eq!(raw.matches("- id: connection\n").count(), 1);
        for id in ["web", "dshp-inx-qqbot", "genui", "dsh-market"] {
            if real_raw.contains(&format!("- id: {id}")) {
                assert!(raw.contains(&format!("- id: {id}")), "{id} 条目丢失");
            }
        }
        if real_raw.contains("- insert:") {
            assert!(raw.contains("- insert:"), "insert 块丢失");
        }
        // 管理禁用块数量不变，且新写入条目都位于首个管理块之前
        assert_eq!(raw.matches(MANAGE_MARKER).count(), real_raw.matches(MANAGE_MARKER).count());
        let disable_pos = raw.find(MANAGE_MARKER).unwrap();
        for id in ["webserver", "web-runtime", "connection"] {
            assert!(raw.find(&format!("- id: {id}")).unwrap() < disable_pos);
        }
        // 回读与输入一致
        let after = get_web_quick_config("web").unwrap();
        assert_eq!(after.host.as_deref(), Some(input.host.as_str()));
        assert_eq!(after.port, Some(input.port));
        assert_eq!(after.cookie_max_age_days, Some(input.cookie_max_age_days));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }
}
