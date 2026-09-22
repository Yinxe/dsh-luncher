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
    /// 宿主自带（in-box）：前端据此禁用「卸载」与启停开关
    pub official: bool,
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

/// package.json dependencies 里的一个直接依赖（含未声明为 bundle 的「非插件包」，
/// 供手动卸载误装/残留依赖）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackageDepInfo {
    pub name: String,
    pub version: Option<String>,
    pub source: String,
    /// 是否被 dsh.profile.bundles 声明为插件 bundle
    pub is_bundle: bool,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetail {
    pub profile: String,
    pub exists: bool,
    pub bundles: Vec<BundleInfo>,
    /// dependencies 全量直接依赖（bundles 之外的就是「不是插件但装进来了」的包）
    pub packages: Vec<PackageDepInfo>,
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

/// 依赖来源分类（决定更新检测方式与前端徽标）
/// npm=registry 包 / git=GitHub 等 git 规格 / tarball=打包产物直链 /
/// git-clone=克隆到本地的仓库再 link / link=普通本地链接 / file / workspace
fn source_of(version_value: Option<&str>) -> &'static str {
    let Some(v) = version_value else { return "npm" };
    let v = v.trim();
    if v.starts_with("workspace:") {
        return "workspace";
    }
    if v.starts_with("link:") || v.starts_with("file:") {
        // 指向本地 git 工作树 = clone+link 安装 → 可用 git pull 更新
        let is_git = crate::plugin::link_target(v)
            .and_then(|p| find_git_root(&p))
            .is_some();
        return if is_git { "git-clone" } else { "link" };
    }
    if v.starts_with("http://") || v.starts_with("https://") {
        if v.ends_with(".tgz")
            || v.ends_with(".tar.gz")
            || v.ends_with(".tar")
            || v.contains("/releases/download/")
        {
            return "tarball";
        }
        return "link";
    }
    if v.starts_with("git+")
        || v.starts_with("github:")
        || v.starts_with("git@")
        || v.contains(".git#")
        || v.ends_with(".git")
    {
        return "git";
    }
    "npm"
}

/// 从某个本地路径向上查找 git 工作树根（含 .git 的最近祖先目录）
pub fn find_git_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let mut cur: Option<&Path> = Some(&start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
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
    let mut packages = Vec::new();
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
                official: is_inbox_bundle(name),
            });
        }
        // dependencies 全量直接依赖：不在 bundles 里的就是「装了但不是插件」的包
        if let Some(deps) = deps {
            for (name, ver) in deps {
                let v = ver.as_str().map(String::from);
                let is_bundle = bundles_list.iter().any(|b| b == name);
                packages.push(PackageDepInfo {
                    name: name.clone(),
                    version: v.clone(),
                    source: source_of(v.as_deref()).to_string(),
                    is_bundle,
                });
            }
            packages.sort_by(|a, b| a.name.cmp(&b.name));
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
        packages,
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

/// 覆写前先备份原文件。固定单一备份文件 <名>.starter-bak，每次覆写覆盖同一份；
/// 同时清理旧版按时间戳堆积的 <名>.starter-bak-<ts> 备份。
pub(crate) fn backup(path: &Path) -> Result<(), String> {
    let content = match std::fs::read(path) {
        Ok(c) => c,
        Err(_) => return Ok(()), // 目标不存在则无从备份
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bak = path.with_file_name(format!("{stem}.starter-bak"));
    std::fs::write(&bak, content).map_err(|e| format!("写备份失败: {e}"))?;
    // 备份必须继承源文件权限：凭据文件是 0600，它的备份（同样躺着明文 token）
    // 若按默认 umask 落成 0644，多用户机器上任一次保存都把全套 token 敞开可读。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let src_mode = meta.permissions().mode() & 0o777;
            let _ = std::fs::set_permissions(
                &bak,
                std::fs::Permissions::from_mode(src_mode),
            );
        }
    }
    if let Some(dir) = path.parent() {
        let legacy_prefix = format!("{stem}.starter-bak-");
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
        .map_err(|e| {
            // 前端已按行列拦一道，这里是写盘前的权威兜底：同样带上「第 X 行第 Y 列」，
            // 让用户拿到的报错能直接对到编辑器行号（Location 已做 1 起始换算）。
            let loc = e.location().map(|l| format!("第 {} 行第 {} 列：", l.line(), l.column()));
            let located = |msg: &str| match loc {
                Some(l) => format!("{l}{msg}"),
                None => msg.to_string(),
            };
            let msg = e.to_string();
            // 「多文档」来路基本只有一个：顶层 `[]`（flow 写法）后面接了 `- id: …` 这类
            // block 条目 —— `[]` 自己就构成整个文档，后面只能靠 `---` 开新文档。原文
            // 只说了「不支持多文档」，一个字都没说该怎么改，这里补上下一步动作。
            if msg.contains("more than one document") {
                return format!(
                    "YAML 语法错误：{}\n\
                     提示：这份文件里有多个 YAML 文档 —— 顶层的 `[]` 是 flow 写法，\
                     后面不能再接 `- id: …` 这样的 block 条目。删掉那行 `[]`（条目照旧逐条列出）即可；\
                     单个 `[]` 本身是合法的空补丁层。",
                    located(&msg)
                );
            }
            format!("YAML 语法错误：{}", located(&msg))
        })
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
        let out = util::hidden_command(&node)
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
    // 宿主自带的包不允许停用（后端权威闸门；前端也会把开关置灰）
    if is_inbox_bundle(name) {
        // enabled=true 是「启用」，false 是「停用」——两个方向都拒绝
        return Err(inbox_bundle_reject(name, if enabled { "启用" } else { "停用" }));
    }
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
    set_ids_disabled(profile, name, &ids, !enabled)?;
    crate::diag::info(
        "profile",
        &format!(
            "插件包启停：profile「{profile}」{name} → {}（涉及 {} 个插件 id）",
            if enabled { "启用" } else { "停用" },
            ids.len()
        ),
    );
    Ok(())
}

/// 顶层「空 flow 占位」行判定：profile 模板自带的 patch 文件就是
/// 一串注释 + 一行 `[]`（顶层类型是 flow 序列）。
fn is_empty_flow_placeholder(line: &str) -> bool {
    leading_spaces(line) == 0 && matches!(line.trim(), "[]" | "[ ]")
}

/// 追加 block 条目之前摘掉顶层空 flow 占位行。
///
/// 不摘会变成「一个文件两个 YAML 文档」—— `[]` 是一个完整的 flow 序列，后面再接
/// `- id: …` 就是新的文档，serde_yaml 直接拒绝：
/// `deserializing from YAML containing more than one document is not supported`。
/// 于是「关掉任意插件开关」在全新 profile 上必然失败（web 快捷配置那条路径早已
/// 单独处理过这个占位，插件启停这条漏了）。逐行复制、保留每行原有换行，不重写整份文件。
fn drop_empty_flow_placeholder(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.split_inclusive('\n') {
        if is_empty_flow_placeholder(line) {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// 文件里是否还剩「非注释的实际内容」。
fn has_patch_content(raw: &str) -> bool {
    raw.lines().any(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })
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
    // 只有「文件不存在」才当作空；权限 / IO 等读取失败要拒绝写入，否则会用只含本次改动的
    // 新内容覆盖掉磁盘上已有的 patch 文件，静默丢掉其它条目。
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取插件配置失败，已拒绝写入以免覆盖现有内容：{e}")),
    };

    if disabled {
        // 已禁用的 id 不重复追加
        let already = disabled_ids_in(&raw);
        let todo: Vec<&String> = ids.iter().filter(|id| !already.contains(*id)).collect();
        if todo.is_empty() {
            return Ok(());
        }
        let mut out = drop_empty_flow_placeholder(&raw);
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
    // 条目被清空后不能只剩注释：patch 文件的契约是「顶层 YAML 数组」，空文档是 null，
    // dsh 拿到的就不再是补丁列表。这时把模板那份 `[]` 占位放回去（文件回到初始形态）。
    if !has_patch_content(&out_text) {
        out_text.push_str("[]\n");
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
    crate::diag::info(
        "profile",
        &format!("profile「{profile}」配置已保存：{file}（{} 字节）", content.len()),
    );
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
    // settings.yaml 含 apiKeyEnv 等引用信息，只记路径与字节数
    crate::diag::info(
        "profile",
        &format!("全局配置已保存：{}（{} 字节）", path.display(), content.len()),
    );
    Ok(())
}

// ── 基于 cordis.patch 的插件包启停 ─────────────────────

const MANAGE_MARKER: &str = "# dsh-starter: disable";

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

const WEB_QUICK_MARKER: &str = "# dsh-starter: web-quick";

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
/// printUrl 不开放配置：启动器依赖启动日志 `dsh web: <url>` 识别访问地址，恒为 true。
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WebQuickConfigInput {
    pub host: String,
    pub port: u64,
    pub open_browser: bool,
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

/// 该 profile 的 web 监听地址 `(host, port)`；仅当 patch 里存在**启用**的
/// webserver 条目且带 port 时返回。
///
/// 用途：端口是 web 实例的稳定身份。启动器重启 / `detached.json` 丢失 / 终端外部
/// 启动，只要端口还在监听就能重新发现实例（见 `crate::netports`）。
/// 没配 webserver 条目时返回 `None`——此时不得猜默认端口，否则别的程序占着 3080
/// 会被误判成该 profile 在运行。
pub fn web_addr(profile: &str) -> Option<(String, u16)> {
    let cfg = get_web_quick_config(profile).ok()?;
    let port = u16::try_from(cfg.port?).ok()?;
    let host = cfg
        .host
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    Some((host, port))
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
            "- id: web-runtime\n  config:\n    openBrowser: {}\n    printUrl: true\n    surfaceContext: {}\n    trustedHosts: !!js ctx.webStartup.trustedHosts\n",
            input.open_browser, input.surface_context
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
    // 只有「文件不存在」才当作空；权限 / IO 等读取失败要拒绝写入，否则会用只含本次改动的
    // 新内容覆盖掉磁盘上已有的 patch 文件，静默丢掉其它条目。
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取插件配置失败，已拒绝写入以免覆盖现有内容：{e}")),
    };
    let lines: Vec<&str> = raw.lines().collect();

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 16);
    let mut replaced = [false; 3];
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].starts_with("- ") {
            // 空 flow 占位在补块后非法（flow 与 block 序列不能混用），先行移除。
            // 判定与插件启停共用 `is_empty_flow_placeholder`：两条路径改的是同一份文件，
            // 对「什么算空占位」必须是同一套（含带空格的 `[ ]`）。
            if is_empty_flow_placeholder(lines[i]) && replaced.iter().any(|r| !r) {
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
    let written = out_text.len();
    std::fs::write(&path, out_text).map_err(|e| format!("写入失败: {e}"))?;
    crate::diag::info(
        "profile",
        &format!(
            "web 快捷配置已保存：profile「{profile}」{}:{}（{written} 字节）",
            input.host, input.port
        ),
    );
    Ok(())
}

// ── 复制 profile 实例 ─────────────────────────

/// 复制 profile：目录型整目录拷贝（跳过 node_modules / cache 等可重建的运行时产物）；
/// 文件型（profiles/ 下的 yaml/json）拷为「新名.同扩展名」文件。实例名需手动输入并过
/// 合法性校验，目标已存在则拒绝。
///
/// **注意**：dsh 不会在首次启动时自动重装依赖 —— bundle 解析不到直接报错退出。
/// 复制出来的副本必须走 [`copy_profile_with_deps`]，由启动器补一次官方安装命令。
pub fn copy_profile(source: &str, new_name: &str) -> Result<(), String> {
    let source = source.trim();
    let name = new_name.trim();
    validate_profile_name(name)?;
    let root = profiles::profiles_dir();
    let dst = root.join(name);
    if dst.exists() {
        return Err(format!("实例「{name}」已存在"));
    }
    // 与同名文件型 profile 撞名会让扫描出两个同名条目，一并拒绝
    for ext in ["yaml", "yml", "json"] {
        if root.join(format!("{name}.{ext}")).is_file() {
            return Err(format!("已存在同名文件型 profile「{name}.{ext}」"));
        }
    }
    let src_dir = root.join(source);
    let r = if src_dir.is_dir() {
        copy_dir_excluding(&src_dir, &dst, &["node_modules", "cache"])
    } else {
        let src = ["yaml", "yml", "json"]
            .iter()
            .find_map(|ext| {
                let p = root.join(format!("{source}.{ext}"));
                p.is_file().then_some(p)
            })
            .ok_or_else(|| format!("源 profile「{source}」不存在"))?;
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("yaml");
        std::fs::copy(&src, root.join(format!("{name}.{ext}")))
            .map(|_| ())
            .map_err(|e| format!("复制失败: {e}"))
    };
    if r.is_ok() {
        crate::diag::info("profile", &format!("profile 已复制：{source} → {name}"));
    }
    r
}

/// 复制的完整流程：目录拷贝 → 补装依赖 → 错开 web 端口。
/// 依赖装不上就把副本整体回滚（副本目录是我们刚建的，删掉无副作用），
/// 保证「复制成功 = 可启动」；用户修好网络/镜像后重试即可。
pub fn copy_profile_with_deps(
    settings: &Settings,
    source: &str,
    new_name: &str,
) -> Result<Option<u16>, String> {
    copy_profile(source, new_name)?;
    let name = new_name.trim();
    if let Err(e) = install_copied_deps(settings, name) {
        let dir = profiles::profiles_dir().join(name);
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(&dir);
            crate::diag::warn("profile", &format!("副本「{name}」依赖安装失败，已回滚删除"));
        }
        return Err(format!(
            "副本依赖自动安装失败，已回滚（未留下半成品）：{e}\n\
             请检查网络 / npm 镜像后重试；也可先关闭试用，稍后在「插件管理」里手动补装。"
        ));
    }
    assign_free_web_port(name)
}

/// 副本跳过了 node_modules，而 dsh 启动时解析不到 bundle 会直接报错退出，
/// 所以复制后替用户跑一次官方 `dsh plugin --profile <名> install`。
/// 文件型 profile（无目录）没有依赖可装，直接返回 Ok。
fn install_copied_deps(settings: &Settings, name: &str) -> Result<(), String> {
    let dir = profiles::profiles_dir().join(name);
    if !dir.is_dir() || !dir.join("package.json").is_file() {
        return Ok(());
    }
    let (node, bin_js) = resolve_dsh_bin(settings)?;
    let mut cmd = util::hidden_command(&node);
    cmd.arg(&bin_js)
        .arg("plugin")
        .arg("--profile")
        .arg(name)
        .arg("install")
        .current_dir(&dir)
        .env(
            "DSH_HOME",
            profiles::dsh_native_home().to_string_lossy().into_owned(),
        )
        // pnpm 10+ 没有 TTY 时会卡在静默交互提示上；CI 模式让它直接干活或报错
        .env("CI", "true")
        .env("npm_config_update_notifier", "false");
    // git（含 pnpm 内部调用 git）一律不弹账号密码提示：宁可失败也要把原因写进输出
    for (k, v) in crate::plugin::git_no_prompt_env() {
        cmd.env(k, v);
    }
    util::with_node_on_path(&mut cmd, Some(&node));
    crate::diag::info("profile", &format!("副本「{name}」开始安装依赖（dsh plugin install）"));
    let out = cmd
        .output()
        .map_err(|e| format!("无法执行 dsh plugin install（{}）: {e}", bin_js.display()))?;
    if !out.status.success() {
        let mut text = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if text.is_empty() {
            text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
        let short: String = text.chars().rev().take(2000).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(if short.is_empty() {
            format!("退出码 {:?}", out.status.code())
        } else {
            short
        });
    }
    crate::diag::info("profile", &format!("副本「{name}」依赖安装完成"));
    Ok(())
}

/// 递归复制目录，跳过指定名称的子目录（可重建的运行时产物）与符号链接等非普通文件
fn copy_dir_excluding(src: &Path, dst: &Path, skip: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {e}"))?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| format!("读取目录失败: {e}"))?
        .flatten()
    {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            if skip.contains(&name.as_str()) {
                continue;
            }
            copy_dir_excluding(&entry.path(), &dst.join(&name), skip)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), dst.join(&name))
                .map_err(|e| format!("复制文件 {name} 失败: {e}"))?;
        }
    }
    Ok(())
}

// ── profile 改名 / 删除 / 恢复模式 ──────────────

/// 实例名合法性：启动器与 dsh 都按目录名解析，必须挡住路径穿越与保留目录
fn validate_profile_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('.')
        || name == "node_modules"
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err("非法实例名：不能为空、以 . 开头、为 node_modules 或包含路径字符".into());
    }
    Ok(())
}

/// 定位已存在的 profile（目录型，或 yaml/yml/json 文件型）
fn existing_profile_path(name: &str) -> Option<PathBuf> {
    let root = profiles::profiles_dir();
    let dir = root.join(name);
    if dir.is_dir() {
        return Some(dir);
    }
    ["yaml", "yml", "json"].iter().find_map(|ext| {
        let f = root.join(format!("{name}.{ext}"));
        f.is_file().then_some(f)
    })
}

/// 移动文件/目录；DSH_HOME 与启动器目录不在同一磁盘时退化为复制 + 删除
fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if src.is_dir() {
        copy_dir_excluding(src, dst, &[])?;
        std::fs::remove_dir_all(src).map_err(|e| format!("清理原目录失败: {e}"))
    } else {
        std::fs::copy(src, dst)
            .map(|_| ())
            .map_err(|e| format!("复制失败: {e}"))?;
        std::fs::remove_file(src).map_err(|e| format!("清理原文件失败: {e}"))
    }
}

/// 重命名 profile。dsh 内置保留 profile（headless/web/desktop）一律拒绝——
/// 改名会破坏 dsh 自身的默认 profile 与核心数据。调用方需先确认无实例在运行。
pub fn rename_profile(old: &str, new_name: &str) -> Result<String, String> {
    let old = old.trim();
    let name = new_name.trim();
    if profiles::is_reserved_profile(old) {
        return Err(format!(
            "「{old}」是 dsh 内置保留 profile，不允许重命名（避免核心数据丢失）"
        ));
    }
    validate_profile_name(name)?;
    if profiles::is_reserved_profile(name) {
        return Err(format!("不能改名成内置保留名「{name}」"));
    }
    let Some(src) = existing_profile_path(old) else {
        return Err(format!("profile「{old}」不存在"));
    };
    if existing_profile_path(name).is_some() {
        return Err(format!("实例「{name}」已存在"));
    }
    let root = profiles::profiles_dir();
    let dst = if src.is_dir() {
        root.join(name)
    } else {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("yaml");
        root.join(format!("{name}.{ext}"))
    };
    move_path(&src, &dst)?;
    crate::diag::info(
        "profile",
        &format!("profile 已重命名：{old} → {name}（{}）", dst.display()),
    );
    Ok(dst.to_string_lossy().into_owned())
}

/// 删除 profile：不直接 `rm -rf`，而是移入 `~/.dsh-starter/deleted-profiles/`，
/// 误删可手动找回。dsh 内置保留 profile 一律拒绝。返回回收站路径。
pub fn delete_profile(name: &str) -> Result<String, String> {
    let name = name.trim();
    if profiles::is_reserved_profile(name) {
        return Err(format!(
            "「{name}」是 dsh 内置保留 profile，不允许删除（避免核心数据丢失）"
        ));
    }
    let Some(src) = existing_profile_path(name) else {
        return Err(format!("profile「{name}」不存在"));
    };
    let trash = crate::settings::starter_home().join("deleted-profiles");
    std::fs::create_dir_all(&trash).map_err(|e| format!("创建回收目录失败: {e}"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dst = if src.is_dir() {
        trash.join(format!("{name}-{ts}"))
    } else {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("yaml");
        trash.join(format!("{name}-{ts}.{ext}"))
    };
    move_path(&src, &dst)?;
    crate::diag::info(
        "profile",
        &format!("profile「{name}」已删除并移入回收：{}", dst.display()),
    );
    Ok(dst.to_string_lossy().into_owned())
}

/// 恢复模式只允许这两个官方内置插件（= dsh 随附 web 模板的内容）
const OFFICIAL_BUNDLES: &[&str] = &["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"];

/// dsh 宿主自带（in-box）的插件包：**不允许卸载、不允许停用**。
///
/// 它们由 dsh 安装本体提供，不是 profile 的普通依赖：
/// - `@deepseek-ai/dsh-base` 提供核心运行时（timer / hmr / llm / session …），
///   卸掉或停掉整个 profile 都起不来；
/// - `@deepseek-ai/dsh-web-app` 提供 Web GUI 表面（消息反馈、会话日志、打开方式…），
///   Web profile 少了它就没有界面；
/// - `@deepseek-ai/dsh-headless` 同理，用于 headless profile。
///
/// 命名与语义对齐同生态的 dshmarket（它的 `INBOX_BUNDLES` 同样把这几个视为
/// "ships with the dsh host and must stay put"）。
pub const INBOX_BUNDLES: &[&str] = &[
    "@deepseek-ai/dsh-base",
    "@deepseek-ai/dsh-web-app",
    "@deepseek-ai/dsh-headless",
];

/// 该包是否是宿主自带、需受保护的插件包
pub fn is_inbox_bundle(name: &str) -> bool {
    INBOX_BUNDLES.contains(&name.trim())
}

/// 受保护包被操作时的统一拒绝话术
pub fn inbox_bundle_reject(name: &str, action: &str) -> String {
    format!(
        "{name} 是 dsh 宿主自带的插件包，不能{action}：它由 dsh 安装本体提供，\
         卸载或停用会让这个 profile 起不来（base 提供核心运行时，web-app 提供 Web GUI）。\n\
         若要一个只含官方插件、可放心折腾的实例，请用「恢复模式」新建一个 profile。"
    )
}

/// 把 package.json 的 bundles 收敛到官方两个，并同步移除对应的 dependencies。
/// 恢复模式的骨架由 dsh 的 `--from-default-profile web` 生成、本来就是这两个；
/// 这里是防呆兜底（模板被本地改过时），保证它永远不加载也不安装第三方插件。
fn prune_to_official_bundles(dir: &Path) -> Result<(), String> {
    let path = dir.join("package.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let mut v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("解析 package.json 失败: {e}"))?;

    let removed: Vec<String> = v
        .pointer("/dsh/profile/bundles")
        .and_then(|b| b.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|b| b.as_str())
                .filter(|b| !OFFICIAL_BUNDLES.contains(b))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if let Some(arr) = v
        .pointer_mut("/dsh/profile/bundles")
        .and_then(|b| b.as_array_mut())
    {
        arr.clear();
        for b in OFFICIAL_BUNDLES {
            arr.push(serde_json::Value::String((*b).to_string()));
        }
    }
    if let Some(deps) = v.get_mut("dependencies").and_then(|d| d.as_object_mut()) {
        for r in &removed {
            deps.remove(r);
        }
    }
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, out).map_err(|e| format!("写入 package.json 失败: {e}"))
}

/// 在 base 之上找一个「邻近、当前空闲、也没被别的 profile 配置占用」的端口
/// （复制实例、恢复模式共用这一套错开逻辑）
fn pick_free_nearby_port(host: &str, base: u16) -> Result<u16, String> {
    let used: Vec<u16> = profiles::scan_profiles()
        .iter()
        .filter_map(|p| web_addr(&p.name).map(|(_, port)| port))
        .collect();
    for offset in 1..=200u16 {
        let Some(port) = base.checked_add(offset) else {
            break;
        };
        if used.contains(&port) || crate::netports::is_listening(host, port) {
            continue;
        }
        return Ok(port);
    }
    Err(format!(
        "{base} 附近 200 个端口都被占用，请创建后到「快捷配置」手动指定端口"
    ))
}

/// 给某个 web profile 换一个邻近的空闲端口（原端口已被占用/与其它 profile 冲突时）。
/// 返回 `Ok(None)` = 该 profile 没有 webserver 配置（如 headless），无需处理。
/// 用于「复制实例后自动错开端口」，避免用户复制完两个实例抢同一个端口。
pub fn assign_free_web_port(profile: &str) -> Result<Option<u16>, String> {
    let Some((host, base)) = web_addr(profile) else {
        return Ok(None);
    };
    let cfg = get_web_quick_config(profile).unwrap_or_default();
    let port = pick_free_nearby_port(&host, base)?;
    set_web_quick_config(
        profile,
        &WebQuickConfigInput {
            host,
            port: port as u64,
            open_browser: cfg.open_browser.unwrap_or(true),
            surface_context: cfg.surface_context.unwrap_or(true),
            cookie_max_age_days: cfg.cookie_max_age_days.unwrap_or(36500),
        },
    )?;
    Ok(Some(port))
}

/// 恢复模式 profile 名：用 dsh 随附的 `web` 模板新建（web-Recovery）
pub const RECOVERY_PROFILE: &str = "web-Recovery";

/// 恢复模式取自 dsh 随附模板里的 `web`（= 官方 base + web-app）
const RECOVERY_TEMPLATE: &str = "web";

/// dsh web 的默认端口：连官方 web profile 都还没落 webserver 配置时，以它作错开基准
const DEFAULT_WEB_PORT: u16 = 3080;

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCreated {
    pub name: String,
    pub port: u16,
}

/// 恢复模式的目标目录。dsh 只认 `$DSH_HOME/profiles`（单数 `profile` 是历史兼容），
/// 所以这里不跟 `profiles_dir()` 的单数回退走。
fn recovery_dir() -> PathBuf {
    profiles::dsh_native_home()
        .join("profiles")
        .join(RECOVERY_PROFILE)
}

/// 让 dsh 自己按随附模板初始化 profile 骨架：
/// `dsh --profile <name> --from-default-profile <template> --dump-default-config`
///
/// `--from-default-profile` 负责从随附模板建出 package.json / cordis.patch.yml /
/// pnpm-workspace.yaml；后面接 `--dump-default-config` 而不是真的 boot，是为了让它
/// 打印完配置树就退出 —— 建完不启动服务，也不去解析（可能正是被第三方插件弄坏的）
/// 用户补丁层。骨架天生只有官方 bundle，所以不再需要「先复制 web 再裁剪」。
fn init_profile_from_template(
    settings: &Settings,
    name: &str,
    template: &str,
) -> Result<(), String> {
    let (node, bin_js) = resolve_dsh_bin(settings)?;
    let out = util::hidden_command(&node)
        .arg(&bin_js)
        .arg("--profile")
        .arg(name)
        .arg("--from-default-profile")
        .arg(template)
        .arg("--dump-default-config")
        .env(
            "DSH_HOME",
            profiles::dsh_native_home().to_string_lossy().into_owned(),
        )
        .output()
        .map_err(|e| format!("无法执行 dsh（{}）: {e}", bin_js.display()))?;
    if out.status.success() {
        crate::diag::info(
            "profile",
            &format!("dsh 模板初始化 profile 成功：{name} ← 模板 {template}"),
        );
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let err = err.trim();
    let msg = if err.is_empty() {
        format!("dsh 初始化 profile 失败（退出码 {:?}）", out.status.code())
    } else {
        format!("dsh 初始化 profile 失败：{err}")
    };
    crate::diag::warn("profile", &format!("{msg}：{name} ← 模板 {template}"));
    Err(msg)
}

/// 防呆：骨架必须就是「官方 base + web-app」。模板被本地改过（或 dsh 换了模板）时
/// 裁回官方两个，保证恢复模式永远只跑官方插件。骨架正常时一个字节都不动。
fn ensure_official_bundles(dir: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let bundles: Vec<String> = serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.pointer("/dsh/profile/bundles").and_then(|b| b.as_array()).cloned())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if bundles.iter().map(String::as_str).eq(OFFICIAL_BUNDLES.iter().copied()) {
        return Ok(());
    }
    prune_to_official_bundles(dir)
}

/// 给刚建出来的恢复模式写「快捷配置」：端口取官方 web 端口（没配就用 dsh 默认端口）
/// 之后的邻近空闲端口，其余项（openBrowser / surfaceContext / cookieMaxAgeDays）
/// 沿用官方 web 当前配置、缺项用默认值。写的是它自己的 cordis.patch.yml。
fn write_recovery_quick_config(profile: &str) -> Result<u16, String> {
    let seed = get_web_quick_config(RECOVERY_TEMPLATE).unwrap_or_default();
    let (host, base) = match web_addr(RECOVERY_TEMPLATE) {
        Some(v) => v,
        // 官方 web 还没配 webserver（甚至没建过）也要能生成恢复模式：退到 dsh 默认端口
        None => ("127.0.0.1".to_string(), DEFAULT_WEB_PORT),
    };
    let port = pick_free_nearby_port(&host, base)?;
    set_web_quick_config(
        profile,
        &WebQuickConfigInput {
            host,
            port: port as u64,
            open_browser: seed.open_browser.unwrap_or(true),
            surface_context: seed.surface_context.unwrap_or(true),
            cookie_max_age_days: seed.cookie_max_age_days.unwrap_or(36500),
        },
    )?;
    Ok(port)
}

/// 生成「恢复模式」profile：用 dsh 自己的 `--from-default-profile web` 从随附模板新建
/// `web-Recovery` —— 不复制用户当前的 web，因此天生只有官方 base + web-app；随后把端口
/// 等快捷配置写进它自己的 cordis.patch.yml。相当于「原版 web 换个端口运行」，
/// 用于排查第三方插件把 web 跑挂的情况。
///
/// 只在显式调用时创建，启动器**永远不会**自动创建它。
pub fn create_recovery_profile(settings: &Settings) -> Result<RecoveryCreated, String> {
    create_recovery_profile_with(settings, init_profile_from_template)
}

/// `create_recovery_profile` 的可测内核：`init` 注入，测试不必依赖真实 dsh 安装。
fn create_recovery_profile_with<F>(settings: &Settings, init: F) -> Result<RecoveryCreated, String>
where
    F: FnOnce(&Settings, &str, &str) -> Result<(), String>,
{
    let dir = recovery_dir();
    // existing_profile_path 覆盖单数 profile 目录的回退，dir 覆盖 dsh 真正会写的复数目录
    if dir.exists() || existing_profile_path(RECOVERY_PROFILE).is_some() {
        return Err(format!("恢复模式 profile「{RECOVERY_PROFILE}」已存在"));
    }
    // dsh 自己就不会覆盖已有 profile：失败时它要么没建目录，要么已自行清理
    init(settings, RECOVERY_PROFILE, RECOVERY_TEMPLATE)?;
    // 骨架就位后的任何一步失败都回滚，避免留下一个「建了一半」的 profile
    let built = (|| -> Result<u16, String> {
        if !dir.join("package.json").is_file() {
            return Err("dsh 没有生成 profile 骨架（package.json 缺失）".into());
        }
        ensure_official_bundles(&dir)?;
        write_recovery_quick_config(RECOVERY_PROFILE)
    })();
    match built {
        Ok(port) => {
            crate::diag::info(
                "profile",
                &format!("恢复模式 profile「{RECOVERY_PROFILE}」已创建（端口 {port}）"),
            );
            Ok(RecoveryCreated {
                name: RECOVERY_PROFILE.to_string(),
                port,
            })
        }
        Err(e) => {
            crate::diag::warn(
                "profile",
                &format!("恢复模式创建失败，已回滚骨架：{e}"),
            );
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

// ── 回收站（删除的 profile）─────────────────────────

/// 回收站条目：删除的 profile 只是被移动到这里，可还原或彻底删除
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedProfile {
    /// 回收站里的目录/文件名（还原、彻底删除用它定位）
    pub dir_name: String,
    /// 解析出的原 profile 名
    pub name: String,
    pub path: String,
    /// 删除时间（毫秒时间戳；从条目名解析，解析不到为 0）
    pub deleted_at: u64,
    pub is_dir: bool,
}

fn trash_dir() -> PathBuf {
    crate::settings::starter_home().join("deleted-profiles")
}

/// 回收站条目名是否合法（挡住路径穿越）
fn validate_trash_entry(dir_name: &str) -> Result<(), String> {
    if dir_name.trim().is_empty()
        || dir_name.contains('/')
        || dir_name.contains('\\')
        || dir_name.contains("..")
    {
        return Err("非法回收站条目".into());
    }
    Ok(())
}

/// 解析回收站条目名：`<name>-<毫秒时间戳>[.yaml|yml|json]` → (原名, 时间戳, 扩展名)
fn parse_trash_entry(dir_name: &str) -> (String, u64, Option<String>) {
    let (base, ext) = match dir_name.rsplit_once('.') {
        Some((b, e)) if matches!(e, "yaml" | "yml" | "json") => (b, Some(e.to_string())),
        _ => (dir_name, None),
    };
    match base.rsplit_once('-') {
        Some((name, ts))
            if !name.is_empty() && !ts.is_empty() && ts.chars().all(|c| c.is_ascii_digit()) =>
        {
            (name.to_string(), ts.parse().unwrap_or(0), ext)
        }
        _ => (base.to_string(), 0, ext),
    }
}

/// 从回收站条目名解析「原 profile 名」
fn parse_trash_name(dir_name: &str) -> String {
    parse_trash_entry(dir_name).0
}

/// 列出回收站里的 profile（按删除时间倒序）
pub fn list_deleted_profiles() -> Vec<DeletedProfile> {
    let dir = trash_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<DeletedProfile> = rd
        .flatten()
        .filter_map(|e| {
            let dir_name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let is_dir = path.is_dir();
            let (name, deleted_at, _ext) = parse_trash_entry(&dir_name);
            Some(DeletedProfile {
                name,
                dir_name,
                path: path.to_string_lossy().into_owned(),
                deleted_at,
                is_dir,
            })
        })
        .collect();
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    out
}

/// 把回收站条目还原回 profiles 目录；同名 profile 已存在时拒绝（避免覆盖）
pub fn restore_deleted_profile(dir_name: &str) -> Result<String, String> {
    validate_trash_entry(dir_name)?;
    let src = trash_dir().join(dir_name);
    if !src.exists() {
        return Err("回收站里找不到该条目".into());
    }
    let name = parse_trash_name(dir_name);
    validate_profile_name(&name)?;
    if existing_profile_path(&name).is_some() {
        return Err(format!(
            "已存在同名 profile「{name}」——请先改名或删除它，再还原"
        ));
    }
    let root = profiles::profiles_dir();
    let dst = if src.is_dir() {
        root.join(&name)
    } else {
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("yaml");
        root.join(format!("{name}.{ext}"))
    };
    move_path(&src, &dst)?;
    crate::diag::info(
        "profile",
        &format!("回收站条目「{dir_name}」已还原为 profile「{name}」（{}）", dst.display()),
    );
    Ok(dst.to_string_lossy().into_owned())
}

/// 从回收站彻底删除（不可恢复）
pub fn purge_deleted_profile(dir_name: &str) -> Result<(), String> {
    validate_trash_entry(dir_name)?;
    let path = trash_dir().join(dir_name);
    if !path.exists() {
        return Err("回收站里找不到该条目".into());
    }
    let r = if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    match r {
        Ok(()) => {
            crate::diag::warn("profile", &format!("回收站条目已彻底删除（不可恢复）：{dir_name}"));
            Ok(())
        }
        Err(e) => Err(format!("彻底删除失败: {e}")),
    }
}

// ── 插件更新检测 ─────────────────────────

/// 更新检测结果（一行对应 package.json 里的一个直接依赖）
#[derive(Clone, Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateInfo {
    pub name: String,
    pub source: String,
    /// package.json 里的原始规格（^1.2.3 / link:… / github:…）
    pub spec: String,
    pub has_update: bool,
    /// 是否完成了检测（link/file/workspace 等无渠道来源为 false）
    pub checked: bool,
    /// npm：已安装版本 / registry latest
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    /// git：仓库（owner/repo）、已装提交、远端最新提交
    pub repo: Option<String>,
    pub installed_commit: Option<String>,
    pub remote_commit: Option<String>,
    /// 可直接交给 `dsh plugin add` 的升级规格（npm=包名@latest；git=去 sha 的 github 规格）
    pub update_spec: Option<String>,
    /// 升级方式：add（重新走 dsh plugin add）/ git-pull（先 pull 再重新 link）/ None（无渠道）
    pub update_kind: Option<String>,
    /// 本地 link 目标路径（link / git-clone 源）
    pub local_path: Option<String>,
    /// git 工作树根目录绝对路径（git-clone 源，更新走 git pull）
    pub clone_dir: Option<String>,
    /// 该 git 工作树是否由启动器克隆（落在 ~/.dsh-starter/git-plugins 下）
    pub managed_clone: Option<bool>,
    /// 插件目录相对 git 根的路径（git-clone 源，重新 link 用）
    pub sub_path: Option<String>,
    /// git 工作树有未提交改动
    pub dirty: Option<bool>,
    /// link 目标目录里是否有构建产物（lib/*.js）；false = 升级时需要重新构建
    pub lib_ok: Option<bool>,
    /// 无渠道原因：no-channel（link/直链本就没有版本渠道）/ private-repo（私有仓库被拒绝）
    pub blocked: Option<String>,
    pub note: Option<String>,
}

/// package.json dependencies 的原始规格（按名字排序）
#[derive(Clone, Debug)]
pub struct DependencySpecInfo {
    pub name: String,
    pub spec: String,
    pub source: String,
}

pub fn dependency_specs(profile: &str) -> Result<Vec<DependencySpecInfo>, String> {
    let dir = profile_dir(profile)?;
    let raw = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|e| format!("读取 package.json 失败: {e}"))?;
    let pkg: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("package.json 解析失败: {e}"))?;
    let mut out = Vec::new();
    if let Some(deps) = pkg.get("dependencies").and_then(|v| v.as_object()) {
        for (name, ver) in deps {
            let spec = ver.as_str().unwrap_or_default().to_string();
            out.push(DependencySpecInfo {
                name: name.clone(),
                source: source_of(Some(&spec)).to_string(),
                spec,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 依赖的已安装版本：优先 profile 自带 node_modules，回退 profiles 根共享 node_modules
pub fn installed_version_of(profile: &str, name: &str) -> Option<String> {
    let dir = profile_dir(profile).ok()?;
    for base in [dir, profiles::profiles_dir()] {
        let raw = std::fs::read_to_string(base.join("node_modules").join(name).join("package.json"))
            .ok()?;
        let pkg: serde_json::Value = serde_json::from_str(&raw).ok()?;
        if let Some(v) = pkg.get("version").and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
    }
    None
}

pub fn read_pnpm_lock(profile: &str) -> Option<String> {
    std::fs::read_to_string(profile_dir(profile).ok()?.join("pnpm-lock.yaml")).ok()
}

/// 行内首个 40 位 hex 令牌（git 提交 SHA）
fn first_40hex(line: &str) -> Option<String> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
        .find(|t| t.len() == 40 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(String::from)
}

/// 在 pnpm-lock.yaml 文本中定位某 git 依赖已解析的提交哈希。
/// pnpm 各版本对 git 依赖的锁记录键式不一（packages: 段键内嵌 commit、resolution.commit 等），
/// 做宽容扫描：命中依赖名（或 owner/repo）的行向前 1 行 / 向后 5 行窗口内找 40-hex。
pub fn find_git_commit_in_lock(lock_text: &str, dep_name: &str, repo_hint: &str) -> Option<String> {
    let lines: Vec<&str> = lock_text.lines().collect();
    for i in 0..lines.len() {
        let hit = lines[i].contains(dep_name)
            || (!repo_hint.is_empty() && lines[i].contains(repo_hint));
        if !hit {
            continue;
        }
        let lo = i.saturating_sub(1);
        let hi = (i + 6).min(lines.len());
        for line in &lines[lo..hi] {
            if let Some(h) = first_40hex(line) {
                return Some(h);
            }
        }
    }
    None
}

/// 检测 profile 全部直接依赖是否有新版本：
/// - npm 源：已装版本（node_modules）vs registry `latest`，semver 比较；
/// - git 源（github:owner/repo）：已解析提交（规格内嵌 sha → pnpm-lock.yaml 扫描）vs 远端 HEAD；
/// - git-clone 源（克隆到本地工作树再 link）：本地 HEAD vs 远端 HEAD，用 git pull 更新；
/// - link/file/workspace/tarball：没有版本渠道，或私有仓库被拒绝（blocked 标明原因）。
/// git-clone 源的本地快照（一次 spawn_blocking 里全部采完，异步循环只碰网络）
#[derive(Default)]
struct CloneProbe {
    local_path: Option<String>,
    /// link 目标解析失败 / 不是 git 工作树，直接作为 note 上屏
    note: Option<String>,
    git_root: Option<PathBuf>,
    managed_clone: Option<bool>,
    sub_path: Option<String>,
    update_spec: Option<String>,
    dirty: Option<bool>,
    lib_ok: Option<bool>,
    installed_commit: Option<String>,
    branch: Option<String>,
    url: Option<String>,
}

#[derive(Default)]
struct SyncProbe {
    installed_version: Option<String>,
    lock_commit: Option<String>,
    clone: Option<CloneProbe>,
    link_local: Option<String>,
}

/// 本地侧探测：读 lockfile、扫 node_modules、逐条起 git 子进程
/// （status / rev-parse / config）。这些全是阻塞操作，只允许在
/// spawn_blocking 里被调用（AGENTS.md 硬性要求）。
fn probe_local(profile: &str, d: &DependencySpecInfo, lock: Option<&str>) -> SyncProbe {
    match d.source.as_str() {
        "npm" => SyncProbe {
            installed_version: installed_version_of(profile, &d.name),
            ..Default::default()
        },
        "git" => {
            let repo_hint = crate::registry::parse_github_spec(&d.spec)
                .map(|s| format!("{}/{}", s.owner, s.repo))
                .unwrap_or_default();
            SyncProbe {
                lock_commit: lock
                    .and_then(|lk| find_git_commit_in_lock(lk, &d.name, &repo_hint)),
                ..Default::default()
            }
        }
        "git-clone" => {
            let mut cp = CloneProbe::default();
            match crate::plugin::link_target(&d.spec) {
                None => cp.note = Some("无法解析 link 目标路径".into()),
                Some(local) => {
                    cp.local_path = Some(local.to_string_lossy().into_owned());
                    match find_git_root(&local) {
                        None => cp.note = Some("link 目标不是 git 工作树，跳过".into()),
                        Some(git_root) => {
                            cp.git_root = Some(git_root.clone());
                            cp.managed_clone = Some(crate::plugin::in_git_plugins(&git_root));
                            cp.sub_path = local
                                .strip_prefix(&git_root)
                                .ok()
                                .map(|p| p.to_string_lossy().replace('\\', "/"))
                                .filter(|s| !s.is_empty());
                            cp.update_spec =
                                Some(format!("link:{}", local.to_string_lossy()));
                            cp.dirty = crate::plugin::git_output(
                                &git_root,
                                &["status", "--porcelain"],
                            )
                            .map(|s| !s.is_empty());
                            // 本地工作树里没有构建产物 → 升级时要重新 install+build
                            cp.lib_ok = Some(crate::registry::local_lib_ok(&local));
                            cp.installed_commit =
                                crate::plugin::git_output(&git_root, &["rev-parse", "HEAD"]);
                            cp.branch = crate::plugin::git_output(
                                &git_root,
                                &["rev-parse", "--abbrev-ref", "HEAD"],
                            );
                            cp.url = crate::plugin::git_output(
                                &git_root,
                                &["config", "--get", "remote.origin.url"],
                            );
                        }
                    }
                }
            }
            SyncProbe {
                clone: Some(cp),
                ..Default::default()
            }
        }
        "link" | "file" | "workspace" => SyncProbe {
            link_local: crate::plugin::link_target(&d.spec)
                .map(|p| p.to_string_lossy().into_owned()),
            ..Default::default()
        },
        _ => SyncProbe::default(),
    }
}

pub async fn check_plugin_updates(
    registry_base: &str,
    profile: &str,
    github_token: Option<&str>,
) -> Result<Vec<PluginUpdateInfo>, String> {
    // 本地部分（读 package.json、扫几 MB 的 lockfile、每个依赖起 git 子进程）
    // 收拢进一次 spawn_blocking：这是 async 命令，同步 git 会把整条 runtime
    // 的异步执行器线程挂住，后面所有 async 命令排队等它。
    let prof = profile.to_string();
    let (deps, probes) = tauri::async_runtime::spawn_blocking(move || {
        let deps = dependency_specs(&prof)?;
        let lock = read_pnpm_lock(&prof);
        let prof2 = prof.clone();
        let probes: Vec<SyncProbe> = deps
            .iter()
            .map(|d| probe_local(&prof2, d, lock.as_deref()))
            .collect();
        Ok::<(Vec<DependencySpecInfo>, Vec<SyncProbe>), String>((deps, probes))
    })
    .await
    .map_err(|e| format!("本地检测任务异常（内部线程崩溃）: {e}"))??;

    let mut out = Vec::new();
    for (d, probe) in deps.into_iter().zip(probes) {
        let mut info = PluginUpdateInfo {
            name: d.name.clone(),
            source: d.source.clone(),
            spec: d.spec.clone(),
            ..Default::default()
        };
        match d.source.as_str() {
            "npm" => {
                info.installed_version = probe.installed_version;
                info.update_spec = Some(format!("{}@latest", d.name));
                info.update_kind = Some("add".into());
                match crate::registry::npm_latest_version(registry_base, &d.name).await {
                    Ok(latest) => {
                        info.latest_version = latest.clone();
                        info.checked = true;
                        if let (Some(inst), Some(lat)) = (&info.installed_version, &latest) {
                            info.has_update =
                                crate::semver::compare(lat, inst) == std::cmp::Ordering::Greater;
                        }
                        if info.installed_version.is_none() {
                            info.note = Some("未找到已安装版本（尚未安装成功）".into());
                        }
                    }
                    Err(e) => info.note = Some(e),
                }
            }
            "git" => {
                if let Some(spec) = crate::registry::parse_github_spec(&d.spec) {
                    let repo_full = format!("{}/{}", spec.owner, spec.repo);
                    info.repo = Some(repo_full.clone());
                    // 已装提交：规格内嵌 sha（pnpm 会把解析出的 sha 写回规格）→ lockfile 扫描
                    if let Some(r) = &spec.git_ref {
                        if (7..=40).contains(&r.len())
                            && r.chars().all(|c| c.is_ascii_hexdigit())
                        {
                            info.installed_commit = Some(r.clone());
                        }
                    }
                    if info.installed_commit.is_none() {
                        info.installed_commit = probe.lock_commit;
                    }
                    match crate::registry::github_head_commit(
                        &spec.owner,
                        &spec.repo,
                        spec.git_ref.as_deref(),
                        github_token,
                    )
                    .await
                    {
                        // 404：私有仓库或不存在（GitHub API 对二者都返回 404）
                        Ok(None) => {
                            info.blocked = Some("private-repo".into());
                            info.note = Some(format!(
                                "{}/{} 不存在或为私有仓库：暂不支持私有仓库的更新检测；请在本地手动 clone + link 安装",
                                spec.owner, spec.repo
                            ));
                        }
                        Ok(remote) => {
                            info.remote_commit = remote.clone();
                            info.checked = true;
                            match (&info.installed_commit, &remote) {
                                (Some(a), Some(b)) => {
                                    info.has_update = !b.to_ascii_lowercase()
                                        .starts_with(&a.to_ascii_lowercase());
                                }
                                (None, Some(_)) => {
                                    info.note =
                                        Some("无法从 lockfile 确定已装提交，跳过比对".into());
                                }
                                _ => {}
                            }
                        }
                        Err(e) => info.note = Some(e),
                    }
                    info.update_spec = crate::registry::github_upgrade_spec(&d.spec);
                    info.update_kind = Some("add".into());
                } else {
                    info.note = Some("非 GitHub 规格（通用 git 链接暂不支持检测）".into());
                }
            }
            "git-clone" => {
                // 克隆到本地工作树再 link：本地 HEAD vs 远端 HEAD，更新方式 git pull。
                // 本地字段已在 probe_local（spawn_blocking 内）采完，这里只回填 + 网络探测。
                let Some(cp) = probe.clone else {
                    info.note = Some("本地工作树信息探测失败".into());
                    out.push(info);
                    continue;
                };
                info.local_path = cp.local_path;
                if cp.git_root.is_none() {
                    info.note = cp.note;
                } else {
                    info.clone_dir = cp.git_root.map(|p| p.to_string_lossy().into_owned());
                    info.managed_clone = cp.managed_clone;
                    info.sub_path = cp.sub_path;
                    info.update_spec = cp.update_spec;
                    info.update_kind = Some("git-pull".into());
                    info.dirty = cp.dirty;
                    info.lib_ok = cp.lib_ok;
                    info.installed_commit = cp.installed_commit.clone();
                    if let Some(u) = &cp.url {
                        info.repo = Some(github_repo_of(u));
                    }
                    match (&cp.url, &cp.branch) {
                        (Some(u), b) => {
                            // 远端最新提交：GitHub 走未认证 API（快、不受 git 网络挂起影响），
                            // 其它主机走非交互 git ls-remote（缺凭据立即失败，不弹登录）
                            // —— ls-remote 是子进程 + 网络，异步线程里必须 spawn_blocking。
                            let github = crate::registry::parse_github_spec(u)
                                .filter(|sp| sp.tarball_url.is_none() && !sp.owner.is_empty());
                            let remote: Result<Option<String>, String> = match &github {
                                Some(sp) => {
                                    crate::registry::github_head_commit(
                                        &sp.owner,
                                        &sp.repo,
                                        b.as_deref(),
                                        github_token,
                                    )
                                    .await
                                }
                                None => {
                                    let (uu, bb) = (u.clone(), b.clone());
                                    tauri::async_runtime::spawn_blocking(move || {
                                        crate::plugin::git_remote_head(&uu, bb.as_deref())
                                            .map(Some)
                                    })
                                    .await
                                    .map_err(|e| format!("远端探测任务异常（内部线程崩溃）: {e}"))?
                                }
                            };
                            match remote {
                                Ok(Some(r)) => {
                                    info.remote_commit = Some(r.clone());
                                    info.checked = true;
                                    match &info.installed_commit {
                                        Some(a) => {
                                            info.has_update = !r
                                                .to_ascii_lowercase()
                                                .starts_with(&a.to_ascii_lowercase())
                                        }
                                        None => {
                                            info.note = Some("本地仓库没有提交记录".into())
                                        }
                                    }
                                }
                                // GitHub API 404：私有仓库或仓库不存在，一律拒绝检测
                                Ok(None) => {
                                    info.blocked = Some("private-repo".into());
                                    info.note = Some(format!(
                                        "远端不可匿名访问（私有仓库或不存在）：暂不支持私有仓库的更新检测；\
                                         请在本地仓库手动 git pull 后重启实例（{}）",
                                        u
                                    ));
                                }
                                Err(e) => {
                                    let line =
                                        e.lines().next().unwrap_or("").trim().to_string();
                                    if crate::plugin::looks_like_auth_error(&e) {
                                        info.blocked = Some("private-repo".into());
                                        info.note = Some(format!(
                                            "{}（{}）",
                                            crate::plugin::PRIVATE_REPO_REJECT, line
                                        ));
                                    } else if crate::plugin::looks_like_missing_repo(&e) {
                                        info.blocked = Some("private-repo".into());
                                        info.note = Some(format!(
                                            "远端不可匿名访问（私有仓库或不存在）：暂不支持私有仓库的更新检测；\
                                             请在本地仓库手动 git pull 后重启实例（{}）",
                                            line
                                        ));
                                    } else {
                                        info.note = Some(format!("无法读取远端：{line}"));
                                    }
                                }
                            }
                        }
                        (None, _) => info.note = Some("本地仓库缺少 origin 远端".into()),
                    }
                }
            }
            "tarball" => {
                info.note = Some("打包产物直链没有版本渠道：需要重新安装同一链接（手动）".into());
                info.blocked = Some("no-channel".into());
                info.update_spec = Some(d.spec.clone());
            }
            "link" | "file" | "workspace" => {
                info.local_path = probe.link_local;
                info.note = Some("本地源（link/file/workspace）不检测更新，改代码即时生效".into());
                info.blocked = Some("no-channel".into());
                info.update_spec = Some(d.spec.clone());
            }
            _ => {
                info.note = Some("未知来源，跳过检测".into());
            }
        }
        out.push(info);
    }
    Ok(out)
}

/// 从 git remote url 里提取 owner/repo（非 GitHub 返回原 url）
fn github_repo_of(url: &str) -> String {
    if let Some(spec) = crate::registry::parse_github_spec(url) {
        if !spec.owner.is_empty() && !spec.repo.is_empty() {
            return format!("{}/{}", spec.owner, spec.repo);
        }
    }
    url.to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    use crate::util::DSH_ENV_LOCK;

    /// 回归：`link:` 指向一个**远端是私有仓库**的本地 clone 时，
    /// 更新检测必须直接给出「私有仓库不支持」，既不能挂起也不能弹登录。
    #[test]
    fn private_clone_source_is_rejected() {
        let _net = crate::util::NET_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-upd-priv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let clone = tmp.join("clone");
        std::fs::create_dir_all(&clone).unwrap();

        // 造一个本地 git 仓库，远端指向一个私有 GitHub 仓库（git ls-remote 会要凭据、API 会 404）
        let git = |args: &[&str]| {
            let ok = util::hidden_command("git")
                .args(args)
                .current_dir(&clone)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} 失败");
        };
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", "https://github.com/Yinxe/dsh-qqbot.git"]);
        std::fs::write(
            clone.join("package.json"),
            r#"{"name":"@dshp-inx/qqbot","version":"0.0.1","dsh":{"bundle":{"patch":"./cordis.patch.yml"}}}"#,
        )
        .unwrap();
        std::fs::write(clone.join("cordis.patch.yml"), "- id: qqbot\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-qm", "init"]);

        let dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("DSH_HOME", &tmp);
        std::fs::write(
            dir.join("package.json"),
            format!(
                r#"{{"name":"dsh-profile-web","dependencies":{{"@dshp-inx/qqbot":"link:{}"}},"dsh":{{"profile":{{"bundles":["@dshp-inx/qqbot"]}}}}}}"#,
                clone.to_string_lossy()
            ),
        )
        .unwrap();

        let list = tauri::async_runtime::block_on(check_plugin_updates(
            "https://registry.npmjs.org",
            "web",
            None,
        ))
        .unwrap();
        // 全量跑时前面的网络用例可能已把匿名额度耗光：这不是分类逻辑坏了
        if crate::registry::rate_limit().exhausted {
            eprintln!("跳过：GitHub API 额度已用尽");
            std::env::remove_var("DSH_HOME");
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }
        let q = list.iter().find(|u| u.name == "@dshp-inx/qqbot").expect("应有该依赖");
        assert_eq!(q.source, "git-clone");
        let note = q.note.clone().unwrap_or_default();
        // API 配额耗尽/网络不可达时跳过：这不代表分类逻辑坏了
        if crate::plugin::api_unavailable(&note) {
            eprintln!("跳过：GitHub API 不可用/被限流（{note}）");
            std::env::remove_var("DSH_HOME");
            let _ = std::fs::remove_dir_all(&tmp);
            return;
        }
        assert_eq!(q.blocked.as_deref(), Some("private-repo"), "note={note}");
        assert!(!q.checked, "私有仓库不该被当成已检测");
        assert!(note.contains("私有仓库"), "note={note}");
        assert!(!note.contains("Username"), "不能把登录提示透出去：{note}");
        // 给出可执行的下一步：本地手动 git pull
        assert!(note.contains("git pull"), "note={note}");

        std::env::remove_var("DSH_HOME");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 宿主自带的插件包受保护：不允许卸载、不允许停用
    #[test]
    fn inbox_bundles_are_protected() {
        for name in INBOX_BUNDLES {
            assert!(is_inbox_bundle(name), "{name} 应受保护");
        }
        // 前缀不是宽泛匹配：第三方 @deepseek-ai/dsh-* 与社区包都不在名单里
        assert!(!is_inbox_bundle("@deepseek-ai/dsh-mcp-client"));
        assert!(!is_inbox_bundle("@dshp/mcwiki-search"));
        assert!(!is_inbox_bundle(""));
        let msg = inbox_bundle_reject("@deepseek-ai/dsh-base", "卸载");
        assert!(msg.contains("不能卸载"), "{msg}");
        assert!(msg.contains("恢复模式"), "{msg}");

        // 停用请求在读文件之前就被拒绝（profile 不存在也不该变成"读取失败"）
        let settings = crate::settings::Settings::default();
        let err = set_bundle_enabled(&settings, "web-not-exist", "@deepseek-ai/dsh-base", false)
            .unwrap_err();
        assert!(err.contains("不能停用"), "{err}");
    }

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
        assert_eq!(raw.matches("dsh-starter: disable").count(), 2);

        // 启用：该包管理块整体移除，用户内容保留
        set_ids_disabled("web", bundle, &ids, false).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# my notes"));
        assert!(raw.contains("- id: webserver"));
        assert!(!raw.contains("dsh-starter: disable"));
        assert!(!raw.contains("disabled: true"));
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        // 再次启用应报“本就启用”
        assert!(set_ids_disabled("web", bundle, &ids, false).is_err());

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// Windows 用户实测报错：全新 profile 的 cordis.patch.yml 是「一串注释 + 一行 `[]`」
    /// （模板默认内容），此时关掉任意插件开关直接失败 ——「YAML 语法错误：deserializing from
    /// YAML containing more than one document is not supported」。
    /// 原因：`[]` 是 flow 序列，后面再接 block 条目就变成一个文件两个 YAML 文档。
    #[test]
    fn bundle_toggle_works_on_template_empty_flow_placeholder() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-empty-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        let patch = prof_dir.join("cordis.patch.yml");
        std::fs::write(
            &patch,
            "# Your patch layer for this dsh profile, applied after every bundle layer:\n\
             # a top-level YAML array of loader patch entries (id-targeted config\n\
             # overrides, disables, and insert lists; `!!js` expressions allowed).\n\
             []\n",
        )
        .unwrap();

        let ids = vec!["dshp-demo".to_string()];
        // 关掉开关：占位行必须让位给 block 条目，否则整个操作报 YAML 多文档错误
        set_ids_disabled("web", "@dshp/demo", &ids, true).unwrap();
        let raw = std::fs::read_to_string(&patch).unwrap();
        assert!(raw.contains("# Your patch layer"), "用户注释必须保留:\n{raw}");
        assert!(raw.contains("- id: dshp-demo"));
        assert!(!raw.contains("[]"), "空 flow 占位必须被摘掉:\n{raw}");
        serde_yaml::from_str::<serde_yaml::Value>(&raw).expect("必须是单文档合法 YAML");
        assert!(patch_disabled_ids("web").contains("dshp-demo"));

        // 再打开：条目移除后不能只剩注释 —— patch 文件的契约是顶层数组，
        // 空文档是 null，得把模板那份 `[]` 占位放回去
        set_ids_disabled("web", "@dshp/demo", &ids, false).unwrap();
        let raw = std::fs::read_to_string(&patch).unwrap();
        assert!(raw.contains("# Your patch layer"), "用户注释必须保留:\n{raw}");
        assert!(!raw.contains("dshp-demo"));
        assert_eq!(
            serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap(),
            serde_yaml::Value::Sequence(vec![]),
            "空补丁层应回到 `[]`，不是 null 文档:\n{raw}"
        );

        // 往返一致：再来一轮仍然可用（幂等 + 不残留坏写法）
        set_ids_disabled("web", "@dshp/demo", &ids, true).unwrap();
        set_ids_disabled("web", "@dshp/demo", &ids, false).unwrap();
        let raw = std::fs::read_to_string(&patch).unwrap();
        assert_eq!(raw.matches("[]").count(), 1, "占位只应有一行:\n{raw}");

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 同一个 profile 上「插件启停」与「web 快捷配置」会轮流改写**同一份** patch 文件：
    /// 两条路径对空 flow 占位的处理必须一致，否则先写的那步就把文件弄成多文档，后面全崩。
    /// 顺带覆盖带空格的 `[ ]`，以及写坏文件时给的是可操作提示而不是解析器原文。
    #[test]
    fn plugin_toggle_and_web_quick_share_one_patch_file() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-patch-mix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("DSH_HOME", &tmp);
        let prof_dir = tmp.join("profiles/web");
        std::fs::create_dir_all(&prof_dir).unwrap();
        let patch = prof_dir.join("cordis.patch.yml");
        // 带空格的空 flow 序列同样是合法的空补丁层
        std::fs::write(&patch, "# header\n[ ]\n").unwrap();

        let ids = vec!["dshp-demo".to_string()];
        let read = || std::fs::read_to_string(&patch).unwrap();
        let assert_single_doc = |raw: &str| {
            serde_yaml::from_str::<serde_yaml::Value>(raw)
                .unwrap_or_else(|e| panic!("必须是单文档合法 YAML：{e}\n{raw}"));
        };

        let input = WebQuickConfigInput {
            host: "0.0.0.0".into(),
            port: 3080,
            open_browser: false,
            surface_context: true,
            cookie_max_age_days: 30,
        };

        // 1) 先写 web 快捷配置：占位让位给 block 条目
        set_web_quick_config("web", &input).unwrap();
        assert_single_doc(&read());
        assert!(read().contains("host: '0.0.0.0'"));
        assert!(!read().contains("[ ]"), "占位应被移除:\n{}", read());

        // 2) 紧接着关插件开关：文件里已经有 block 条目，追加必须仍然只产生一个文档
        set_ids_disabled("web", "@dshp/demo", &ids, true).unwrap();
        assert_single_doc(&read());
        assert!(patch_disabled_ids("web").contains("dshp-demo"));

        // 3) 打开开关：只摘管理块，web 配置与注释都留着
        set_ids_disabled("web", "@dshp/demo", &ids, false).unwrap();
        let raw = read();
        assert_single_doc(&raw);
        assert!(raw.contains("# header") && raw.contains("host: '0.0.0.0'"));
        assert!(!raw.contains("dshp-demo"));

        // 4) 快捷配置幂等替换：同一份文件再写一遍也不出错
        set_web_quick_config("web", &input).unwrap();
        assert_single_doc(&read());
        assert_eq!(read().matches("id: webserver").count(), 1, "同 id 条目应只保留一份");

        // 5) 万一用户手写坏文件，报错要直接告诉他怎么改
        let err = validate_yaml("[]\n- id: x\n  disabled: true\n").unwrap_err();
        assert!(err.contains("多个 YAML 文档"), "{err}");
        assert!(err.contains("删掉那行"), "报错要给出下一步动作：{err}");

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
        std::fs::write(tmp.join("cordis.patch.starter-bak-1700000000"), "old").unwrap();

        backup(&f).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join("cordis.patch.starter-bak")).unwrap(),
            "v1"
        );

        std::fs::write(&f, "v2").unwrap();
        backup(&f).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join("cordis.patch.starter-bak")).unwrap(),
            "v2"
        );

        let baks: Vec<String> = std::fs::read_dir(&tmp)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("starter-bak"))
            .collect();
        assert_eq!(baks, vec!["cordis.patch.starter-bak".to_string()]);

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 回归：凭据备份必须继承源文件的 0600 —— 备份同样装着明文 token，
    /// 按默认 umask 落成 0644 就等于把全套凭据对同组/其他人敞开。
    #[cfg(unix)]
    #[test]
    fn backup_inherits_source_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = std::env::temp_dir().join(format!("dsh-bak-mode-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("credentials.yaml");
        std::fs::write(&f, "refs:\n  A: secret\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();

        backup(&f).unwrap();
        let bak = tmp.join("credentials.starter-bak");
        let mode = std::fs::metadata(&bak).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "备份权限应跟随源文件，实际 {mode:#o}");

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 语法错误的报错必须带「第 X 行第 Y 列」，让用户能直接对到编辑器行号
    #[test]
    fn validate_yaml_errors_are_located() {
        assert!(validate_yaml("a: 1\nb:\n  c: [1, 2]\n").is_ok());
        let err = validate_yaml("a: 1\n  b: broken\n").unwrap_err();
        assert!(err.contains('行') && err.contains('列'), "报错应带行列定位：{err}");
        // 多文档提示同样不能丢定位
        let err = validate_yaml("[]\n- id: x\n").unwrap_err();
        assert!(err.contains("多个 YAML 文档"), "{err}");
        assert!(err.contains('行'), "多文档报错也要带行号：{err}");
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
             # dsh-starter: disable x/y（启动器管理：关闭开关会自动移除此块）\n\
             - id: other\n  disabled: true\n",
        )
        .unwrap();

        let input = WebQuickConfigInput {
            host: "127.0.0.1".into(),
            port: 3081,
            open_browser: true,
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
        // printUrl 不开放配置：启动器靠日志识别 URL，恒为 true
        assert!(raw.contains("printUrl: true"));
        assert!(raw.contains("surfaceContext: true"));
        assert!(raw.contains("trustedHosts: !!js ctx.webStartup.trustedHosts"));
        assert!(raw.contains("trustedHosts: !!js ctx.webRuntime.trustedHosts"));
        assert!(raw.contains("cookieMaxAgeDays: 36500"));
        // 无关的禁用管理块保留，且新条目都在它之前（否则 disabled 会被覆盖回启用）
        assert!(raw.contains("- id: other") && raw.contains("disabled: true"));
        let disable_pos = raw.find("# dsh-starter: disable").unwrap();
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
        assert_eq!(cfg.print_url, Some(true));
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
            surface_context: true,
            cookie_max_age_days: 30,
        };
        set_web_quick_config("web", &input).unwrap();
        let raw = std::fs::read_to_string(prof_dir.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("# header comment"));
        assert!(!raw.contains("[]"), "`[]` 占位应被移除:\n{raw}");
        assert!(raw.contains("host: '0.0.0.0'"));
        assert!(raw.contains("printUrl: true"));
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
        // 真实 patch 的 disabled 块也可能是手写的（不含启动器 marker），此时没有位置可比
        if let Some(disable_pos) = raw.find(MANAGE_MARKER) {
            for id in ["webserver", "web-runtime", "connection"] {
                assert!(raw.find(&format!("- id: {id}")).unwrap() < disable_pos);
            }
        }
        // 回读与输入一致
        let after = get_web_quick_config("web").unwrap();
        assert_eq!(after.host.as_deref(), Some(input.host.as_str()));
        assert_eq!(after.port, Some(input.port));
        assert_eq!(after.cookie_max_age_days, Some(input.cookie_max_age_days));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn find_git_commit_in_lock_scans_windows() {
        // v9：packages 段键内嵌 commit（键行命中，同行取 40-hex）
        let lock = "packages:\n\n  github:owner/repo/1234567890abcdef1234567890abcdef12345678:\n    name: my-plugin\n    version: 1.0.0\n";
        assert_eq!(
            find_git_commit_in_lock(lock, "my-plugin", "owner/repo").as_deref(),
            Some("1234567890abcdef1234567890abcdef12345678")
        );
        // resolution.commit 风格：命中名行，commit 在其后 5 行窗口内
        let lock2 = "gitPackages:\n  my-plugin@github:owner/repo#dev:\n    resolution:\n      type: git\n      repo: github:owner/repo\n      commit: abcdef1234567890abcdef1234567890abcdef12\n";
        assert_eq!(
            find_git_commit_in_lock(lock2, "my-plugin", "owner/repo").as_deref(),
            Some("abcdef1234567890abcdef1234567890abcdef12")
        );
        // 无仓库提示也无名字时才不命中（owner/repo 命中属预期：锁键以 repo 定位）
        assert_eq!(find_git_commit_in_lock(lock, "other-pkg", ""), None);
        // 空内容
        assert_eq!(find_git_commit_in_lock("", "my-plugin", "owner/repo"), None);
    }

    #[test]
    fn copy_profile_copies_config_skips_runtime() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-copy-test-{}", std::process::id()));
        std::env::set_var("DSH_HOME", &tmp);
        let prof = tmp.join("profiles/web");
        std::fs::create_dir_all(prof.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(prof.join("cache")).unwrap();
        std::fs::create_dir_all(prof.join("sub/dir")).unwrap();
        for f in ["package.json", "cordis.yml", "cordis.patch.yml", "pnpm-workspace.yaml"] {
            std::fs::write(prof.join(f), "x").unwrap();
        }
        std::fs::write(prof.join("node_modules/x/junk.txt"), "junk").unwrap();
        std::fs::write(prof.join("cache/cat.json"), "{}").unwrap();
        std::fs::write(prof.join("sub/dir/extra.txt"), "extra").unwrap();
        // 同名文件型 profile，用于撞名校验
        std::fs::write(tmp.join("profiles/foo.yaml"), "[]").unwrap();

        // 正常复制：配置与子目录保留，node_modules / cache 跳过
        copy_profile("web", "web-copy").unwrap();
        let dst = tmp.join("profiles/web-copy");
        for f in ["package.json", "cordis.yml", "cordis.patch.yml", "pnpm-workspace.yaml"] {
            assert!(dst.join(f).is_file(), "{f} 应被复制");
        }
        assert!(dst.join("sub/dir/extra.txt").is_file(), "用户子目录应被复制");
        assert!(!dst.join("node_modules").exists(), "node_modules 不应复制");
        assert!(!dst.join("cache").exists(), "cache 不应复制");
        // 源目录不受影响
        assert!(prof.join("node_modules/x/junk.txt").is_file());

        // 名称修剪空格
        copy_profile("web", " web2 ").unwrap();
        assert!(tmp.join("profiles/web2/package.json").is_file());

        // 目标已存在（目录/文件型撞名）均拒绝
        assert!(copy_profile("web", "web-copy").is_err());
        assert!(copy_profile("web", "foo").is_err());
        // 非法名
        for bad in ["", "..", "a/b", "a\\b", ".hid", "node_modules"] {
            assert!(copy_profile("web", bad).is_err(), "bad={bad}");
        }
        // 源不存在
        assert!(copy_profile("nope", "x").is_err());

        // 文件型 profile：拷为「新名.同扩展名」
        copy_profile("foo", "bar").unwrap();
        assert!(tmp.join("profiles/bar.yaml").is_file());
        assert!(!tmp.join("profiles/bar.yml").exists());

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 改名 / 删除 / 恢复模式：dsh 内置保留 profile 必须全部拒绝，
    /// 普通 profile 可改名、删除进回收站；恢复模式用 dsh 的 `--from-default-profile web`
    /// 新建（只有官方插件）并换邻近空闲端口。
    #[test]
    fn rename_delete_and_recovery_profile() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-rd-test-{}", std::process::id()));
        let home = tmp.join("starter-home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("DSH_HOME", &tmp);
        std::env::set_var("DSH_STARTER_HOME", &home);

        let profiles = tmp.join("profiles");
        let web = profiles.join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(
            web.join("package.json"),
            r#"{"dependencies":{"third-party":"1.0.0"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","third-party"]}},"name":"dsh-profile-web"}"#,
        )
        .unwrap();
        std::fs::write(
            web.join("cordis.patch.yml"),
            "- id: webserver\n  config:\n    host: '0.0.0.0'\n    port: 3080\n",
        )
        .unwrap();
        std::fs::create_dir_all(profiles.join("mine/sub")).unwrap();

        // 1) 内置保留 profile：改名/删除一律拒绝，也不许改成保留名
        for reserved in ["headless", "web", "desktop"] {
            assert!(
                crate::profiles::is_reserved_profile(reserved),
                "{reserved} 应视为保留"
            );
            assert!(rename_profile(reserved, "whatever").is_err(), "{reserved} 不应可改名");
            assert!(delete_profile(reserved).is_err(), "{reserved} 不应可删除");
        }
        assert!(crate::profiles::is_reserved_profile("WEB"), "保留名判断应忽略大小写");
        assert!(!crate::profiles::is_reserved_profile("web-Recovery"));
        assert!(rename_profile("mine", "web").is_err(), "不许改名成保留名");
        assert!(rename_profile("mine", "Desktop").is_err());

        // 2) 普通 profile：改名保留内容；删除是移入回收目录而不是销毁
        rename_profile("mine", "mine2").unwrap();
        assert!(profiles.join("mine2/sub").is_dir(), "改名应保留原内容");
        assert!(!profiles.join("mine").exists());
        let trash = delete_profile("mine2").unwrap();
        assert!(!profiles.join("mine2").exists());
        assert!(std::path::Path::new(&trash).exists(), "删除应移入回收目录");

        // 3) 恢复模式：走 dsh 的 `--from-default-profile web`（这里注入 stub 代替真实 dsh），
        //    骨架天生只有官方两个插件，随后写入端口等快捷配置
        let calls = std::cell::RefCell::new(Vec::new());
        let rec = create_recovery_profile_with(&crate::settings::Settings::default(), |_s, name, template| {
            calls.borrow_mut().push(format!("{name}<-{template}"));
            let dir = profiles.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("package.json"),
                r#"{"name":"dsh-profile-web-Recovery","private":true,"dependencies":{},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"],"patchReload":"live"}}}"#,
            )
            .unwrap();
            std::fs::write(dir.join("cordis.patch.yml"), "# 模板自带的空补丁层\n[]\n").unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            calls.borrow().as_slice(),
            ["web-Recovery<-web"],
            "恢复模式必须用 dsh 的 --from-default-profile web 建骨架"
        );
        assert_eq!(rec.name, RECOVERY_PROFILE);
        assert!(
            rec.port > 3080 && rec.port <= 3080 + 200,
            "应是 3080 之后的邻近空闲端口，实际 {}",
            rec.port
        );
        let rec_dir = profiles.join(RECOVERY_PROFILE);
        let pkg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(rec_dir.join("package.json")).unwrap())
                .unwrap();
        assert_eq!(
            pkg.pointer("/dsh/profile/bundles").unwrap().as_array().unwrap(),
            &vec![
                serde_json::Value::String("@deepseek-ai/dsh-base".into()),
                serde_json::Value::String("@deepseek-ai/dsh-web-app".into()),
            ],
            "恢复模式只应保留官方 base + web-app"
        );
        assert_eq!(
            get_web_quick_config(RECOVERY_PROFILE).unwrap().port,
            Some(rec.port as u64),
            "新 profile 的 webserver 端口应指向新端口"
        );
        // 官方 web 本体不受影响（恢复模式不再读它、更不会改它）
        assert_eq!(web_addr("web").unwrap().1, 3080);
        assert!(std::fs::read_to_string(web.join("package.json"))
            .unwrap()
            .contains("third-party"));
        // 已存在时二次创建应拒绝
        assert!(create_recovery_profile_with(&crate::settings::Settings::default(), |_, _, _| {
            panic!("已存在时不得再调用 dsh 初始化")
        })
        .is_err());

        // 4) 复制实例：web 类副本必须自动错开端口，否则两个实例抢同一个端口
        copy_profile("web", "web-copy").unwrap();
        let copied = assign_free_web_port("web-copy")
            .unwrap()
            .expect("web 类副本应有 webserver 端口");
        assert!(copied > 3080, "副本端口应错开原端口，实际 {copied}");
        assert_eq!(
            get_web_quick_config("web-copy").unwrap().port,
            Some(copied as u64)
        );
        // 没有 webserver 配置的 profile 无需处理
        std::fs::create_dir_all(profiles.join("plain")).unwrap();
        assert_eq!(assign_free_web_port("plain").unwrap(), None);

        // 5) 回收站：列出 → 还原 → 再删 → 彻底删除
        std::fs::create_dir_all(profiles.join("trashed")).unwrap();
        delete_profile("trashed").unwrap();
        let items = list_deleted_profiles();
        let hit = items
            .iter()
            .find(|d| d.name == "trashed")
            .expect("回收站应列出刚删除的 profile");
        assert!(hit.deleted_at > 0, "应从条目名解析出删除时间戳");
        assert_eq!(hit.is_dir, true);
        restore_deleted_profile(&hit.dir_name).unwrap();
        assert!(profiles.join("trashed").is_dir(), "还原后应回到 profiles 目录");
        let trash = delete_profile("trashed").unwrap();
        let entry = std::path::Path::new(&trash)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(list_deleted_profiles().iter().any(|d| d.dir_name == entry));
        purge_deleted_profile(&entry).unwrap();
        assert!(!std::path::Path::new(&trash).exists(), "彻底删除后不应还留在回收站");
        // 还原目标已存在时必须拒绝，避免覆盖
        std::fs::create_dir_all(profiles.join("dup")).unwrap();
        delete_profile("dup").unwrap();
        let dup = list_deleted_profiles()
            .into_iter()
            .find(|d| d.name == "dup")
            .unwrap();
        std::fs::create_dir_all(profiles.join("dup")).unwrap();
        assert!(restore_deleted_profile(&dup.dir_name).is_err());
        assert!(purge_deleted_profile("../evil").is_err(), "回收站条目名要挡路径穿越");

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
        std::env::remove_var("DSH_STARTER_HOME");
    }

    /// 恢复模式的另外两种情况：本地没有任何 web profile 时也要能建（端口退到 dsh 默认端口）；
    /// 模板被改出第三方 bundle 时裁回官方两个；dsh 建骨架失败时回滚干净。
    #[test]
    fn recovery_profile_falls_back_and_prunes() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = std::env::temp_dir().join(format!("dsh-rec-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let home = tmp.join("starter-home");
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("DSH_HOME", &tmp);
        std::env::set_var("DSH_STARTER_HOME", &home);
        let profiles = tmp.join("profiles");

        // 1) 完全没有官方 web profile：端口基准退到 3080，用 dsh 默认值补齐快捷配置
        let rec = create_recovery_profile_with(&crate::settings::Settings::default(), |_s, name, _t| {
            let dir = profiles.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            // 模拟「模板漂移」：多出一个第三方 bundle + 一条依赖
            std::fs::write(
                dir.join("package.json"),
                r#"{"dependencies":{"third-party":"1.0.0"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","third-party"]}}}"#,
            )
            .unwrap();
            std::fs::write(dir.join("cordis.patch.yml"), "[]\n").unwrap();
            Ok(())
        })
        .unwrap();
        assert!(
            rec.port > 3080 && rec.port <= 3080 + 200,
            "没有 web profile 时也应以 3080 为基准错开，实际 {}",
            rec.port
        );
        let pkg: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(profiles.join(RECOVERY_PROFILE).join("package.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            pkg.pointer("/dsh/profile/bundles").unwrap().as_array().unwrap(),
            &vec![
                serde_json::Value::String("@deepseek-ai/dsh-base".into()),
                serde_json::Value::String("@deepseek-ai/dsh-web-app".into()),
            ],
            "漂移出来的第三方 bundle 应被裁掉"
        );
        assert!(
            pkg.get("dependencies")
                .and_then(|d| d.as_object())
                .map(|d| d.is_empty())
                .unwrap_or(true),
            "第三方依赖应一并移除"
        );
        let cfg = get_web_quick_config(RECOVERY_PROFILE).unwrap();
        assert_eq!(cfg.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(cfg.port, Some(rec.port as u64));
        assert_eq!(cfg.open_browser, Some(true));
        assert_eq!(cfg.surface_context, Some(true));
        assert_eq!(cfg.cookie_max_age_days, Some(36500));

        // 2) dsh 没建出骨架：报错且不留残骸
        delete_profile(RECOVERY_PROFILE).unwrap();
        let err = create_recovery_profile_with(&crate::settings::Settings::default(), |_s, _n, _t| Ok(()))
            .unwrap_err();
        assert!(err.contains("package.json"), "应指出骨架缺失，实际：{err}");
        assert!(
            !profiles.join(RECOVERY_PROFILE).exists(),
            "建了一半的 profile 必须回滚"
        );

        // 3) dsh 自身失败：错误原样上抛
        let err = create_recovery_profile_with(
            &crate::settings::Settings::default(),
            |_s, _n, _t| Err("dsh 初始化 profile 失败：unknown default profile".into()),
        )
        .unwrap_err();
        assert!(err.contains("unknown default profile"));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
        std::env::remove_var("DSH_STARTER_HOME");
    }
}
